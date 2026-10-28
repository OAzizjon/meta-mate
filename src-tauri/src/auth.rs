//! Sign-in with Google or GitHub (D54): the browser does the provider's pages, the proxy
//! holds the client secrets, and the app only ever sees a one-time login code on its own
//! loopback port, which it trades with its PKCE verifier for a session token. The token
//! replaces the built-in invite for every call (`Proxy::set_session`) and is kept in the
//! app's config folder until the person signs out.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::proxy::Proxy;

const FILE_NAME: &str = "session.json";
/// How long the browser has to come back: signing in may need a password and a code.
const BROWSER_WAIT: Duration = Duration::from_secs(300);
const CHANGED_EVENT: &str = "auth-changed";
const PROVIDERS: [&str; 2] = ["google", "github"];

/// What the settings window shows (src/settings/account.ts).
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AuthStatus {
    pub signed_in: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
}

/// The kept session. Debug never prints the token.
#[derive(Serialize, Deserialize, Clone)]
struct Session {
    token: String,
    name: String,
    provider: String,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("provider", &self.provider)
            .finish_non_exhaustive()
    }
}

/// The signed-in session in memory, and whether a sign-in is under way.
#[derive(Default)]
pub struct Auth {
    session: Mutex<Option<Session>>,
    busy: AtomicBool,
}

impl Auth {
    fn status(&self) -> AuthStatus {
        let session = self.session.lock().ok().and_then(|session| session.clone());
        AuthStatus {
            signed_in: session.is_some(),
            name: session.as_ref().map(|s| s.name.clone()),
            provider: session.map(|s| s.provider),
        }
    }
}

/// At startup: the kept session (if any) goes into the proxy client before the first call.
pub fn restore(app: &AppHandle, proxy: &Proxy) {
    let Some(session) = load(app) else {
        return;
    };
    proxy.set_session(Some(session.token.clone()));
    if let Some(auth) = app.try_state::<Auth>()
        && let Ok(mut current) = auth.session.lock()
    {
        *current = Some(session);
    }
}

#[tauri::command]
pub fn auth_status(auth: State<'_, Auth>) -> AuthStatus {
    auth.status()
}

#[tauri::command]
pub async fn sign_in(app: AppHandle, provider: String) -> Result<AuthStatus, String> {
    if !PROVIDERS.contains(&provider.as_str()) {
        return Err("auth.unavailable".into());
    }
    let (Some(auth), Some(proxy)) = (app.try_state::<Auth>(), app.try_state::<Proxy>()) else {
        return Err("auth.unavailable".into());
    };
    if auth.busy.swap(true, Ordering::AcqRel) {
        return Err("auth.busy".into());
    }
    let proxy = proxy.inner().clone();
    let result = sign_in_with(&app, &proxy, &provider).await;
    auth.busy.store(false, Ordering::Release);
    let session = result?;
    proxy.set_session(Some(session.token.clone()));
    if let Err(error) = save(&app, &session) {
        // Signed in for this run all the same; the next start asks again.
        eprintln!("auth: could not keep the session: {error}");
    }
    if let Ok(mut current) = auth.session.lock() {
        *current = Some(session);
    }
    let status = auth.status();
    let _ = app.emit(CHANGED_EVENT, status.clone());
    Ok(status)
}

#[tauri::command]
pub fn sign_out(app: AppHandle, auth: State<'_, Auth>) {
    if let Ok(mut current) = auth.session.lock() {
        *current = None;
    }
    if let Some(proxy) = app.try_state::<Proxy>() {
        proxy.set_session(None);
    }
    if let Some(path) = file(&app) {
        let _ = std::fs::remove_file(path);
    }
    let _ = app.emit(CHANGED_EVENT, auth.status());
}

async fn sign_in_with(app: &AppHandle, proxy: &Proxy, provider: &str) -> Result<Session, String> {
    let verifier = random_url_safe::<32>().ok_or("auth.failed")?;
    let state = random_url_safe::<24>().ok_or("auth.failed")?;
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|_| "auth.failed")?;
    let port = listener.local_addr().map_err(|_| "auth.failed")?.port();
    let start = format!(
        "{}/auth/{provider}/start?port={port}&state={state}&challenge={challenge}",
        proxy.base()
    );
    if !mm_win32::shell::open_url(&start) {
        return Err("auth.unavailable".into());
    }
    let pages = Pages {
        done: crate::locale_text(&locale(app), "auth.browser_done"),
        failed: crate::locale_text(&locale(app), "auth.browser_failed"),
    };
    let expected = state.clone();
    let code =
        tauri::async_runtime::spawn_blocking(move || wait_for_code(&listener, &expected, &pages))
            .await
            .map_err(|_| "auth.failed")??;
    let answer = proxy
        .redeem_login(&code, &verifier)
        .await
        .map_err(|_| "auth.failed")?;
    let text = |key: &str| {
        answer
            .get(key)
            .and_then(|value| value.as_str())
            .map(str::to_owned)
    };
    match (text("token"), text("name"), text("provider")) {
        (Some(token), Some(name), Some(provider)) if token.starts_with("mms1.") => Ok(Session {
            token,
            name,
            provider,
        }),
        _ => Err("auth.failed".into()),
    }
}

/// The browser tab's last page, in the app's language.
struct Pages {
    done: String,
    failed: String,
}

/// Waits for the browser's one request to `/callback?code=..&state=..` and answers it.
/// `auth.cancelled` when nobody comes back in time; a wrong state is `auth.failed`.
fn wait_for_code(listener: &TcpListener, expected: &str, pages: &Pages) -> Result<String, String> {
    listener.set_nonblocking(true).map_err(|_| "auth.failed")?;
    let started = Instant::now();
    loop {
        if started.elapsed() > BROWSER_WAIT {
            return Err("auth.cancelled".into());
        }
        let mut stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            Err(_) => return Err("auth.failed".into()),
        };
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut buffer = [0u8; 8192];
        let read = stream.read(&mut buffer).unwrap_or(0);
        let request = String::from_utf8_lossy(&buffer[..read]);
        let Some(found) = callback_params(&request) else {
            // Something else (a favicon): not ours, keep waiting.
            let _ = stream.write_all(
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
            continue;
        };
        let ok = found.1 == expected && !found.0.is_empty();
        let page = page(if ok { &pages.done } else { &pages.failed });
        let _ = stream.write_all(page.as_bytes());
        return if ok {
            Ok(found.0)
        } else {
            Err("auth.failed".into())
        };
    }
}

/// `(code, state)` from a request line `GET /callback?code=..&state=.. HTTP/1.1`.
fn callback_params(request: &str) -> Option<(String, String)> {
    let line = request.lines().next()?;
    let target = line.strip_prefix("GET ")?.split(' ').next()?;
    let url = reqwest::Url::parse(&format!("http://127.0.0.1{target}")).ok()?;
    if url.path() != "/callback" {
        return None;
    }
    let mut code = None;
    let mut state = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "code" if value.len() <= 512 => code = Some(value.into_owned()),
            "state" if value.len() <= 256 => state = Some(value.into_owned()),
            _ => {}
        }
    }
    Some((code?, state?))
}

fn page(text: &str) -> String {
    let escaped = text
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let body = format!(
        "<!doctype html><meta charset=\"utf-8\"><title>MetaMate</title>\
         <p style=\"font:22px system-ui;margin:3em\">{escaped}</p>"
    );
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn random_url_safe<const N: usize>() -> Option<String> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).ok()?;
    Some(URL_SAFE_NO_PAD.encode(bytes))
}

fn locale(app: &AppHandle) -> String {
    app.try_state::<crate::answer_stream::AnswerStream>()
        .map(|stream| stream.locale())
        .unwrap_or_else(|| "en".into())
}

fn file(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join(FILE_NAME))
}

fn load(app: &AppHandle) -> Option<Session> {
    let text = std::fs::read_to_string(file(app)?).ok()?;
    let session: Session = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    session.token.starts_with("mms1.").then_some(session)
}

/// Written next to settings.json, through a temporary file renamed over the old one.
fn save(app: &AppHandle, session: &Session) -> std::io::Result<()> {
    let path = file(app).ok_or_else(|| std::io::Error::other("no config folder"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let temporary = path.with_extension("json.tmp");
    let text = serde_json::to_vec(session).map_err(std::io::Error::other)?;
    std::fs::write(&temporary, text)?;
    std::fs::rename(&temporary, &path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_callback_with_both_values_counts() {
        assert_eq!(
            callback_params("GET /callback?code=abc&state=xyz HTTP/1.1\r\nHost: x\r\n\r\n"),
            Some(("abc".into(), "xyz".into()))
        );
        assert_eq!(
            callback_params("GET /callback?state=xyz&code=a%2Bb HTTP/1.1"),
            Some(("a+b".into(), "xyz".into()))
        );
        for request in [
            "GET /favicon.ico HTTP/1.1",
            "GET /callback?code=abc HTTP/1.1",
            "POST /callback?code=a&state=b HTTP/1.1",
            "GET //evil.example/callback?code=a&state=b HTTP/1.1",
            "",
        ] {
            assert_eq!(callback_params(request), None, "{request}");
        }
    }

    #[test]
    fn the_browser_page_escapes_its_text() {
        let page = page("<b>&</b>");
        assert!(page.contains("&lt;b&gt;&amp;&lt;/b&gt;"));
        assert!(page.starts_with("HTTP/1.1 200 OK\r\n"));
    }

    #[test]
    fn verifiers_are_long_enough_for_pkce() {
        let verifier = random_url_safe::<32>().unwrap();
        assert_eq!(verifier.len(), 43);
        assert_ne!(verifier, random_url_safe::<32>().unwrap());
    }

    #[test]
    fn debug_never_shows_the_token() {
        let session = Session {
            token: "mms1.secret".into(),
            name: "Ann".into(),
            provider: "google".into(),
        };
        assert!(!format!("{session:?}").contains("mms1"));
    }
}

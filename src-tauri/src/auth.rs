//! Sign-in with Google or GitHub (D54): the browser does the provider's pages, the proxy
//! holds the client secrets, and the app only ever sees a one-time login code on its own
//! loopback port, which it trades with its PKCE verifier for a session token. The token
//! replaces the built-in invite for every call (`Proxy::set_session`) and is kept in the
//! app's config folder, sealed for this Windows user (DPAPI), until the person signs out.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::proxy::Proxy;

const FILE_NAME: &str = "session.bin";
/// Builds before DPAPI kept the token readable here: removed when found.
const PLAIN_FILE_NAME: &str = "session.json";
/// How long the browser has to come back: signing in may need a password and a code.
const BROWSER_WAIT: Duration = Duration::from_secs(300);
/// One connection to the loopback port: its request line within this time and size.
const REQUEST_WAIT: Duration = Duration::from_secs(5);
const REQUEST_MAX_BYTES: usize = 8192;
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

/// The signed-in session in memory, whether a sign-in is under way, and how many times the
/// person signed out: a sign-in that started before a sign-out never lands after it.
#[derive(Default)]
pub struct Auth {
    session: Mutex<Option<Session>>,
    busy: AtomicBool,
    sign_outs: AtomicU64,
}

/// Clears `busy` however the sign-in ends.
struct Busy<'a>(&'a AtomicBool);

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
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
    let _busy = Busy(&auth.busy);
    let sign_outs = auth.sign_outs.load(Ordering::Acquire);
    let proxy = proxy.inner().clone();
    let session = sign_in_with(&app, &proxy, &provider).await?;
    {
        // Under the session lock, which `sign_out` holds while it counts: either the
        // sign-out comes after this and undoes it, or it came before and this is dropped.
        let Ok(mut current) = auth.session.lock() else {
            return Err("auth.failed".into());
        };
        if auth.sign_outs.load(Ordering::Acquire) != sign_outs {
            return Err("auth.cancelled".into());
        }
        proxy.set_session(Some(session.token.clone()));
        if let Err(error) = save(&app, &session) {
            // Signed in for this run all the same; the next start asks again.
            eprintln!("auth: could not keep the session: {error}");
        }
        *current = Some(session);
    }
    let status = auth.status();
    let _ = app.emit(CHANGED_EVENT, status.clone());
    Ok(status)
}

#[tauri::command]
pub fn sign_out(app: AppHandle, auth: State<'_, Auth>) {
    {
        let mut current = auth
            .session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        auth.sign_outs.fetch_add(1, Ordering::AcqRel);
        *current = None;
        if let Some(proxy) = app.try_state::<Proxy>() {
            proxy.set_session(None);
        }
        for name in [FILE_NAME, PLAIN_FILE_NAME] {
            if let Some(path) = file(&app, name) {
                let _ = std::fs::remove_file(path);
            }
        }
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

/// Waits for the browser's request to `/callback?code=..&state=..` and answers it.
/// `auth.cancelled` when nobody comes back in time. A callback with another state or no
/// code gets the "failed" page and the wait goes on: another program on this computer that
/// finds the port cannot end the person's sign-in.
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
        let _ = stream.set_read_timeout(Some(REQUEST_WAIT));
        let _ = stream.set_write_timeout(Some(REQUEST_WAIT));
        let request = request_line(&mut stream, Instant::now() + REQUEST_WAIT);
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
        if ok {
            return Ok(found.0);
        }
    }
}

/// The request's first line (through CRLF), read in as many pieces as TCP delivers it, at
/// most `REQUEST_MAX_BYTES` and until `deadline`; whatever arrived when it stops.
fn request_line(stream: &mut impl Read, deadline: Instant) -> String {
    let mut received = Vec::new();
    let mut chunk = [0u8; 1024];
    while received.len() < REQUEST_MAX_BYTES && Instant::now() < deadline {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(read) => received.extend_from_slice(&chunk[..read]),
        }
        if received.windows(2).any(|pair| pair == b"\r\n") {
            break;
        }
    }
    received.truncate(REQUEST_MAX_BYTES);
    String::from_utf8_lossy(&received).into_owned()
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

fn file(app: &AppHandle, name: &str) -> Option<std::path::PathBuf> {
    app.path().app_config_dir().ok().map(|dir| dir.join(name))
}

fn load(app: &AppHandle) -> Option<Session> {
    if let Some(plain) = file(app, PLAIN_FILE_NAME) {
        // A readable token from an older build: signing in again is the safe way back.
        let _ = std::fs::remove_file(plain);
    }
    let sealed = std::fs::read(file(app, FILE_NAME)?).ok()?;
    let session: Session = serde_json::from_slice(&mm_win32::dpapi::unprotect(&sealed)?).ok()?;
    session.token.starts_with("mms1.").then_some(session)
}

/// Sealed for this Windows user and written next to settings.json, through a temporary file
/// renamed over the old one.
fn save(app: &AppHandle, session: &Session) -> std::io::Result<()> {
    let path = file(app, FILE_NAME).ok_or_else(|| std::io::Error::other("no config folder"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_vec(session).map_err(std::io::Error::other)?;
    let sealed =
        mm_win32::dpapi::protect(&text).ok_or_else(|| std::io::Error::other("DPAPI refused"))?;
    let temporary = path.with_extension("bin.tmp");
    std::fs::write(&temporary, sealed)?;
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

    /// Hands out its bytes a few at a time, like TCP may.
    struct Pieces(Vec<u8>, usize);

    impl Read for Pieces {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let end = (self.1 + 7).min(self.0.len()).min(self.1 + buffer.len());
            let piece = &self.0[self.1..end];
            buffer[..piece.len()].copy_from_slice(piece);
            self.1 = end;
            Ok(piece.len())
        }
    }

    #[test]
    fn a_request_line_split_by_tcp_is_read_whole() {
        let request = b"GET /callback?code=abc&state=xyz-long-state HTTP/1.1\r\nHost: x\r\n\r\n";
        let line = request_line(
            &mut Pieces(request.to_vec(), 0),
            Instant::now() + Duration::from_secs(5),
        );
        assert_eq!(
            callback_params(&line),
            Some(("abc".into(), "xyz-long-state".into()))
        );
        let endless = vec![b'a'; REQUEST_MAX_BYTES * 2];
        let line = request_line(
            &mut Pieces(endless, 0),
            Instant::now() + Duration::from_secs(5),
        );
        assert_eq!(line.len(), REQUEST_MAX_BYTES);
    }

    #[test]
    fn a_wrong_callback_does_not_end_the_wait() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let browser = std::thread::spawn(move || {
            let mut pages = Vec::new();
            for target in [
                "/callback?code=planted&state=wrong-state",
                "/callback?code=&state=right-state",
                "/callback?code=real&state=right-state",
            ] {
                let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
                write!(stream, "GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").unwrap();
                let mut page = String::new();
                let _ = stream.read_to_string(&mut page);
                pages.push(page);
            }
            pages
        });
        let pages = Pages {
            done: "done".into(),
            failed: "failed".into(),
        };
        assert_eq!(
            wait_for_code(&listener, "right-state", &pages),
            Ok("real".into())
        );
        let seen = browser.join().unwrap();
        assert!(seen[0].ends_with("failed</p>") && seen[1].ends_with("failed</p>"));
        assert!(seen[2].ends_with("done</p>"));
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

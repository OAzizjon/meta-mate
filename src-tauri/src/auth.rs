//! Sign-in with Google or GitHub (D54): the browser does the provider's pages, the proxy
//! holds the client secrets, and the app only ever sees a one-time login code on its own
//! loopback port, which it trades with its PKCE verifier for a session token. The token
//! replaces the built-in invite for every call (`Proxy::set_session`) and is kept in the
//! app's local data folder, sealed for this Windows user (DPAPI), until the person signs out.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::proxy::Proxy;

const FILE_NAME: &str = "session.bin";
const TEMPORARY_FILE_NAME: &str = "session.bin.tmp";
/// Builds before DPAPI kept the token readable here: removed when found.
const PLAIN_FILE_NAME: &str = "session.json";
/// How long the browser has to come back: signing in may need a password and a code.
const BROWSER_WAIT: Duration = Duration::from_secs(300);
/// One connection to the loopback port: its request line within this time and size (one
/// read may wait `READ_WAIT` more).
const REQUEST_WAIT: Duration = Duration::from_secs(2);
const READ_WAIT: Duration = Duration::from_secs(1);
/// A second click on the same provider within this time joins the sign-in already waiting
/// in the browser instead of opening one more tab; later, it is a forgotten tab and a new
/// sign-in replaces it.
const JOIN_WAIT: Duration = Duration::from_secs(120);
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

/// The signed-in session in memory, and a count of sign-ins and sign-outs: only the latest
/// sign-in may land, so a new one replaces a forgotten browser tab's (never "busy" for
/// minutes) and a sign-in that started before a sign-out never lands after it.
#[derive(Default)]
pub struct Auth {
    session: Mutex<Option<Session>>,
    attempts: AtomicU64,
    waiting: Mutex<Option<Waiting>>,
}

/// The sign-in whose browser tab is open now. The sign-in window and Settings both have the
/// buttons, and a window opened again forgets it is waiting: a click there joins this one.
struct Waiting {
    provider: String,
    started: Instant,
    attempt: u64,
    outcome: Arc<Outcome>,
}

type Outcome = tokio::sync::watch::Sender<Option<Result<AuthStatus, String>>>;

/// The first outcome stays: a sign-out's "cancelled" is never overwritten by the sign-in it
/// ended, and a success published under the session lock is never turned into a failure.
fn settle(outcome: &Outcome, result: Result<AuthStatus, String>) {
    outcome.send_if_modified(|current| {
        if current.is_some() {
            return false;
        }
        *current = Some(result);
        true
    });
}

impl Auth {
    fn status(&self) -> AuthStatus {
        let session = self.session.lock().ok().and_then(|session| session.clone());
        status_of(session.as_ref())
    }

    /// Whether a session is kept (signin.rs: Mate answers only then when required).
    pub(crate) fn signed_in(&self) -> bool {
        self.session.lock().is_ok_and(|session| session.is_some())
    }

    fn is_latest(&self, attempt: u64) -> bool {
        self.attempts.load(Ordering::Acquire) == attempt
    }
}

fn status_of(session: Option<&Session>) -> AuthStatus {
    AuthStatus {
        signed_in: session.is_some(),
        name: session.map(|s| s.name.clone()),
        provider: session.map(|s| s.provider.clone()),
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
pub async fn auth_providers(app: AppHandle) -> Vec<String> {
    // Older proxies have no discovery route: keep Google available on any failure.
    let fallback = || vec!["google".to_owned()];
    let Some(proxy) = app.try_state::<Proxy>() else {
        return fallback();
    };
    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
    else {
        return fallback();
    };
    #[derive(Deserialize)]
    struct Providers {
        providers: Vec<String>,
    }
    let Ok(response) = client
        .get(format!("{}/auth/providers", proxy.base()))
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
    else {
        return fallback();
    };
    match response.json::<Providers>().await {
        Ok(answer) => answer
            .providers
            .into_iter()
            .filter(|provider| PROVIDERS.contains(&provider.as_str()))
            .collect(),
        Err(_) => fallback(),
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
    let Some(auth) = app.try_state::<Auth>() else {
        return Err("auth.unavailable".into());
    };
    // Joined or started under one lock, with its attempt number, so two quick clicks never
    // open two tabs and a click on the other provider always gets the newest attempt.
    let (outcome, attempt) = {
        let mut waiting = auth
            .waiting
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let joinable = waiting.as_ref().filter(|w| {
            w.provider == provider
                && w.started.elapsed() < JOIN_WAIT
                && auth.is_latest(w.attempt)
                && w.outcome.borrow().is_none()
        });
        match joinable {
            Some(w) => (Arc::clone(&w.outcome), None),
            None => {
                let outcome = Arc::new(Outcome::new(None));
                let attempt = auth.attempts.fetch_add(1, Ordering::AcqRel) + 1;
                *waiting = Some(Waiting {
                    provider: provider.clone(),
                    started: Instant::now(),
                    attempt,
                    outcome: Arc::clone(&outcome),
                });
                (outcome, Some(attempt))
            }
        }
    };
    let Some(attempt) = attempt else {
        let mut joined = outcome.subscribe();
        return match joined.wait_for(Option::is_some).await {
            Ok(result) => result.clone().unwrap_or_else(|| Err("auth.failed".into())),
            Err(_) => Err("auth.cancelled".into()),
        };
    };
    let result = sign_in_once(&app, &auth, &provider, attempt, &outcome).await;
    settle(&outcome, result);
    // Every caller gets the settled outcome: a sign-out's "cancelled" wins for this one too.
    let result = outcome
        .borrow()
        .clone()
        .unwrap_or_else(|| Err("auth.failed".into()));
    let mut waiting = auth
        .waiting
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // A newer sign-in (another provider, or after JOIN_WAIT) keeps its own entry.
    if waiting
        .as_ref()
        .is_some_and(|w| Arc::ptr_eq(&w.outcome, &outcome))
    {
        *waiting = None;
    }
    result
}

async fn sign_in_once(
    app: &AppHandle,
    auth: &Auth,
    provider: &str,
    attempt: u64,
    outcome: &Outcome,
) -> Result<AuthStatus, String> {
    let Some(proxy) = app.try_state::<Proxy>() else {
        return Err("auth.unavailable".into());
    };
    let proxy = proxy.inner().clone();
    let session = sign_in_with(app, &proxy, provider, attempt).await?;
    // Under the session lock, which `sign_out` holds while it counts: either the sign-out
    // comes after this and undoes it, or it came before and this is dropped. The event goes
    // out under the lock too, so the windows never see the two in the wrong order.
    let Ok(mut current) = auth.session.lock() else {
        return Err("auth.failed".into());
    };
    if !auth.is_latest(attempt) {
        return Err("auth.cancelled".into());
    }
    proxy.set_session(Some(session.token.clone()));
    // Signed in: the trial's token is not needed any more.
    proxy.set_guest(None);
    if let Err(error) = save(app, &session) {
        // Signed in for this run all the same; the next start asks again.
        eprintln!("auth: could not keep the session: {error}");
    }
    let status = status_of(Some(&session));
    *current = Some(session);
    let _ = app.emit(CHANGED_EVENT, status.clone());
    // Joined clicks hear of it before a sign-out can take the session lock.
    settle(outcome, Ok(status.clone()));
    crate::signin::close(app);
    Ok(status)
}

#[tauri::command]
pub fn sign_out(app: AppHandle, auth: State<'_, Auth>) {
    forget(&app, &auth, None);
}

/// When the proxy refuses the kept session (its signing key was rotated, the account was
/// revoked), Mate signs out as if the person had, and the sign-in window opens: never
/// "I can't connect" on every question until someone signs out by hand.
pub fn watch_rejected(app: &AppHandle, proxy: &Proxy) {
    let mut rejected = proxy.watch_rejected();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        while rejected.changed().await.is_ok() {
            let Some(token) = rejected.borrow_and_update().clone() else {
                continue;
            };
            let Some(auth) = app.try_state::<Auth>() else {
                continue;
            };
            if forget(&app, &auth, Some(&token)) {
                eprintln!("auth: the proxy refused the kept session; signed out");
            }
        }
    });
}

/// Signs out (`refused: None`), or drops the session the proxy refused (`Some(token)`) and
/// returns whether it was still the kept one: compared under the session lock, so a sign-in
/// that landed after the refused call keeps its new session. A refused session leaves a
/// sign-in that is waiting for its browser tab alone: that is the way back in.
fn forget(app: &AppHandle, auth: &Auth, refused: Option<&str>) -> bool {
    {
        let mut current = auth
            .session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(token) = refused {
            if current
                .as_ref()
                .is_none_or(|session| session.token != token)
            {
                return false;
            }
        } else {
            // A sign-in still waiting for its browser tab ends too, and so do the clicks that
            // joined it: at once, not when its tab gives up. Counted under the lock that
            // sign-ins register under, so none starts between the count and the removal.
            let mut waiting = auth
                .waiting
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            auth.attempts.fetch_add(1, Ordering::AcqRel);
            if let Some(waiting) = waiting.take() {
                settle(&waiting.outcome, Err("auth.cancelled".into()));
            }
        }
        *current = None;
        if let Some(proxy) = app.try_state::<Proxy>() {
            proxy.set_session(None);
        }
        for name in [FILE_NAME, TEMPORARY_FILE_NAME, PLAIN_FILE_NAME] {
            for path in [file(app, name), old_file(app, name)].into_iter().flatten() {
                let _ = std::fs::remove_file(path);
            }
        }
        let _ = app.emit(CHANGED_EVENT, status_of(None));
    }
    // Signing in is required: the way back in opens at once, with the guest trial.
    if crate::signin::required() {
        if let Some(proxy) = app.try_state::<Proxy>() {
            crate::signin::start_guest(app, proxy.inner());
        }
        crate::signin::open_later(app);
    }
    true
}

async fn sign_in_with(
    app: &AppHandle,
    proxy: &Proxy,
    provider: &str,
    attempt: u64,
) -> Result<Session, String> {
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
    let checker = app.clone();
    let wanted = move || {
        checker
            .try_state::<Auth>()
            .is_some_and(|auth| auth.is_latest(attempt))
    };
    let code = tauri::async_runtime::spawn_blocking(move || {
        wait_for_code(&listener, &expected, &pages, wanted)
    })
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
/// `auth.cancelled` when nobody comes back in time or `wanted` turns false (a newer sign-in,
/// a sign-out). A callback with another state or no code gets the "failed" page and the
/// wait goes on, and so does a connection that fails: another program on this computer
/// that finds the port cannot end the person's sign-in.
fn wait_for_code(
    listener: &TcpListener,
    expected: &str,
    pages: &Pages,
    wanted: impl Fn() -> bool,
) -> Result<String, String> {
    listener.set_nonblocking(true).map_err(|_| "auth.failed")?;
    let started = Instant::now();
    loop {
        if started.elapsed() > BROWSER_WAIT || !wanted() {
            return Err("auth.cancelled".into());
        }
        let mut stream = match listener.accept() {
            Ok((stream, _)) => stream,
            // WouldBlock, or a connection reset before it was taken: wait for the next.
            Err(_) => {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
        };
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(READ_WAIT));
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
        let page = page(if ok { &pages.done } else { &pages.failed }, ok);
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

/// The tab's last page. After a sign-in that worked it tries to close itself: browsers allow
/// that only to a tab whose history is this one page (the provider signed the person in
/// without a page of its own); otherwise its text says to close it.
fn page(text: &str, done: bool) -> String {
    let escaped = text
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let (mark, close) = if done {
        (
            "\u{2713}",
            "<script>setTimeout(()=>window.close(),1500)</script>",
        )
    } else {
        ("!", "")
    };
    let body = format!(
        "<!doctype html><meta charset=\"utf-8\"><meta name=\"color-scheme\" content=\"light dark\">\
         <title>MetaMate</title><body style=\"margin:0;min-height:100vh;display:grid;\
         place-items:center;font:22px/1.45 system-ui,sans-serif\"><main style=\"max-width:28em;\
         padding:2em;text-align:center\"><div style=\"width:64px;height:64px;margin:0 auto 1em;\
         border-radius:50%;background:#ffd76a;color:#3a2a00;display:grid;place-items:center;\
         font-size:34px;box-shadow:0 0 40px #ffd76a88\">{mark}</div><p style=\"margin:0\">\
         {escaped}</p></main>{close}</body>"
    );
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

pub(crate) fn random_url_safe<const N: usize>() -> Option<String> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).ok()?;
    Some(URL_SAFE_NO_PAD.encode(bytes))
}

fn locale(app: &AppHandle) -> String {
    app.try_state::<crate::answer_stream::AnswerStream>()
        .map(|stream| stream.locale())
        .unwrap_or_else(|| "en".into())
}

/// In the local (not roaming) app data: DPAPI keys roam with a domain profile, the
/// session should not.
fn file(app: &AppHandle, name: &str) -> Option<std::path::PathBuf> {
    app.path()
        .app_local_data_dir()
        .ok()
        .map(|dir| dir.join(name))
}

/// Where builds of 2026-10-28 kept it (roaming).
fn old_file(app: &AppHandle, name: &str) -> Option<std::path::PathBuf> {
    app.path().app_config_dir().ok().map(|dir| dir.join(name))
}

fn load(app: &AppHandle) -> Option<Session> {
    for name in [PLAIN_FILE_NAME, FILE_NAME, TEMPORARY_FILE_NAME] {
        // A readable or roaming token from an older build: signing in again is the safe
        // way back.
        if let Some(old) = old_file(app, name) {
            let _ = std::fs::remove_file(old);
        }
    }
    let sealed = std::fs::read(file(app, FILE_NAME)?).ok()?;
    let session: Session = serde_json::from_slice(&mm_win32::dpapi::unprotect(&sealed)?).ok()?;
    session.token.starts_with("mms1.").then_some(session)
}

/// Sealed for this Windows user, through a temporary file renamed over the old one; the
/// temporary file never outlives a failure.
fn save(app: &AppHandle, session: &Session) -> std::io::Result<()> {
    let path = file(app, FILE_NAME).ok_or_else(|| std::io::Error::other("no data folder"))?;
    let temporary =
        file(app, TEMPORARY_FILE_NAME).ok_or_else(|| std::io::Error::other("no data folder"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_vec(session).map_err(std::io::Error::other)?;
    let sealed =
        mm_win32::dpapi::protect(&text).ok_or_else(|| std::io::Error::other("DPAPI refused"))?;
    let written = std::fs::File::create(&temporary).and_then(|mut file| {
        file.write_all(&sealed)?;
        // Before the rename: a power cut never leaves an empty session file.
        file.sync_all()
    });
    let renamed = written.and_then(|()| std::fs::rename(&temporary, &path));
    if renamed.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    renamed
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
            wait_for_code(&listener, "right-state", &pages, || true),
            Ok("real".into())
        );
        let seen = browser.join().unwrap();
        let says = |page: &str, text: &str| page.contains(&format!(">{text}</p>"));
        assert!(says(&seen[0], "failed") && says(&seen[1], "failed"));
        assert!(!seen[0].contains("window.close") && !seen[1].contains("window.close"));
        assert!(says(&seen[2], "done") && seen[2].contains("window.close"));
    }

    #[test]
    fn a_newer_sign_in_ends_the_wait() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let pages = Pages {
            done: "done".into(),
            failed: "failed".into(),
        };
        let started = Instant::now();
        assert_eq!(
            wait_for_code(&listener, "state", &pages, || false),
            Err("auth.cancelled".into())
        );
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn the_browser_page_escapes_its_text() {
        let page = page("<b>&</b>", false);
        assert!(page.contains("&lt;b&gt;&amp;&lt;/b&gt;"));
        assert!(page.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(!page.contains("window.close"));
        assert!(super::page("ok", true).contains("window.close"));
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

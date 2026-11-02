//! Minimal, safe self-update for Windows (NSIS installer in `currentUser` mode).
//!
//! Checks `https://meta-mate.stream/version.json` 60 s after startup and then every 12 hours
//! (never in debug builds unless `MM_UPDATE_URL` is set). Installing only ever runs after the
//! person clicks the tray item or the Settings update button: it downloads the setup `.exe`
//! to the app's local data folder, verifies its SHA-256, spawns the silent installer with a follow-up restart
//! (`CREATE_NO_WINDOW`, invariant 1) and exits MetaMate so its files can be replaced.

use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const DEFAULT_UPDATE_URL: &str = "https://meta-mate.stream/version.json";
pub const ALLOWED_URL_PREFIX: &str = "https://meta-mate.stream/";
pub const MANIFEST_MAX_BYTES: usize = 8 * 1024;
pub const INSTALLER_MAX_BYTES: usize = 80 * 1024 * 1024;
pub const NOTE_MAX_CHARS: usize = 200;

const STARTUP_DELAY: Duration = Duration::from_secs(60);
const CHECK_INTERVAL: Duration = Duration::from_secs(12 * 60 * 60);
const MANIFEST_TIMEOUT: Duration = Duration::from_secs(10);
const INSTALLER_TIMEOUT: Duration = Duration::from_secs(120);
const EXIT_DELAY: Duration = Duration::from_secs(1);
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateNotes {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub en: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ru: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateManifest {
    pub version: String,
    pub url: String,
    pub sha256: String,
    /// The installer's size for the landing page (`mm release`); not used here.
    #[serde(default)]
    pub size_mb: Option<u64>,
    #[serde(default)]
    pub notes: Option<UpdateNotes>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct UpdateStatus {
    pub current: String,
    pub available: Option<String>,
    pub notes: Option<UpdateNotes>,
}

#[derive(Default)]
pub struct Updater {
    inner: Mutex<UpdaterInner>,
    checking: tokio::sync::Mutex<()>,
    started: AtomicBool,
    installing: AtomicBool,
}

#[derive(Default)]
struct UpdaterInner {
    available: Option<UpdateManifest>,
    announced_version: Option<String>,
    last_checked: Option<Instant>,
}

impl Updater {
    pub fn available_version(&self) -> Option<String> {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .available
            .as_ref()
            .map(|manifest| manifest.version.clone())
    }

    pub fn status(&self) -> UpdateStatus {
        let guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        UpdateStatus {
            current: CURRENT_VERSION.to_owned(),
            available: guard.available.as_ref().map(|m| m.version.clone()),
            notes: guard.available.as_ref().and_then(|m| m.notes.clone()),
        }
    }
}

/// Parses a strict `MAJOR.MINOR.PATCH` version with no leading zeros or suffixes.
pub fn parse_semver(version: &str) -> Option<(u32, u32, u32)> {
    let mut parts = version.split('.');
    let major = parse_semver_part(parts.next()?)?;
    let minor = parse_semver_part(parts.next()?)?;
    let patch = parse_semver_part(parts.next()?)?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

fn parse_semver_part(part: &str) -> Option<u32> {
    if part.is_empty()
        || (part.len() > 1 && part.starts_with('0'))
        || !part.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    part.parse::<u32>().ok()
}

/// True only when both versions are valid `MAJOR.MINOR.PATCH` and `candidate` is strictly newer.
pub fn is_newer_version(current: &str, candidate: &str) -> bool {
    let Some(curr) = parse_semver(current) else {
        return false;
    };
    let Some(cand) = parse_semver(candidate) else {
        return false;
    };
    cand > curr
}

/// Compares `candidate` against `env!("CARGO_PKG_VERSION")`.
pub fn is_newer(candidate: &str) -> bool {
    is_newer_version(CURRENT_VERSION, candidate)
}

/// True when `url` starts with `https://meta-mate.stream/` and has no whitespace, quotes or control chars.
pub fn is_valid_url(url: &str) -> bool {
    url.starts_with(ALLOWED_URL_PREFIX)
        && url.len() > ALLOWED_URL_PREFIX.len()
        && !url
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '"')
}

/// True when `sha256` is exactly 64 lowercase ASCII hex digits.
pub fn is_valid_sha256(sha256: &str) -> bool {
    sha256.len() == 64
        && sha256
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn is_valid_note(note: &str) -> bool {
    note.chars().count() <= NOTE_MAX_CHARS && !note.chars().any(char::is_control)
}

/// Strictly parses and validates a `version.json` body (<= 8 KB).
pub fn parse_manifest(body: &[u8]) -> Result<UpdateManifest, &'static str> {
    if body.len() > MANIFEST_MAX_BYTES {
        return Err("manifest exceeds 8 KB");
    }
    let manifest: UpdateManifest =
        serde_json::from_slice(body).map_err(|_| "invalid manifest JSON")?;
    if parse_semver(&manifest.version).is_none() {
        return Err("invalid version");
    }
    if !is_valid_url(&manifest.url) {
        return Err("invalid update url");
    }
    if !is_valid_sha256(&manifest.sha256) {
        return Err("invalid sha256");
    }
    if let Some(notes) = &manifest.notes
        && (!notes.en.as_deref().is_none_or(is_valid_note)
            || !notes.ru.as_deref().is_none_or(is_valid_note))
    {
        return Err("invalid release notes");
    }
    Ok(manifest)
}

/// Lowercase 64-character hex SHA-256 of `bytes`.
#[cfg(test)]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(&mut hex, "{byte:02x}");
    }
    hex
}

/// Verifies `bytes` against a 64-character lowercase hex SHA-256 digest.
#[cfg(test)]
pub fn verify_sha256(bytes: &[u8], expected_hex: &str) -> bool {
    is_valid_sha256(expected_hex) && sha256_hex(bytes) == expected_hex
}

/// Streams `path` from disk and verifies its SHA-256 against `expected_hex`.
pub fn verify_file_sha256(path: &Path, expected_hex: &str) -> bool {
    if !is_valid_sha256(expected_hex) {
        return false;
    }
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => count,
            Err(_) => return false,
        };
        hasher.update(&buffer[..count]);
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(&mut hex, "{byte:02x}");
    }
    hex == expected_hex
}

/// Builds the `cmd /C` payload that runs the NSIS setup silently and restarts MetaMate.
pub fn installer_command_line(installer_path: &Path, current_exe: &Path) -> Option<String> {
    let setup = installer_path.to_str()?;
    let exe = current_exe.to_str()?;
    if setup.is_empty()
        || exe.is_empty()
        // cmd expands %VAR% even inside quotes.
        || setup.chars().any(|c| c == '"' || c == '%' || c.is_control())
        || exe.chars().any(|c| c == '"' || c == '%' || c.is_control())
    {
        return None;
    }
    Some(format!("\"\"{setup}\" /S && start \"\" \"{exe}\"\""))
}

fn manifest_url() -> Option<String> {
    if let Ok(url) = std::env::var("MM_UPDATE_URL") {
        let trimmed = url.trim();
        // Testing only: our own site or a local server, never another host (review).
        if trimmed.starts_with(ALLOWED_URL_PREFIX)
            || (cfg!(debug_assertions) && trimmed.starts_with("http://127.0.0.1:"))
        {
            return Some(trimmed.to_owned());
        }
    }
    if cfg!(debug_assertions) {
        None
    } else {
        Some(DEFAULT_UPDATE_URL.to_owned())
    }
}

/// Starts the periodic update check: 60 s after startup and then every 12 hours.
pub fn check(app: &AppHandle) {
    let updater = app.state::<Updater>();
    if updater.started.swap(true, Ordering::AcqRel) {
        return;
    }
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(STARTUP_DELAY).await;
        loop {
            let _ = check_once(&handle, false).await;
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    });
}

/// One check of version.json. The timer respects CHECK_INTERVAL; the person's own
/// "Check for updates" (`force`) always asks and reports a failure instead of hiding it.
async fn check_once(app: &AppHandle, force: bool) -> Result<(), &'static str> {
    let url = manifest_url().ok_or("updates are not configured in this build")?;
    let updater = app.state::<Updater>();
    let _guard = updater.checking.lock().await;
    {
        let mut inner = updater.inner.lock().unwrap_or_else(PoisonError::into_inner);
        if !force
            && inner
                .last_checked
                .is_some_and(|at| at.elapsed() < CHECK_INTERVAL)
        {
            return Ok(());
        }
        inner.last_checked = Some(Instant::now());
    }
    let manifest = fetch_manifest(&url).await?;
    if !is_newer(&manifest.version) {
        return Ok(());
    }
    let announce = {
        let mut inner = updater.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let should_announce = inner.announced_version.as_deref() != Some(manifest.version.as_str());
        if should_announce {
            inner.announced_version = Some(manifest.version.clone());
        }
        inner.available = Some(manifest);
        should_announce
    };
    crate::show_tray_update(app);
    // No spoken line: a line is a barge-in that would end a recording, an answer or a
    // lesson (Gemini review). The tray item and Settings show the update.
    let _ = announce;
    Ok(())
}

async fn fetch_manifest(url: &str) -> Result<UpdateManifest, &'static str> {
    let client = reqwest::Client::builder()
        .timeout(MANIFEST_TIMEOUT)
        // Redirects could leave meta-mate.stream: the URL checked is the URL fetched.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "client build failed")?;
    let response = tokio::time::timeout(MANIFEST_TIMEOUT, client.get(url).send())
        .await
        .map_err(|_| "manifest timeout")?
        .map_err(|_| "manifest request failed")?;
    if !response.status().is_success() {
        return Err("manifest status not success");
    }
    let raw = tokio::time::timeout(MANIFEST_TIMEOUT, capped_bytes(response, MANIFEST_MAX_BYTES))
        .await
        .map_err(|_| "manifest body timeout")??;
    parse_manifest(&raw)
}

async fn capped_bytes(response: reqwest::Response, cap: usize) -> Result<Vec<u8>, &'static str> {
    if response
        .content_length()
        .is_some_and(|len| len > cap as u64)
    {
        return Err("response too large");
    }
    let mut body = Vec::new();
    let mut chunks = response.bytes_stream();
    while let Some(chunk) = chunks.next().await {
        let chunk = chunk.map_err(|_| "stream read failed")?;
        if body.len().saturating_add(chunk.len()) > cap {
            return Err("response too large");
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Downloads the available installer to `%TEMP%`, verifies its SHA-256, spawns it and exits.
pub async fn install(app: &AppHandle) -> Result<(), String> {
    let updater = app.state::<Updater>();
    if updater
        .installing
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("update already in progress".into());
    }
    struct InstallGuard<'a>(&'a AtomicBool);
    impl Drop for InstallGuard<'_> {
        fn drop(&mut self) {
            self.0.store(false, Ordering::Release);
        }
    }
    let guard = InstallGuard(&updater.installing);

    let manifest = updater
        .inner
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .available
        .clone()
        .ok_or_else(|| "no update available".to_owned())?;

    if parse_semver(&manifest.version).is_none()
        || !is_valid_url(&manifest.url)
        || !is_valid_sha256(&manifest.sha256)
    {
        show_line(app, "robot.update_failed");
        return Err("invalid update manifest".into());
    }

    // The app's own data folder, not the shared %TEMP% (Mistral review).
    let folder = app
        .path()
        .app_local_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir());
    let _ = std::fs::create_dir_all(&folder);
    let installer_path = folder.join(format!("MetaMate-update-{}.exe", manifest.version));
    if let Err(error) = download_installer(&manifest.url, &installer_path).await {
        let _ = std::fs::remove_file(&installer_path);
        show_line(app, "robot.update_failed");
        return Err(error.to_owned());
    }

    // Hold the verified file open without write sharing until the installer has started,
    // so nothing can swap it between the hash check and the launch (review).
    let lock = lock_for_launch(&installer_path);
    let verified = lock.is_some() && {
        let path = installer_path.clone();
        let expected = manifest.sha256.clone();
        tokio::task::spawn_blocking(move || verify_file_sha256(&path, &expected))
            .await
            .unwrap_or(false)
    };
    if !verified {
        let _ = std::fs::remove_file(&installer_path);
        show_line(app, "robot.update_failed");
        return Err("installer sha256 mismatch".into());
    }

    let Ok(current_exe) = std::env::current_exe() else {
        let _ = std::fs::remove_file(&installer_path);
        show_line(app, "robot.update_failed");
        return Err("could not resolve current executable".into());
    };
    if let Err(error) = spawn_installer(&installer_path, &current_exe) {
        let _ = std::fs::remove_file(&installer_path);
        show_line(app, "robot.update_failed");
        return Err(error.to_owned());
    }

    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(EXIT_DELAY).await;
        drop(lock);
        handle.exit(0);
    });
    // Installing until the app exits: a second click must not start another download.
    std::mem::forget(guard);
    Ok(())
}

/// The downloaded installer, open for reading with no write or delete sharing.
#[cfg(windows)]
fn lock_for_launch(path: &Path) -> Option<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_SHARE_READ: u32 = 0x1;
    std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(path)
        .ok()
}

#[cfg(not(windows))]
fn lock_for_launch(path: &Path) -> Option<std::fs::File> {
    std::fs::File::open(path).ok()
}

async fn download_installer(url: &str, destination: &Path) -> Result<(), &'static str> {
    let client = reqwest::Client::builder()
        .timeout(INSTALLER_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "client build failed")?;
    let download = async {
        let response = client
            .get(url)
            .send()
            .await
            .map_err(|_| "installer download failed")?;
        if !response.status().is_success() {
            return Err("installer download status failed");
        }
        if response
            .content_length()
            .is_some_and(|len| len > INSTALLER_MAX_BYTES as u64)
        {
            return Err("installer exceeds 80 MB");
        }
        let mut file =
            std::fs::File::create(destination).map_err(|_| "could not create installer file")?;
        let mut total = 0usize;
        let mut chunks = response.bytes_stream();
        while let Some(chunk) = chunks.next().await {
            let chunk = chunk.map_err(|_| "installer stream failed")?;
            total = total.saturating_add(chunk.len());
            if total > INSTALLER_MAX_BYTES {
                return Err("installer exceeds 80 MB");
            }
            file.write_all(&chunk)
                .map_err(|_| "could not write installer file")?;
        }
        file.sync_all()
            .map_err(|_| "could not flush installer file")?;
        Ok(())
    };
    tokio::time::timeout(INSTALLER_TIMEOUT, download)
        .await
        .map_err(|_| "installer download timed out")?
}

#[cfg(windows)]
fn spawn_installer(installer_path: &Path, current_exe: &Path) -> Result<(), &'static str> {
    use std::os::windows::process::CommandExt;
    let command_line =
        installer_command_line(installer_path, current_exe).ok_or("invalid installer path")?;
    std::process::Command::new("cmd")
        .arg("/C")
        .raw_arg(command_line)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map(|_| ())
        .map_err(|_| "could not start installer")
}

#[cfg(not(windows))]
fn spawn_installer(installer_path: &Path, current_exe: &Path) -> Result<(), &'static str> {
    let _ = installer_command_line(installer_path, current_exe).ok_or("invalid installer path")?;
    Err("installer is Windows-only")
}

fn show_line(app: &AppHandle, key: &'static str) {
    #[cfg(windows)]
    crate::voice::show_line(app, key);
    #[cfg(not(windows))]
    {
        use crate::answer_stream::{AnswerMsg, AnswerStream};
        app.state::<AnswerStream>()
            .send(AnswerMsg::Line { generation: 1, key });
    }
}

#[tauri::command]
pub async fn update_status(app: AppHandle) -> Result<UpdateStatus, String> {
    Ok(app.state::<Updater>().status())
}

/// Settings' "Check for updates": asks now, and a failure reaches the person.
#[tauri::command]
pub async fn update_check(app: AppHandle) -> Result<UpdateStatus, String> {
    check_once(&app, true).await.map_err(str::to_owned)?;
    Ok(app.state::<Updater>().status())
}

#[tauri::command]
pub async fn update_install(app: AppHandle) -> Result<(), String> {
    install(&app).await
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    #[test]
    fn parses_strict_manifest_and_rejects_malformed_inputs() {
        let json = format!(
            r#"{{"version":"0.3.0","url":"https://meta-mate.stream/download/MetaMate-setup.exe","sha256":"{VALID_SHA}","notes":{{"en":"Faster startup","ru":"Быстрый запуск"}}}}"#
        );
        let manifest = parse_manifest(json.as_bytes()).unwrap();
        assert_eq!(manifest.version, "0.3.0");
        assert_eq!(
            manifest.url,
            "https://meta-mate.stream/download/MetaMate-setup.exe"
        );
        assert_eq!(manifest.sha256, VALID_SHA);
        assert_eq!(
            manifest.notes,
            Some(UpdateNotes {
                en: Some("Faster startup".into()),
                ru: Some("Быстрый запуск".into()),
            })
        );

        let without_notes = format!(
            r#"{{"version":"1.0.2","url":"https://meta-mate.stream/MetaMate-setup.exe","sha256":"{VALID_SHA}"}}"#
        );
        let manifest = parse_manifest(without_notes.as_bytes()).unwrap();
        assert_eq!(manifest.notes, None);

        // Oversized body (> 8 KB) is rejected.
        let oversized = vec![b' '; MANIFEST_MAX_BYTES + 1];
        assert!(parse_manifest(&oversized).is_err());

        // Unknown fields, non-meta-mate URLs, uppercase or short hashes, bad versions, and >200 char notes fail.
        let long_note = "a".repeat(NOTE_MAX_CHARS + 1);
        let upper_sha = VALID_SHA.to_uppercase();
        for bad in [
            format!(
                r#"{{"version":"0.3","url":"https://meta-mate.stream/a.exe","sha256":"{VALID_SHA}"}}"#
            ),
            format!(
                r#"{{"version":"0.3.0-rc1","url":"https://meta-mate.stream/a.exe","sha256":"{VALID_SHA}"}}"#
            ),
            format!(
                r#"{{"version":"01.3.0","url":"https://meta-mate.stream/a.exe","sha256":"{VALID_SHA}"}}"#
            ),
            format!(
                r#"{{"version":"0.3.0","url":"http://meta-mate.stream/a.exe","sha256":"{VALID_SHA}"}}"#
            ),
            format!(
                r#"{{"version":"0.3.0","url":"https://evil.example/a.exe","sha256":"{VALID_SHA}"}}"#
            ),
            format!(
                r#"{{"version":"0.3.0","url":"https://meta-mate.stream/","sha256":"{VALID_SHA}"}}"#
            ),
            format!(
                r#"{{"version":"0.3.0","url":"https://meta-mate.stream/a b.exe","sha256":"{VALID_SHA}"}}"#
            ),
            format!(
                r#"{{"version":"0.3.0","url":"https://meta-mate.stream/a.exe","sha256":"{upper_sha}"}}"#
            ),
            r#"{"version":"0.3.0","url":"https://meta-mate.stream/a.exe","sha256":"abcd"}"#
                .to_owned(),
            format!(
                r#"{{"version":"0.3.0","url":"https://meta-mate.stream/a.exe","sha256":"{VALID_SHA}","extra":1}}"#
            ),
            format!(
                r#"{{"version":"0.3.0","url":"https://meta-mate.stream/a.exe","sha256":"{VALID_SHA}","notes":{{"en":"{long_note}"}}}}"#
            ),
            format!(
                r#"{{"version":"0.3.0","url":"https://meta-mate.stream/a.exe","sha256":"{VALID_SHA}","notes":{{"de":"Hallo"}}}}"#
            ),
        ] {
            assert!(parse_manifest(bad.as_bytes()).is_err(), "{bad}");
        }
    }

    #[test]
    fn compares_semver_strictly() {
        assert_eq!(parse_semver("0.2.0"), Some((0, 2, 0)));
        assert_eq!(parse_semver("10.20.30"), Some((10, 20, 30)));
        for invalid in [
            "",
            "0.2",
            "0.2.0.1",
            "v0.2.0",
            "0.2.0-beta",
            "0.02.0",
            "-1.0.0",
        ] {
            assert_eq!(parse_semver(invalid), None, "{invalid}");
        }

        assert!(is_newer_version("0.2.0", "0.2.1"));
        assert!(is_newer_version("0.2.0", "0.3.0"));
        assert!(is_newer_version("0.2.0", "1.0.0"));
        assert!(!is_newer_version("0.2.0", "0.2.0"));
        assert!(!is_newer_version("0.2.0", "0.1.9"));
        assert!(!is_newer_version("0.2.0", "invalid"));
        assert!(!is_newer(CURRENT_VERSION));
        assert!(is_newer("99.0.0"));
    }

    #[test]
    fn verifies_sha256_bytes_and_files() {
        assert!(verify_sha256(b"", VALID_SHA));
        let hello_sha = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        assert!(verify_sha256(b"hello", hello_sha));
        assert!(!verify_sha256(b"hello!", hello_sha));
        assert!(!verify_sha256(b"hello", &hello_sha.to_uppercase()));
        assert!(!verify_sha256(b"hello", "short"));

        let temp_path =
            std::env::temp_dir().join(format!("metamate-sha256-test-{}.tmp", std::process::id()));
        std::fs::write(&temp_path, b"hello").unwrap();
        assert!(verify_file_sha256(&temp_path, hello_sha));
        assert!(!verify_file_sha256(&temp_path, VALID_SHA));
        let _ = std::fs::remove_file(&temp_path);
        assert!(!verify_file_sha256(&temp_path, hello_sha));
    }

    #[test]
    fn quotes_installer_command_line_and_rejects_quotes_or_control_chars() {
        let cmd = installer_command_line(
            Path::new(r"C:\Users\Jane Doe\AppData\Local\Temp\MetaMate-update-0.3.0.exe"),
            Path::new(r"C:\Users\Jane Doe\AppData\Local\Programs\MetaMate\metamate.exe"),
        )
        .unwrap();
        // cmd /C strips the outer pair of quotes and keeps the inner quoted paths.
        assert_eq!(
            cmd,
            r#"""C:\Users\Jane Doe\AppData\Local\Temp\MetaMate-update-0.3.0.exe" /S && start "" "C:\Users\Jane Doe\AppData\Local\Programs\MetaMate\metamate.exe"""#
        );
        assert!(
            installer_command_line(Path::new("C:\\bad\"path.exe"), Path::new("C:\\app.exe"))
                .is_none()
        );
    }
}

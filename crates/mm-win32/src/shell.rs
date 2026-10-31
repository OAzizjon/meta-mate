//! A web page in the person's default browser (sign-in, D54).

use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{HSTRING, PCWSTR, w};

/// Whether `url` may be handed to the shell: Display Settings, an https page, or http on
/// the loopback, with no quote or control character (never a file or a program).
pub fn openable(url: &str) -> bool {
    (url == "ms-settings:display"
        || url.starts_with("https://")
        || url.starts_with("http://127.0.0.1:"))
        && !url.chars().any(|c| c.is_control() || c == '"' || c == ' ')
}

/// Opens `url` with the shell. False when it is not `openable` or Windows refused.
pub fn open_url(url: &str) -> bool {
    if !openable(url) {
        return false;
    }
    execute(url)
}

/// Opens only a fixed app target from mm-core's allowlist, with no arguments.
pub fn open_app(app: mm_core::matescript::AppId) -> bool {
    execute(app.spec().0)
}

/// Opens a known local directory without interpreting it as a command.
pub fn open_folder(folder: &std::path::Path) -> bool {
    if !folder.is_absolute() || !folder.is_dir() {
        return false;
    }
    let Some(target) = folder.to_str() else {
        return false;
    };
    if target.chars().any(|c| c.is_control() || c == '"') {
        return false;
    }
    execute(target)
}

fn execute(target: &str) -> bool {
    let target = HSTRING::from(target);
    // SAFETY: the operation and the target are NUL-terminated wide strings that outlive the
    // call; no parameters, no directory, no owner window.
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &target,
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    // Success is any value above 32 (the historical HINSTANCE convention).
    result.0 as isize > 32
}

#[cfg(test)]
mod tests {
    use super::openable;

    #[test]
    fn only_web_pages_and_display_settings_are_opened() {
        assert!(openable("ms-settings:display"));
        assert!(openable(
            "https://example.com/auth/google/start?port=51234&state=x"
        ));
        assert!(openable("http://127.0.0.1:8787/auth/github/start?port=5"));
        for url in [
            "file:///C:/Windows/System32/calc.exe",
            "C:\\Windows\\notepad.exe",
            "http://example.com/",
            "https://example.com/\" --evil",
            "https://example.com/a b",
            "https://example.com/\nx",
            "ms-settings:privacy-microphone",
            "ms-settings:display --evil",
            "ms-settings:display\n",
        ] {
            assert!(!openable(url), "{url}");
        }
    }
}

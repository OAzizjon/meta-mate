//! Platform failures and refusals as values (convention 5): never a panic on a user path.

use std::fmt;

/// Why a platform call failed or refused. `Display` text is for logs only; users hear a
/// localized line chosen from the variant (invariant 7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlatformError {
    /// The target window is gone, or it is not the window this backend serves.
    WindowNotFound,
    /// The UI Automation provider did not answer in time.
    UiaTimeout,
    /// Password redaction cannot be guaranteed, so nothing is captured (invariant 3).
    RedactionUncertain,
    /// The capture failed, came back black or incomplete, or its buffer has the wrong size.
    CaptureFailed,
    /// Injected input without a human's yes for exactly that action (invariant 4).
    ConfirmationRequired,
    /// The action touches a payment, purchase, delete, money or password control (invariant 4).
    SafetyBlocked,
    /// No replay fixture with this name exists.
    FixtureNotFound,
    /// The replay fixture is unreadable or malformed; the text says what is wrong.
    FixtureInvalid(String),
}

impl fmt::Display for PlatformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WindowNotFound => f.write_str("target window not found"),
            Self::UiaTimeout => f.write_str("UI Automation timed out"),
            Self::RedactionUncertain => f.write_str("password redaction cannot be guaranteed"),
            Self::CaptureFailed => f.write_str("screen capture failed"),
            Self::ConfirmationRequired => f.write_str("action needs a human confirmation"),
            Self::SafetyBlocked => f.write_str("action is on the safety blocklist"),
            Self::FixtureNotFound => f.write_str("fixture not found"),
            Self::FixtureInvalid(reason) => write!(f, "invalid fixture: {reason}"),
        }
    }
}

impl std::error::Error for PlatformError {}

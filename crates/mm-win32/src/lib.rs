//! MetaMate's Windows layer: the only crate that calls Win32 (docs/project-structure.md).
//! Small and boring on purpose; every `unsafe` block says why it is sound.
//!
//! Invariant 1: our windows never take focus from the user's app. Invariant 2: the
//! process is Per-Monitor DPI v2 aware and everything here is in physical pixels.

#![cfg(windows)]

pub mod dpi;
pub mod focus;
pub mod monitors;
pub mod overlay;

pub use windows::Win32::Foundation::HWND;

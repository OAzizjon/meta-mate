//! MetaMate core: pure Rust with no OS calls, no I/O and no async runtime, so every
//! module is unit- and fuzz-testable anywhere (docs/project-structure.md, "Rust split").

pub mod api;
pub mod constants;
pub mod drag;
pub mod flight;
pub mod geom;
pub mod matescript;
pub mod mic;
pub mod placement;
pub mod presence;
pub mod privacy;
pub mod ptt;
pub mod redaction;
pub mod shot;
pub mod teacher;
pub mod vad;
pub mod wav;

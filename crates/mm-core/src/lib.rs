//! MetaMate core: pure Rust with no OS calls, no I/O and no async runtime, so every
//! module is unit- and fuzz-testable anywhere (docs/project-structure.md, "Rust split").

pub mod api;
pub mod constants;
pub mod geom;
pub mod matescript;
pub mod mic;
pub mod privacy;
pub mod ptt;
pub mod redaction;
pub mod shot;
pub mod teacher;

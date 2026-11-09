//! # lumen400-core
//!
//! Pure-Rust building blocks of the Lumen400 media player. Nothing in this crate talks to
//! the GPU or an audio device, which keeps it fast to compile, trivial to test and
//! reusable from any front-end.
//!
//! * [`playlist`] – ordered play queue with shuffle/repeat plus M3U / PLS / XSPF import-export
//! * [`subtitle`] – SRT, WebVTT and ASS/SSA parsing with automatic text-encoding detection
//! * [`dsp`] – 10-band equaliser, WSOLA pitch-preserving time-stretch, spectrum analyser,
//!   soft limiter
//! * [`settings`] – persistent settings and resume-position history
//! * [`types`] – media description types shared by the engine and the UI
//! * [`media_kind`] – file classification, folder scanning, sibling-subtitle discovery
//! * [`time`] – timecode formatting and parsing
//! * [`sync`] – lock-free `f32`/`f64` atomics

#![forbid(unsafe_code)]

pub mod dsp;
pub mod lang;
pub mod media_kind;
pub mod playlist;
pub mod settings;
pub mod subtitle;
pub mod sync;
pub mod text;
pub mod time;
pub mod types;

/// Product name used for configuration folders, window titles and logs.
pub const APP_NAME: &str = "Lumen400";
/// Version of the application (workspace version).
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

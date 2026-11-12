//! # 400_media_player engine
//!
//! The 100% standalone pure-Rust playback engine of 400_media_player: native demuxing, decoding, `bwdif`
//! motion-adaptive deinterlacing, and audio/video synchronisation with a `cpal` audio output.
//! Zero external FFmpeg, OS media DLLs, or C/C++ FFI dependencies (`#![forbid(unsafe_code)]`).
//!
//! ```no_run
//! use media_player_400_engine::{EngineConfig, Player};
//!
//! let player = Player::new(EngineConfig::default(), None);
//! player.open("film.mkv");
//! loop {
//!     for event in player.poll_events() {
//!         println!("{event:?}");
//!     }
//!     if let Some(frame) = player.poll_frame(0.016) {
//!         // upload `frame.plane(i)` to the GPU…
//!         let _ = frame;
//!     }
//!     std::thread::sleep(std::time::Duration::from_millis(8));
//! }
//! ```
//!
//! ## Threads
//!
//! * **controller** — owns the audio device and the state machine ([`PlayState`]);
//! * **demuxer** — reads packets, decodes embedded text & bitmap subtitles;
//! * **video decoder** — native pure-Rust frame decoding + `bwdif` deinterlacer + pixel-format policy;
//! * **audio decoder** — native pure-Rust decoding, Hermite resampling, and WSOLA pitch-preserving time stretching;
//! * **audio callback** — real-time; lock-free; drives the master clock.

#![forbid(unsafe_code)]

mod audio_dec;
mod audio_out;
pub mod codecs;
pub mod container;
pub mod deinterlace;
mod demux;
mod error;
pub mod frame;
mod hwaccel;
mod player;
mod probe;
mod queue;
mod session;
mod shared;
mod subs;
mod thumbs;
mod util;
mod video_dec;

pub use audio_out::{OutputDeviceInfo, list_output_devices};
pub use error::{EngineError, Result};
pub use frame::{ColorInfo, GamutKind, Matrix, PixelLayout, Plane, RgbaImage, Transfer, VideoFrame};
pub use media_player_400_core::settings::Deinterlace;
pub use player::{Player, SpectrumView};
pub use probe::{LangPrefs, StreamSelection, probe};
pub use shared::{EngineConfig, Event, Notice, PlayState, PlayerStats, SeekMode, Snapshot};
pub use thumbs::{MediaPreview, Thumbnail, extract_media_preview, hdr_to_sdr_rgba};

use std::sync::Once;

/// Initialises the native media engine (once per process). Called by [`Player::new`]; safe to call
/// before [`probe`] without a player.
pub fn init() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {});
}

/// The version string of the standalone native Rust A/V engine.
pub fn engine_version() -> String {
    format!("400_media_player Native 100% Pure-Rust A/V {} (0 FFmpeg, 0 DLL)", env!("CARGO_PKG_VERSION"))
}

/// Kept as an alias for [`engine_version`] for compatibility with external callers.
pub fn ffmpeg_version() -> String {
    engine_version()
}

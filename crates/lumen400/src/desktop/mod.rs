#![allow(dead_code)]
//! Desktop integration: how the rest of the session sees the player, and how it is told what to do.
//!
//! * **Media keys, `playerctl`, the media widgets of GNOME and KDE** speak MPRIS over D-Bus: the
//!   player publishes what it plays and obeys play, pause, seek, volume…
//! * **One window**: starting Lumen400 again while it runs hands the files over to the running
//!   window (the second process exits at once) unless `--new-instance` is given or the setting is
//!   off.
//!
//! Linux has an implementation (D-Bus through `zbus`); on the other systems every call here does
//! nothing (Windows and macOS have their own media-control APIs, which are not wired up yet).
//!
//! The interface never waits for the bus: it posts a [`State`] and collects [`Command`]s, and all the
//! talking happens on threads of its own.

use std::sync::Arc;
use std::time::Duration;

use lumen400_core::playlist::RepeatMode;
use lumen400_engine::RgbaImage;

use crate::cli::Cli;

mod glue;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(not(target_os = "linux"))]
mod other;

#[cfg(target_os = "linux")]
pub use linux::{Service, hand_over};
#[cfg(not(target_os = "linux"))]
pub use other::{Service, hand_over};

/// Called (from any thread) when a command is waiting: it wakes the interface up.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// The media types the player opens: advertised over D-Bus and in the desktop file.
pub const MIME_TYPES: &[&str] = &[
    "video/mp4",
    "video/x-matroska",
    "video/webm",
    "video/quicktime",
    "video/x-msvideo",
    "video/mpeg",
    "video/x-flv",
    "video/3gpp",
    "video/3gpp2",
    "video/x-ms-wmv",
    "video/ogg",
    "video/mp2t",
    "video/x-m4v",
    "application/ogg",
    "application/vnd.apple.mpegurl",
    "audio/mpeg",
    "audio/mp4",
    "audio/x-m4a",
    "audio/aac",
    "audio/flac",
    "audio/x-flac",
    "audio/ogg",
    "audio/opus",
    "audio/x-opus+ogg",
    "audio/x-vorbis+ogg",
    "audio/wav",
    "audio/x-wav",
    "audio/webm",
    "audio/x-matroska",
    "audio/x-ms-wma",
    "audio/x-aiff",
    "audio/ac3",
    "audio/x-mpegurl",
    "audio/mpegurl",
    "application/x-mpegurl",
    "audio/x-scpls",
    "application/xspf+xml",
];

/// The URL schemes `OpenUri` accepts.
pub const URI_SCHEMES: &[&str] = &["file", "http", "https", "ftp", "rtmp", "rtsp"];

/// The three states MPRIS knows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
    Playing,
    Paused,
    #[default]
    Stopped,
}

impl Status {
    pub fn mpris(self) -> &'static str {
        match self {
            Status::Playing => "Playing",
            Status::Paused => "Paused",
            Status::Stopped => "Stopped",
        }
    }
}

/// What is playing. It changes with the media, not with the clock, so it is shared.
#[derive(Debug, Default)]
pub struct Track {
    /// Identifies the track (its D-Bus object path); `0` when nothing is loaded.
    pub id: u64,
    pub title: String,
    pub artists: Vec<String>,
    pub album: String,
    /// Where the media comes from: a `file://` or network URL.
    pub url: Option<String>,
    /// Seconds.
    pub length: Option<f64>,
    pub cover: Option<Arc<RgbaImage>>,
}

/// Everything the system is told about the player.
#[derive(Clone, Debug)]
pub struct State {
    pub status: Status,
    /// Seconds.
    pub position: f64,
    pub rate: f64,
    /// `1.0` is 100 %; `0.0` when muted.
    pub volume: f64,
    pub repeat: RepeatMode,
    pub shuffle: bool,
    pub fullscreen: bool,
    pub can_next: bool,
    pub can_previous: bool,
    pub can_seek: bool,
    pub can_play: bool,
    pub can_pause: bool,
    pub track: Arc<Track>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            status: Status::Stopped,
            position: 0.0,
            rate: 1.0,
            volume: 1.0,
            repeat: RepeatMode::Off,
            shuffle: false,
            fullscreen: false,
            can_next: false,
            can_previous: false,
            can_seek: false,
            can_play: false,
            can_pause: false,
            track: Arc::new(Track::default()),
        }
    }
}

impl State {
    /// Same as `other` in everything the bus announces through `PropertiesChanged` (the clock is not
    /// part of it: clients read the position when they need it).
    pub fn same_properties(&self, other: &State) -> bool {
        self.status == other.status
            && self.rate == other.rate
            && self.volume == other.volume
            && self.repeat == other.repeat
            && self.shuffle == other.shuffle
            && self.fullscreen == other.fullscreen
            && self.can_next == other.can_next
            && self.can_previous == other.can_previous
            && self.can_seek == other.can_seek
            && self.can_play == other.can_play
            && self.can_pause == other.can_pause
            && Arc::ptr_eq(&self.track, &other.track)
    }

    /// Where the clock should be `elapsed` seconds after this state was published.
    pub fn position_after(&self, elapsed: Duration) -> f64 {
        extrapolate(self.position, self.rate, self.status, elapsed.as_secs_f64(), self.track.length)
    }

    /// `true` when `now` (published `elapsed` after `self`) moved the clock by more than playing
    /// alone explains: somebody sought. A new track starts from its own beginning, that is no seek.
    pub fn jumped_to(&self, now: &State, elapsed: Duration) -> bool {
        if self.track.id != now.track.id || now.status == Status::Stopped {
            return false;
        }
        let tolerance = if self.status == Status::Playing { 0.6 } else { 0.05 };
        (now.position - self.position_after(elapsed)).abs() > tolerance
    }
}

/// Where a clock that read `position` `elapsed` seconds ago stands now.
pub fn extrapolate(position: f64, rate: f64, status: Status, elapsed: f64, length: Option<f64>) -> f64 {
    let mut p = if status == Status::Playing { position + elapsed.max(0.0) * rate } else { position };
    if let Some(l) = length {
        p = p.min(l);
    }
    if p.is_finite() { p.max(0.0) } else { 0.0 }
}

/// What the system (or another launch of the player) asks for.
#[derive(Clone, Debug)]
pub enum Command {
    Play,
    Pause,
    PlayPause,
    Stop,
    Next,
    Previous,
    /// Relative jump, in seconds (negative goes back).
    SeekBy(f64),
    /// Absolute position, in seconds, within the track whose id is given.
    SeekTo {
        track: u64,
        secs: f64,
    },
    SetVolume(f64),
    SetRate(f64),
    SetRepeat(RepeatMode),
    SetShuffle(bool),
    SetFullscreen(bool),
    OpenUri(String),
    /// Bring the window to the front.
    Raise,
    Quit,
    /// Another launch of the player: what it was asked to do.
    Open(Box<Cli>),
}

/// What became of an attempt to give a launch to the window that already runs.
#[derive(Debug, PartialEq, Eq)]
pub enum HandOver {
    /// The running window has the request: this process has nothing left to do.
    Delivered,
    /// Nobody answers: this process becomes the window.
    NoRunningWindow,
    /// Somebody owns the name but did not take the request (an older version, a hung process…).
    Failed(String),
}

/// Makes the inputs of a launch absolute, so they mean the same thing in another process (which has
/// another working directory). URLs and `file://` URLs are left as they are.
pub fn absolutize(inputs: &[String]) -> Vec<String> {
    use lumen400_core::media_kind::is_url;
    inputs
        .iter()
        .map(|input| {
            if is_url(input) {
                return input.clone();
            }
            std::path::absolute(input).map_or_else(|_| input.clone(), |p| p.to_string_lossy().into_owned())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playing_at(position: f64, id: u64) -> State {
        State { status: Status::Playing, position, track: Arc::new(Track { id, length: Some(100.0), ..Default::default() }), ..Default::default() }
    }

    #[test]
    fn the_clock_runs_only_while_playing_and_never_past_the_end() {
        assert_eq!(extrapolate(10.0, 1.0, Status::Playing, 2.0, Some(100.0)), 12.0);
        assert_eq!(extrapolate(10.0, 2.0, Status::Playing, 2.0, None), 14.0);
        assert_eq!(extrapolate(10.0, 1.0, Status::Paused, 5.0, None), 10.0);
        assert_eq!(extrapolate(99.0, 1.0, Status::Playing, 5.0, Some(100.0)), 100.0);
        assert_eq!(extrapolate(f64::NAN, 1.0, Status::Playing, 1.0, None), 0.0);
        assert_eq!(extrapolate(-3.0, 1.0, Status::Paused, 0.0, None), 0.0);
    }

    #[test]
    fn a_seek_is_a_jump_the_clock_does_not_explain() {
        let before = playing_at(10.0, 1);
        let natural = State { position: 11.0, ..before.clone() };
        assert!(!before.jumped_to(&natural, Duration::from_secs(1)));
        let sought = State { position: 60.0, ..before.clone() };
        assert!(before.jumped_to(&sought, Duration::from_secs(1)));
        let back = State { position: 2.0, ..before.clone() };
        assert!(before.jumped_to(&back, Duration::from_secs(1)));
        // Paused: any movement is a seek, a few milliseconds of jitter are not.
        let paused = State { status: Status::Paused, ..before.clone() };
        assert!(paused.jumped_to(&State { position: 10.5, ..paused.clone() }, Duration::from_secs(3)));
        assert!(!paused.jumped_to(&State { position: 10.01, ..paused.clone() }, Duration::from_secs(3)));
        // Another track, or nothing at all, never counts as a seek.
        let other = playing_at(0.0, 2);
        assert!(!before.jumped_to(&other, Duration::from_secs(1)));
        assert!(!before.jumped_to(&State { status: Status::Stopped, position: 0.0, ..before.clone() }, Duration::from_secs(1)));
    }

    #[test]
    fn properties_are_compared_without_the_clock() {
        let a = playing_at(10.0, 1);
        let later = State { position: 12.0, ..a.clone() };
        assert!(a.same_properties(&later));
        assert!(!a.same_properties(&State { volume: 0.3, ..a.clone() }));
        assert!(!a.same_properties(&State { status: Status::Paused, ..a.clone() }));
        assert!(!a.same_properties(&State { track: Arc::new(Track::default()), ..a.clone() }));
        assert!(!a.same_properties(&State { repeat: RepeatMode::One, ..a.clone() }));
    }

    #[test]
    fn inputs_become_absolute_but_urls_do_not() {
        let abs_path = if cfg!(windows) { "C:\\already\\abs.mp4" } else { "/already/abs.mp4" };
        let out = absolutize(&["movie.mkv".into(), abs_path.into(), "https://example.org/a.m3u8".into(), "file:///x/y.mkv".into()]);
        assert!(std::path::Path::new(&out[0]).is_absolute() && out[0].ends_with("movie.mkv"), "{out:?}");
        assert_eq!(out[1], abs_path);
        assert_eq!(out[2], "https://example.org/a.m3u8");
        assert_eq!(out[3], "file:///x/y.mkv");
    }

    #[test]
    fn the_desktop_file_lists_the_same_media_types() {
        // The packaging file and the list above must not drift apart.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/lumen400.desktop");
        let Ok(text) = std::fs::read_to_string(&path) else { return };
        let line = text.lines().find(|l| l.starts_with("MimeType=")).expect("MimeType line");
        let listed: Vec<&str> = line["MimeType=".len()..].split(';').filter(|s| !s.is_empty()).collect();
        for mime in MIME_TYPES {
            assert!(listed.contains(mime), "{mime} is advertised on D-Bus but missing from lumen400.desktop");
        }
        for mime in &listed {
            assert!(MIME_TYPES.contains(mime), "{mime} is in lumen400.desktop but not advertised on D-Bus");
        }
    }
}

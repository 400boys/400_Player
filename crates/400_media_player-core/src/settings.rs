//! Persistent settings and the resume-position history.
//!
//! Both files are plain JSON, written atomically (temp file + sync + rename) and tolerant to schema
//! changes (`#[serde(default)]` everywhere, corrupt files are moved aside instead of crashing).

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::dsp::eq::EqSettings;
use crate::playlist::RepeatMode;

/// Folder name used under the platform config / data directories.
const DIR_NAME: &str = "400_media_player";

static TMP_SEQ: AtomicU64 = AtomicU64::new(1);

/// `~/.config/400_media_player`, `%APPDATA%\400_media_player`, `~/Library/Application Support/400_media_player`.
pub fn config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join(DIR_NAME))
}

/// Directory for non-essential state (history).
pub fn data_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join(DIR_NAME))
}

/// Writes `bytes` to `path` atomically via a temporary sibling file (`write` + `sync_all` + `rename`),
/// with a safe fallback for Windows read-only/locked targets.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("settings.json");
    let tmp_path = path.with_file_name(format!(".{file_name}.{pid}.{seq}.tmp"));

    let write_tmp = (|| -> io::Result<()> {
        let mut f = File::create(&tmp_path)?;
        f.write_all(bytes)?;
        let _ = f.sync_all();
        Ok(())
    })();

    if write_tmp.is_ok() {
        if fs::rename(&tmp_path, path).is_ok() {
            return Ok(());
        }
        // On Windows, replacing a read-only file via rename can fail; clear read-only and retry.
        if let Ok(metadata) = fs::metadata(path) {
            let mut permissions = metadata.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            permissions.set_readonly(false);
            let _ = fs::set_permissions(path, permissions);
        }
        if fs::rename(&tmp_path, path).is_ok() {
            return Ok(());
        }
        let _ = fs::remove_file(&tmp_path);
    } else {
        let _ = fs::remove_file(&tmp_path);
    }

    // Fallback: direct write if the filesystem does not support atomic rename on this path.
    fs::write(path, bytes)
}

fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

// ---------------------------------------------------------------------------------------------
// Enumerations
// ---------------------------------------------------------------------------------------------

/// User-interface language.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    /// Follow the system locale (French when it starts with `fr`, English otherwise).
    #[default]
    Auto,
    French,
    English,
}

impl Language {
    pub const ALL: [Language; 3] = [Language::Auto, Language::French, Language::English];

    /// Resolves `Auto` against a locale string such as `fr_FR.UTF-8`.
    pub fn resolve(self, locale: &str) -> Language {
        match self {
            Language::Auto => {
                if locale.to_ascii_lowercase().starts_with("fr") {
                    Language::French
                } else {
                    Language::English
                }
            }
            other => other,
        }
    }
}

/// Base colour scheme.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeMode {
    /// Deep blue-black, the signature look.
    #[default]
    Dark,
    /// Pure black for OLED panels.
    Oled,
    Light,
    /// Follow the operating system.
    System,
}

impl ThemeMode {
    pub const ALL: [ThemeMode; 4] = [ThemeMode::Dark, ThemeMode::Oled, ThemeMode::Light, ThemeMode::System];
}

/// Accent colours — every one is the "400" shade of its Tailwind-style ramp, which is where
/// the product name comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Accent {
    #[default]
    Violet,
    Sky,
    Emerald,
    Rose,
    Amber,
    Orange,
    Fuchsia,
    Cyan,
}

impl Accent {
    pub const ALL: [Accent; 8] = [Accent::Violet, Accent::Sky, Accent::Emerald, Accent::Rose, Accent::Amber, Accent::Orange, Accent::Fuchsia, Accent::Cyan];

    /// The 400 shade (used on dark backgrounds).
    pub fn rgb(self) -> [u8; 3] {
        match self {
            Accent::Violet => [167, 139, 250],
            Accent::Sky => [56, 189, 248],
            Accent::Emerald => [52, 211, 153],
            Accent::Rose => [251, 113, 133],
            Accent::Amber => [251, 191, 36],
            Accent::Orange => [251, 146, 60],
            Accent::Fuchsia => [232, 121, 249],
            Accent::Cyan => [34, 211, 238],
        }
    }

    /// The shade used on light backgrounds: Tailwind 600, or 700 where 600 would not give white
    /// text the 4.5:1 contrast that WCAG AA asks for (checked by a test in the app crate).
    pub fn rgb_deep(self) -> [u8; 3] {
        match self {
            Accent::Violet => [124, 58, 237],
            Accent::Sky => [3, 105, 161],
            Accent::Emerald => [4, 120, 87],
            Accent::Rose => [225, 29, 72],
            Accent::Amber => [180, 83, 9],
            Accent::Orange => [194, 65, 12],
            Accent::Fuchsia => [192, 38, 211],
            Accent::Cyan => [14, 116, 144],
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Accent::Violet => "Violet",
            Accent::Sky => "Sky",
            Accent::Emerald => "Emerald",
            Accent::Rose => "Rose",
            Accent::Amber => "Amber",
            Accent::Orange => "Orange",
            Accent::Fuchsia => "Fuchsia",
            Accent::Cyan => "Cyan",
        }
    }

    pub fn next(self) -> Accent {
        let i = Self::ALL.iter().position(|a| *a == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
}

/// Hardware decoding policy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum HwDecode {
    /// Always decode in software.
    #[default]
    Off,
    /// Try the platform decoder (VAAPI / VideoToolbox / D3D11VA / CUDA) and fall back silently.
    Auto,
}

/// What to do about interlaced video (old TV recordings, DVDs).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Deinterlace {
    /// Only pictures the stream flags as interlaced.
    #[default]
    Auto,
    /// Never.
    Off,
    /// Every picture (for streams that forget to flag their fields).
    On,
}

impl Deinterlace {
    /// Next mode in the shortcut cycle: `Auto → Off → On → Auto`.
    pub fn cycle(self) -> Self {
        match self {
            Self::Auto => Self::Off,
            Self::Off => Self::On,
            Self::On => Self::Auto,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------------------------

/// How subtitles are drawn.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SubtitleStyle {
    /// Size multiplier (`1.0` ≈ 4.6 % of the video height).
    pub scale: f32,
    pub color: [u8; 3],
    pub outline: bool,
    pub background: bool,
    pub bg_opacity: f32,
    pub bold: bool,
    /// Distance from the bottom edge as a fraction of the video height.
    pub bottom_margin: f32,
}

impl Default for SubtitleStyle {
    fn default() -> Self {
        Self { scale: 1.0, color: [255, 255, 255], outline: true, background: false, bg_opacity: 0.55, bold: false, bottom_margin: 0.07 }
    }
}

/// Remembered window geometry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowState {
    pub width: f32,
    pub height: f32,
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub maximized: bool,
}

impl Default for WindowState {
    fn default() -> Self {
        Self { width: 1180.0, height: 700.0, x: None, y: None, maximized: false }
    }
}

/// Everything the user can configure.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    pub language: Language,
    pub theme: ThemeMode,
    pub accent: Accent,
    /// UI zoom factor on top of the OS scale (`0.8 ..= 1.6`).
    pub ui_scale: f32,
    /// Draw our own title bar instead of the system one (experimental on some desktops).
    pub custom_titlebar: bool,
    /// Soft glow of the picture in the letter-box bars.
    pub ambient_mode: bool,

    /// `0.0 ..= 2.0`, `1.0` = 100 %.
    pub volume: f32,
    pub muted: bool,
    pub repeat: RepeatMode,
    pub shuffle: bool,
    pub resume_playback: bool,
    /// Opening a single local file queues the other media of its folder (natural order), so
    /// Next / Previous walk through the season.
    pub queue_folder_siblings: bool,
    pub auto_load_subtitles: bool,
    /// Comma-separated ISO 639 codes, best first (`"fra,eng"`).
    pub preferred_audio_langs: String,
    pub preferred_sub_langs: String,
    pub subtitle_style: SubtitleStyle,
    pub eq: EqSettings,
    pub audio_device: Option<String>,
    pub hw_decode: HwDecode,
    pub deinterlace: Deinterlace,
    /// Evens out loud and quiet passages (dynamic range compression) for late-night viewing.
    pub night_mode: bool,
    /// Keep the screen on (and the computer awake) while something plays.
    pub prevent_sleep: bool,
    /// A second launch hands its files to the window that is already open (Linux, over D-Bus).
    pub single_instance: bool,
    /// Seek-bar preview pictures (generated in the background for local video files).
    pub generate_thumbnails: bool,

    /// Short / medium / long seek steps in seconds (arrow keys with no / Shift / Ctrl).
    pub seek_steps: [f64; 3],
    pub volume_step: f32,
    /// Seconds of inactivity before the controls fade out.
    pub controls_timeout: f32,
    pub show_remaining_time: bool,

    pub window: WindowState,
    pub last_dir: Option<String>,
    pub screenshot_dir: Option<String>,
    /// Advanced studio / library dashboard view mode (persisted across sessions).
    pub advanced_mode: bool,
    /// User-bookmarked favorite media sources in the Advanced Studio library.
    pub favorites: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            language: Language::Auto,
            theme: ThemeMode::Dark,
            accent: Accent::Violet,
            ui_scale: 1.0,
            custom_titlebar: false,
            ambient_mode: true,
            advanced_mode: false,
            favorites: Vec::new(),
            volume: 1.0,
            muted: false,
            repeat: RepeatMode::Off,
            shuffle: false,
            resume_playback: true,
            queue_folder_siblings: true,
            auto_load_subtitles: true,
            preferred_audio_langs: String::new(),
            preferred_sub_langs: String::new(),
            subtitle_style: SubtitleStyle::default(),
            eq: EqSettings::default(),
            audio_device: None,
            hw_decode: HwDecode::Off,
            deinterlace: Deinterlace::Auto,
            night_mode: false,
            prevent_sleep: true,
            single_instance: true,
            generate_thumbnails: true,
            seek_steps: [5.0, 10.0, 60.0],
            volume_step: 0.05,
            controls_timeout: 2.5,
            show_remaining_time: false,
            window: WindowState::default(),
            last_dir: None,
            screenshot_dir: None,
        }
    }
}

impl Settings {
    /// Location of `settings.json`.
    pub fn path() -> Option<PathBuf> {
        config_dir().map(|d| d.join("settings.json"))
    }

    /// Loads the user settings (defaults when missing or unreadable).
    pub fn load() -> Self {
        Self::path().map(|p| Self::load_from(&p)).unwrap_or_default()
    }

    /// Loads from `path`. A corrupt file is renamed to `*.bak` and defaults are returned.
    pub fn load_from(path: &Path) -> Self {
        let Ok(bytes) = fs::read(path) else { return Self::default() };
        match serde_json::from_slice::<Settings>(&bytes) {
            Ok(mut s) => {
                s.sanitize();
                s
            }
            Err(_) => {
                let _ = fs::rename(path, path.with_extension("json.bak"));
                Self::default()
            }
        }
    }

    pub fn save(&self) -> io::Result<()> {
        match Self::path() {
            Some(p) => self.save_to(&p),
            None => Ok(()),
        }
    }

    pub fn save_to(&self, path: &Path) -> io::Result<()> {
        let json = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        write_atomic(path, &json)
    }

    /// Where screenshots go: the user's choice, else `Pictures/400_media_player`, else the home folder.
    pub fn screenshot_directory(&self) -> PathBuf {
        if let Some(d) = self.screenshot_dir.as_deref().filter(|d| !d.trim().is_empty()) {
            return PathBuf::from(d);
        }
        dirs::picture_dir().map(|d| d.join(DIR_NAME)).or_else(dirs::home_dir).unwrap_or_else(|| PathBuf::from("."))
    }

    /// Clamps every value to its legal range (a hand-edited file must never crash the app).
    pub fn sanitize(&mut self) {
        fn fix(v: &mut f32, lo: f32, hi: f32, default: f32) {
            *v = if v.is_finite() { v.clamp(lo, hi) } else { default };
        }
        fix(&mut self.volume, 0.0, 2.0, 1.0);
        fix(&mut self.ui_scale, 0.8, 1.6, 1.0);
        fix(&mut self.volume_step, 0.01, 0.25, 0.05);
        fix(&mut self.controls_timeout, 0.8, 10.0, 2.5);
        fix(&mut self.subtitle_style.scale, 0.5, 3.0, 1.0);
        fix(&mut self.subtitle_style.bg_opacity, 0.0, 1.0, 0.55);
        fix(&mut self.subtitle_style.bottom_margin, 0.0, 0.4, 0.07);
        fix(&mut self.eq.preamp_db, -24.0, 24.0, 0.0);
        for g in &mut self.eq.gains_db {
            fix(g, -12.0, 12.0, 0.0);
        }
        for (i, d) in [5.0, 10.0, 60.0].into_iter().enumerate() {
            let v = self.seek_steps[i];
            self.seek_steps[i] = if v.is_finite() { v.clamp(0.5, 3600.0) } else { d };
        }
        fix(&mut self.window.width, 480.0, 16384.0, 1180.0);
        fix(&mut self.window.height, 320.0, 16384.0, 700.0);
        self.favorites.retain(|f| !f.trim().is_empty());
        self.favorites.dedup();
    }
}

// ---------------------------------------------------------------------------------------------
// History
// ---------------------------------------------------------------------------------------------

/// A previously played item.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub source: String,
    pub title: Option<String>,
    /// Last position in seconds.
    pub position: f64,
    /// Duration in seconds (`0` when unknown, e.g. live streams).
    pub duration: f64,
    /// Unix timestamp of the last play.
    pub last_played: u64,
}

impl HistoryEntry {
    /// Fraction watched, `0.0 ..= 1.0`.
    pub fn progress(&self) -> f32 {
        if self.duration > 0.0 { (self.position / self.duration).clamp(0.0, 1.0) as f32 } else { 0.0 }
    }
}

/// Recently played media with their resume positions (most recent first).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct History {
    entries: Vec<HistoryEntry>,
}

const HISTORY_CAP: usize = 200;
/// Positions closer than this to the start are not worth resuming.
const RESUME_MIN_POSITION: f64 = 10.0;
/// …nor those this close to the end.
const RESUME_END_MARGIN: f64 = 15.0;

impl History {
    pub fn path() -> Option<PathBuf> {
        data_dir().map(|d| d.join("history.json"))
    }

    pub fn load() -> Self {
        Self::path().map(|p| Self::load_from(&p)).unwrap_or_default()
    }

    pub fn load_from(path: &Path) -> Self {
        fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn save(&self) -> io::Result<()> {
        match Self::path() {
            Some(p) => self.save_to(&p),
            None => Ok(()),
        }
    }

    pub fn save_to(&self, path: &Path) -> io::Result<()> {
        write_atomic(path, &serde_json::to_vec(self).map_err(io::Error::other)?)
    }

    pub fn entries(&self) -> &[HistoryEntry] {
        &self.entries
    }

    /// Up to `n` most recent entries.
    pub fn recent(&self, n: usize) -> &[HistoryEntry] {
        &self.entries[..n.min(self.entries.len())]
    }

    /// Records the state of `source` (moves it to the front).
    pub fn record(&mut self, source: &str, title: Option<&str>, position: f64, duration: f64) {
        self.record_at(source, title, position, duration, now_unix());
    }

    pub fn record_at(&mut self, source: &str, title: Option<&str>, position: f64, duration: f64, now: u64) {
        let old_title = self.entries.iter().find(|e| e.source == source).and_then(|e| e.title.clone());
        self.entries.retain(|e| e.source != source);
        self.entries.insert(
            0,
            HistoryEntry {
                source: source.to_owned(),
                title: title.map(str::to_owned).or(old_title),
                position: if position.is_finite() { position.max(0.0) } else { 0.0 },
                duration: if duration.is_finite() { duration.max(0.0) } else { 0.0 },
                last_played: now,
            },
        );
        self.entries.truncate(HISTORY_CAP);
    }

    /// Position to resume at, if it is worth resuming.
    pub fn resume_position(&self, source: &str) -> Option<f64> {
        let e = self.entries.iter().find(|e| e.source == source)?;
        let near_end = e.duration > 0.0 && e.position > e.duration - RESUME_END_MARGIN;
        (e.position >= RESUME_MIN_POSITION && !near_end && e.duration >= 60.0).then_some(e.position)
    }

    pub fn remove(&mut self, source: &str) {
        self.entries.retain(|e| e.source != source);
    }

    pub fn clear(&mut self, ) {
        self.entries.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-settings")
            .join(format!("400_media_player-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn settings_roundtrip_and_defaults() {
        let dir = tmp("settings");
        let p = dir.join("settings.json");
        assert_eq!(Settings::load_from(&p), Settings::default(), "missing file → defaults");
        let s = Settings {
            accent: Accent::Rose,
            theme: ThemeMode::Oled,
            volume: 1.4,
            advanced_mode: true,
            favorites: vec!["/movies/demo.mkv".into()],
            eq: EqSettings::from_preset("Rock").unwrap(),
            ..Settings::default()
        };
        s.save_to(&p).unwrap();
        assert_eq!(Settings::load_from(&p), s);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn partial_and_future_files_are_accepted() {
        let dir = tmp("partial");
        let p = dir.join("settings.json");
        fs::write(&p, r#"{"accent":"Cyan","volume":0.5,"some_future_field":123}"#).unwrap();
        let s = Settings::load_from(&p);
        assert_eq!(s.accent, Accent::Cyan);
        assert_eq!(s.volume, 0.5);
        assert!(s.resume_playback, "unspecified fields keep their default");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn corrupt_file_is_moved_aside() {
        let dir = tmp("corrupt");
        let p = dir.join("settings.json");
        fs::write(&p, "{ not json").unwrap();
        assert_eq!(Settings::load_from(&p), Settings::default());
        assert!(dir.join("settings.json.bak").exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn sanitize_clamps_hostile_values() {
        let mut s = Settings { volume: 99.0, ui_scale: f32::NAN, seek_steps: [f64::NAN, -4.0, 1e9], ..Settings::default() };
        s.eq.gains_db[0] = 400.0;
        s.window.width = 1.0;
        s.sanitize();
        assert_eq!(s.volume, 2.0);
        assert_eq!(s.ui_scale, 1.0);
        assert_eq!(s.seek_steps, [5.0, 0.5, 3600.0]);
        assert_eq!(s.eq.gains_db[0], 12.0);
        assert_eq!(s.window.width, 480.0);
    }

    #[test]
    fn accents_are_distinct_400_shades() {
        let mut seen = std::collections::HashSet::new();
        for a in Accent::ALL {
            assert!(seen.insert(a.rgb()));
            assert_ne!(a.rgb(), a.rgb_deep());
        }
        assert_eq!(Accent::Violet.rgb(), [167, 139, 250]);
        assert_eq!(Accent::Cyan.next(), Accent::Violet);
        assert_eq!(Deinterlace::Auto.cycle(), Deinterlace::Off);
        assert_eq!(Deinterlace::Off.cycle(), Deinterlace::On);
        assert_eq!(Deinterlace::On.cycle(), Deinterlace::Auto);
    }

    #[test]
    fn language_resolution() {
        assert_eq!(Language::Auto.resolve("fr_FR.UTF-8"), Language::French);
        assert_eq!(Language::Auto.resolve("en-US"), Language::English);
        assert_eq!(Language::Auto.resolve(""), Language::English);
        assert_eq!(Language::French.resolve("en-US"), Language::French);
    }

    #[test]
    fn history_behaviour() {
        let mut h = History::default();
        h.record_at("/a.mkv", Some("A"), 120.0, 3600.0, 1);
        h.record_at("/b.mkv", None, 5.0, 3600.0, 2);
        h.record_at("/a.mkv", None, 300.0, 3600.0, 3);
        assert_eq!(h.entries().len(), 2);
        assert_eq!(h.entries()[0].source, "/a.mkv", "most recent first");
        assert_eq!(h.entries()[0].title.as_deref(), Some("A"), "title survives updates");
        assert_eq!(h.resume_position("/a.mkv"), Some(300.0));
        assert_eq!(h.resume_position("/b.mkv"), None, "too close to the start");
        h.record_at("/a.mkv", None, 3595.0, 3600.0, 4);
        assert_eq!(h.resume_position("/a.mkv"), None, "finished");
        h.record_at("/short.mp4", None, 20.0, 30.0, 5);
        assert_eq!(h.resume_position("/short.mp4"), None, "short clips never resume");
        assert!((h.entries()[0].progress() - 0.666).abs() < 0.01);
        h.remove("/a.mkv");
        assert_eq!(h.entries().len(), 2);
        h.clear();
        assert!(h.entries().is_empty());
    }

    #[test]
    fn history_is_capped_and_persists() {
        let dir = tmp("history");
        let p = dir.join("h.json");
        let mut h = History::default();
        for i in 0..(HISTORY_CAP + 50) {
            h.record_at(&format!("/m/{i}.mp4"), None, 100.0, 1000.0, i as u64);
        }
        assert_eq!(h.entries().len(), HISTORY_CAP);
        h.save_to(&p).unwrap();
        assert_eq!(History::load_from(&p), h);
        let _ = fs::remove_dir_all(dir);
    }
}

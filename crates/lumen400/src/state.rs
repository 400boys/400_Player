//! Small state types of the application: what is shown, what is selected, what is pending.

use std::path::PathBuf;

use egui::Pos2;
use lumen400_core::subtitle::SubtitleTrack;

/// Where the subtitles on screen come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubSource {
    Off,
    /// An embedded text stream (container stream index).
    Embedded(usize),
    /// A subtitle file loaded by the user or found next to the video.
    External,
}

/// A subtitle file parsed in memory.
pub struct ExternalSub {
    pub name: String,
    pub track: SubtitleTrack,
}

/// The A–B repeat section.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AbLoop {
    Off,
    /// Start marked, waiting for the end.
    A(f64),
    Active(f64, f64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Warning,
    Error,
}

/// What clicking a toast does, besides dismissing it.
#[derive(Clone, Debug)]
pub enum ToastAction {
    /// Show a file in the system file manager.
    Reveal(PathBuf),
}

pub struct Toast {
    pub id: u64,
    pub kind: ToastKind,
    pub title: String,
    pub detail: Option<String>,
    /// Context time (seconds) at which the toast appeared.
    pub born: f64,
    pub ttl: f64,
    pub action: Option<ToastAction>,
}

/// The transient message in the middle-top of the picture (volume, seek, speed…).
#[derive(Clone, Debug)]
pub struct Osd {
    pub icon: char,
    pub text: String,
    /// A bar below the text (`0.0 ..= 1.0`), e.g. the volume.
    pub progress: Option<f32>,
    /// The volume bar goes beyond 100 %: where the 100 % tick sits.
    pub tick: Option<f32>,
    pub born: f64,
}

/// A page of the drill-down menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Root,
    Open,
    Subtitles,
    Audio,
    Video,
    Aspect,
    Speed,
    Playback,
    Picture,
    Tools,
}

/// The open popup menu (context menu or the "more" button).
pub struct MenuState {
    /// Where the menu opened.
    pub pos: Pos2,
    /// Anchor the menu above (`true`, for the dock) or below (`false`, for the pointer) `pos`.
    pub above: bool,
    /// Pages from the root to the current one.
    pub stack: Vec<Page>,
    /// Context time of the opening, to ignore the click that opened it.
    pub opened_at: f64,
}

impl MenuState {
    pub fn page(&self) -> Page {
        *self.stack.last().unwrap_or(&Page::Root)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsTab {
    Appearance,
    Playback,
    Audio,
    Subtitles,
    Advanced,
    About,
}

impl SettingsTab {
    pub const ALL: [SettingsTab; 6] = [SettingsTab::Appearance, SettingsTab::Playback, SettingsTab::Audio, SettingsTab::Subtitles, SettingsTab::Advanced, SettingsTab::About];
}

/// A small modal question.
pub enum Dialog {
    OpenUrl { text: String, append: bool, first_frame: bool },
    JumpToTime { text: String, error: bool, first_frame: bool },
}

pub struct PaletteState {
    pub query: String,
    pub selected: usize,
    pub first_frame: bool,
}

/// Which auxiliary windows are open.
#[derive(Default)]
pub struct Windows {
    pub settings: Option<SettingsTab>,
    pub equalizer: bool,
    pub info: bool,
    pub stats: bool,
    pub shortcuts: bool,
    pub playlist: bool,
    pub palette: Option<PaletteState>,
    pub dialog: Option<Dialog>,
}

impl Windows {
    /// `true` while something modal-ish holds the keyboard or the pointer.
    pub fn any_modal(&self) -> bool {
        self.palette.is_some() || self.dialog.is_some()
    }

    pub fn any_window(&self) -> bool {
        self.settings.is_some() || self.equalizer || self.info || self.shortcuts || self.any_modal()
    }
}

/// An interaction with the seek bar in progress.
#[derive(Clone, Copy, Debug)]
pub struct SeekDrag {
    pub target: f64,
    /// Context time of the last fast seek sent to the engine.
    pub last_sent: f64,
    pub last_target_sent: f64,
}

//! Every command of the player in one table: its label, its group and its keyboard shortcuts.
//!
//! The same table drives the shortcut handling, the command palette, the cheat sheet and the
//! context menus, so what the interface displays is always what the keyboard does.

use egui::{Event, Key};
use lumen400_core::settings::Settings;

use crate::i18n::{Key as T, t, tf};
use crate::util::format_delay;

/// Something the user can ask the player to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    PlayPause,
    Stop,
    Next,
    Previous,
    SeekBackShort,
    SeekForwardShort,
    SeekBackMedium,
    SeekForwardMedium,
    SeekBackLong,
    SeekForwardLong,
    FrameNext,
    FramePrevious,
    SpeedUp,
    SpeedDown,
    SpeedReset,
    NextChapter,
    PreviousChapter,
    ABLoop,
    JumpToTime,
    ToggleRepeat,
    ToggleShuffle,

    VolumeUp,
    VolumeDown,
    ToggleMute,
    CycleAudio,
    AudioDelayEarlier,
    AudioDelayLater,
    OpenEqualizer,
    ToggleNightMode,

    CycleSubtitles,
    LoadSubtitles,
    SubtitleDelayEarlier,
    SubtitleDelayLater,

    ToggleFullscreen,
    CycleAspect,
    CycleFit,
    RotateRight,
    FlipHorizontal,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    ResetPicture,
    Screenshot,
    ToggleAmbient,
    CycleDeinterlace,

    OpenFile,
    OpenFolder,
    OpenUrl,
    AddFiles,

    TogglePlaylist,
    OpenSettings,
    ToggleInfo,
    ToggleStats,
    ShowShortcuts,
    OpenPalette,
    ToggleAlwaysOnTop,
    ToggleRemaining,
    Quit,
}

/// Section of the cheat sheet an action is listed under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Playback,
    Navigation,
    Audio,
    Subtitles,
    Picture,
    Files,
    Window,
}

impl Group {
    pub const ALL: [Group; 7] = [Group::Playback, Group::Navigation, Group::Audio, Group::Subtitles, Group::Picture, Group::Files, Group::Window];

    pub fn label(self) -> &'static str {
        t(match self {
            Group::Playback => T::GroupPlayback,
            Group::Navigation => T::GroupNavigation,
            Group::Audio => T::GroupAudio,
            Group::Subtitles => T::GroupSubtitles,
            Group::Picture => T::GroupPicture,
            Group::Files => T::GroupFiles,
            Group::Window => T::GroupWindow,
        })
    }
}

/// Which of Ctrl (⌘ on macOS), Shift and Alt must be held — exactly these, no more.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Mods {
    pub const NONE: Mods = Mods { ctrl: false, shift: false, alt: false };
    pub const CTRL: Mods = Mods { ctrl: true, shift: false, alt: false };
    pub const SHIFT: Mods = Mods { ctrl: false, shift: true, alt: false };
    pub const ALT: Mods = Mods { ctrl: false, shift: false, alt: true };
    pub const CTRL_SHIFT: Mods = Mods { ctrl: true, shift: true, alt: false };
}

/// A physical key with modifiers, or a typed character (for punctuation, whose key and
/// modifiers differ between keyboard layouts).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Binding {
    Key(Key, Mods),
    Char(char),
}

impl Binding {
    /// Does this keyboard event trigger the binding?
    pub fn matches(&self, event: &Event, allow_repeat: bool) -> bool {
        match (self, event) {
            (Binding::Key(key, mods), Event::Key { key: k, pressed: true, repeat, modifiers, .. }) => {
                *k == *key && (allow_repeat || !*repeat) && modifiers.command == mods.ctrl && modifiers.shift == mods.shift && modifiers.alt == mods.alt
            }
            (Binding::Char(c), Event::Text(s)) => {
                let mut it = s.chars();
                it.next() == Some(*c) && it.next().is_none()
            }
            _ => false,
        }
    }

    /// Text shown in menus and in the cheat sheet (`Ctrl+Shift+S`, `←`, `[`).
    pub fn label(&self) -> String {
        match self {
            Binding::Char(c) => c.to_string(),
            Binding::Key(key, mods) => {
                let mut s = String::new();
                if mods.ctrl {
                    s.push_str(if cfg!(target_os = "macos") { "⌘" } else { "Ctrl+" });
                }
                if mods.alt {
                    s.push_str(if cfg!(target_os = "macos") { "⌥" } else { "Alt+" });
                }
                if mods.shift {
                    s.push_str(if cfg!(target_os = "macos") { "⇧" } else { "Shift+" });
                }
                s.push_str(&key_name(*key));
                s
            }
        }
    }
}

/// Short display name of a key.
pub fn key_name(key: Key) -> String {
    match key {
        Key::ArrowLeft => "←".into(),
        Key::ArrowRight => "→".into(),
        Key::ArrowUp => "↑".into(),
        Key::ArrowDown => "↓".into(),
        Key::Space => "Space".into(),
        Key::Escape => "Esc".into(),
        Key::Enter => "Enter".into(),
        Key::Backspace => (if cfg!(target_os = "macos") { "⌫" } else { "Backspace" }).into(),
        Key::PageUp => "PgUp".into(),
        Key::PageDown => "PgDn".into(),
        Key::Equals | Key::Plus => "+".into(),
        Key::Minus => "−".into(),
        Key::Num0 => "0".into(),
        Key::Num1 => "1".into(),
        Key::Num2 => "2".into(),
        Key::Num3 => "3".into(),
        Key::Num4 => "4".into(),
        Key::Num5 => "5".into(),
        Key::Num6 => "6".into(),
        Key::Num7 => "7".into(),
        Key::Num8 => "8".into(),
        Key::Num9 => "9".into(),
        Key::Comma => ",".into(),
        other => other.name().to_owned(),
    }
}

/// Static description of an [`Action`].
pub struct Def {
    pub action: Action,
    pub group: Group,
    pub bindings: &'static [Binding],
    /// Held keys repeat the action (seeking, volume…).
    pub repeats: bool,
    /// Offered in the command palette.
    pub in_palette: bool,
}

impl Def {
    /// The shortcuts as shown to the user: two physical keys that read the same (`=` and `+`
    /// both zoom in) are listed once.
    pub fn labels(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for b in self.bindings {
            let l = b.label();
            if !out.contains(&l) {
                out.push(l);
            }
        }
        out
    }
}

use Binding::{Char, Key as K};

macro_rules! def {
    ($a:ident, $g:ident, [$($b:expr),*], $rep:expr, $pal:expr) => {
        Def { action: Action::$a, group: Group::$g, bindings: &[$($b),*], repeats: $rep, in_palette: $pal }
    };
}

const N: Mods = Mods::NONE;

/// The complete command table.
pub static DEFS: &[Def] = &[
    // --- playback ------------------------------------------------------------------------------
    def!(PlayPause, Playback, [K(Key::Space, N), K(Key::K, N)], false, true),
    def!(Stop, Playback, [K(Key::S, N)], false, true),
    def!(Next, Playback, [K(Key::N, N), K(Key::PageDown, N)], false, true),
    def!(Previous, Playback, [K(Key::P, N), K(Key::PageUp, N)], false, true),
    def!(FrameNext, Playback, [Char('.')], true, true),
    def!(FramePrevious, Playback, [Char(',')], true, true),
    def!(SpeedUp, Playback, [Char(']')], true, true),
    def!(SpeedDown, Playback, [Char('[')], true, true),
    def!(SpeedReset, Playback, [K(Key::Backspace, N)], false, true),
    def!(ToggleRepeat, Playback, [K(Key::L, N)], false, true),
    def!(ToggleShuffle, Playback, [K(Key::R, N)], false, true),
    def!(ABLoop, Playback, [K(Key::L, Mods::SHIFT)], false, true),
    // --- navigation ----------------------------------------------------------------------------
    def!(SeekBackShort, Navigation, [K(Key::ArrowLeft, N)], true, true),
    def!(SeekForwardShort, Navigation, [K(Key::ArrowRight, N)], true, true),
    def!(SeekBackMedium, Navigation, [K(Key::ArrowLeft, Mods::SHIFT)], true, true),
    def!(SeekForwardMedium, Navigation, [K(Key::ArrowRight, Mods::SHIFT)], true, true),
    def!(SeekBackLong, Navigation, [K(Key::ArrowLeft, Mods::CTRL)], true, true),
    def!(SeekForwardLong, Navigation, [K(Key::ArrowRight, Mods::CTRL)], true, true),
    def!(PreviousChapter, Navigation, [K(Key::PageUp, Mods::CTRL)], false, true),
    def!(NextChapter, Navigation, [K(Key::PageDown, Mods::CTRL)], false, true),
    def!(JumpToTime, Navigation, [K(Key::T, Mods::CTRL)], false, true),
    // --- audio ---------------------------------------------------------------------------------
    def!(VolumeUp, Audio, [K(Key::ArrowUp, N)], true, true),
    def!(VolumeDown, Audio, [K(Key::ArrowDown, N)], true, true),
    def!(ToggleMute, Audio, [K(Key::M, N)], false, true),
    def!(CycleAudio, Audio, [K(Key::B, N)], false, true),
    def!(AudioDelayEarlier, Audio, [K(Key::Z, Mods::CTRL)], true, true),
    def!(AudioDelayLater, Audio, [K(Key::X, Mods::CTRL)], true, true),
    def!(OpenEqualizer, Audio, [K(Key::E, Mods::CTRL)], false, true),
    def!(ToggleNightMode, Audio, [K(Key::N, Mods::SHIFT)], false, true),
    // --- subtitles -----------------------------------------------------------------------------
    def!(CycleSubtitles, Subtitles, [K(Key::V, N)], false, true),
    def!(LoadSubtitles, Subtitles, [K(Key::S, Mods::CTRL_SHIFT)], false, true),
    def!(SubtitleDelayEarlier, Subtitles, [K(Key::Z, N)], true, true),
    def!(SubtitleDelayLater, Subtitles, [K(Key::X, N)], true, true),
    // --- picture -------------------------------------------------------------------------------
    def!(ToggleFullscreen, Picture, [K(Key::F, N), K(Key::F11, N), K(Key::Enter, Mods::ALT)], false, true),
    def!(CycleAspect, Picture, [K(Key::A, N)], false, true),
    def!(CycleFit, Picture, [K(Key::A, Mods::SHIFT)], false, true),
    def!(RotateRight, Picture, [K(Key::R, Mods::SHIFT)], false, true),
    def!(FlipHorizontal, Picture, [K(Key::F, Mods::SHIFT)], false, true),
    def!(ZoomIn, Picture, [K(Key::Equals, Mods::CTRL), K(Key::Plus, Mods::CTRL)], true, true),
    def!(ZoomOut, Picture, [K(Key::Minus, Mods::CTRL)], true, true),
    def!(ZoomReset, Picture, [K(Key::Num0, Mods::CTRL)], false, true),
    def!(ResetPicture, Picture, [K(Key::R, Mods::CTRL_SHIFT)], false, true),
    def!(Screenshot, Picture, [K(Key::S, Mods::SHIFT)], false, true),
    def!(ToggleAmbient, Picture, [K(Key::G, Mods::SHIFT)], false, true),
    def!(CycleDeinterlace, Picture, [K(Key::D, N)], false, true),
    // --- files ---------------------------------------------------------------------------------
    def!(OpenFile, Files, [K(Key::O, Mods::CTRL)], false, true),
    def!(OpenFolder, Files, [K(Key::O, Mods::CTRL_SHIFT)], false, true),
    def!(OpenUrl, Files, [K(Key::N, Mods::CTRL), K(Key::U, Mods::CTRL)], false, true),
    def!(AddFiles, Files, [K(Key::A, Mods::CTRL_SHIFT)], false, true),
    // --- window --------------------------------------------------------------------------------
    def!(TogglePlaylist, Window, [K(Key::L, Mods::CTRL)], false, true),
    def!(OpenSettings, Window, [K(Key::P, Mods::CTRL), K(Key::Comma, Mods::CTRL)], false, true),
    def!(ToggleInfo, Window, [K(Key::J, Mods::CTRL), K(Key::I, Mods::CTRL)], false, true),
    def!(ToggleStats, Window, [K(Key::I, N)], false, true),
    def!(ShowShortcuts, Window, [K(Key::F1, N), Char('?')], false, true),
    def!(OpenPalette, Window, [K(Key::K, Mods::CTRL), K(Key::P, Mods::CTRL_SHIFT)], false, false),
    def!(ToggleAlwaysOnTop, Window, [K(Key::T, Mods::SHIFT)], false, true),
    def!(ToggleRemaining, Window, [K(Key::T, N)], false, true),
    def!(Quit, Window, [K(Key::Q, Mods::CTRL)], false, true),
];

impl Action {
    pub fn def(self) -> &'static Def {
        DEFS.iter().find(|d| d.action == self).expect("every action is in the table")
    }

    /// Name shown in menus, the palette and the cheat sheet.
    pub fn label(self, s: &Settings) -> String {
        let secs = |v: f64| if (v - v.round()).abs() < 1e-6 { format!("{}", v.round() as i64) } else { format!("{v:.1}") };
        match self {
            Action::SeekBackShort => tf(T::SeekBack, &[&secs(s.seek_steps[0])]),
            Action::SeekForwardShort => tf(T::SeekForward, &[&secs(s.seek_steps[0])]),
            Action::SeekBackMedium => tf(T::SeekBack, &[&secs(s.seek_steps[1])]),
            Action::SeekForwardMedium => tf(T::SeekForward, &[&secs(s.seek_steps[1])]),
            Action::SeekBackLong => tf(T::SeekBack, &[&secs(s.seek_steps[2])]),
            Action::SeekForwardLong => tf(T::SeekForward, &[&secs(s.seek_steps[2])]),
            Action::AudioDelayEarlier => tf(T::ActAudioEarlier, &[&format_delay(-AUDIO_DELAY_STEP)]),
            Action::AudioDelayLater => tf(T::ActAudioLater, &[&format_delay(AUDIO_DELAY_STEP)]),
            Action::SubtitleDelayEarlier => tf(T::ActSubtitleEarlier, &[&format_delay(-SUBTITLE_DELAY_STEP)]),
            Action::SubtitleDelayLater => tf(T::ActSubtitleLater, &[&format_delay(SUBTITLE_DELAY_STEP)]),
            other => t(other.label_key()).to_owned(),
        }
    }

    fn label_key(self) -> T {
        match self {
            Action::PlayPause => T::ActPlayPause,
            Action::Stop => T::Stop,
            Action::Next => T::Next,
            Action::Previous => T::Previous,
            Action::FrameNext => T::FrameNext,
            Action::FramePrevious => T::FramePrevious,
            Action::SpeedUp => T::ActSpeedUp,
            Action::SpeedDown => T::ActSpeedDown,
            Action::SpeedReset => T::ActSpeedReset,
            Action::NextChapter => T::NextChapter,
            Action::PreviousChapter => T::PreviousChapter,
            Action::ABLoop => T::ABLoop,
            Action::JumpToTime => T::JumpToTime,
            Action::ToggleRepeat => T::ActRepeat,
            Action::ToggleShuffle => T::Shuffle,
            Action::VolumeUp => T::ActVolumeUp,
            Action::VolumeDown => T::ActVolumeDown,
            Action::ToggleMute => T::ActMute,
            Action::CycleAudio => T::ActCycleAudio,
            Action::OpenEqualizer => T::Equalizer,
            Action::ToggleNightMode => T::NightMode,
            Action::CycleSubtitles => T::ActCycleSubtitles,
            Action::LoadSubtitles => T::LoadSubtitles,
            Action::ToggleFullscreen => T::ActFullscreen,
            Action::CycleAspect => T::ActCycleAspect,
            Action::CycleFit => T::ActCycleFit,
            Action::RotateRight => T::RotateRight,
            Action::FlipHorizontal => T::FlipHorizontal,
            Action::ZoomIn => T::ZoomIn,
            Action::ZoomOut => T::ZoomOut,
            Action::ZoomReset => T::ZoomReset,
            Action::ResetPicture => T::ResetPicture,
            Action::Screenshot => T::Screenshot,
            Action::ToggleAmbient => T::AmbientGlow,
            Action::CycleDeinterlace => T::ActCycleDeinterlace,
            Action::OpenFile => T::OpenFiles,
            Action::OpenFolder => T::OpenFolder,
            Action::OpenUrl => T::OpenUrl,
            Action::AddFiles => T::AddFiles,
            Action::TogglePlaylist => T::Playlist,
            Action::OpenSettings => T::Settings,
            Action::ToggleInfo => T::MediaInfo,
            Action::ToggleStats => T::Statistics,
            Action::ShowShortcuts => T::Shortcuts,
            Action::OpenPalette => T::CommandPalette,
            Action::ToggleAlwaysOnTop => T::AlwaysOnTop,
            Action::ToggleRemaining => T::ShowRemaining,
            Action::Quit => T::Quit,
            // Labels with a parameter are handled in `label`.
            Action::SeekBackShort
            | Action::SeekForwardShort
            | Action::SeekBackMedium
            | Action::SeekForwardMedium
            | Action::SeekBackLong
            | Action::SeekForwardLong
            | Action::AudioDelayEarlier
            | Action::AudioDelayLater
            | Action::SubtitleDelayEarlier
            | Action::SubtitleDelayLater => T::Unknown,
        }
    }

    /// First shortcut, for display next to the label.
    pub fn shortcut_label(self) -> Option<String> {
        self.def().bindings.first().map(Binding::label)
    }

    /// All shortcuts, `"Space / K"`.
    pub fn shortcuts_label(self) -> String {
        self.def().labels().join(" / ")
    }
}

/// Step of the audio delay shortcuts, seconds.
pub const AUDIO_DELAY_STEP: f64 = 0.05;
/// Step of the subtitle delay shortcuts, seconds.
pub const SUBTITLE_DELAY_STEP: f64 = 0.1;

/// The actions triggered by the keyboard events of this frame, in order. Events are *not*
/// consumed here; the caller decides whether a text field owns the keyboard.
pub fn actions_from_events(events: &[Event]) -> Vec<Action> {
    let mut out = Vec::new();
    for ev in events {
        for def in DEFS {
            if def.bindings.iter().any(|b| b.matches(ev, def.repeats)) {
                out.push(def.action);
                break;
            }
        }
    }
    out
}

/// Number keys: `Some(n)` for `0`…`9` pressed without modifiers (seek to `n × 10 %`).
pub fn percent_key(ev: &Event) -> Option<u8> {
    let Event::Key { key, pressed: true, repeat: false, modifiers, .. } = ev else { return None };
    if modifiers.command || modifiers.alt || modifiers.shift {
        return None;
    }
    Some(match key {
        Key::Num0 => 0,
        Key::Num1 => 1,
        Key::Num2 => 2,
        Key::Num3 => 3,
        Key::Num4 => 4,
        Key::Num5 => 5,
        Key::Num6 => 6,
        Key::Num7 => 7,
        Key::Num8 => 8,
        Key::Num9 => 9,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn key_event(key: Key, command: bool, shift: bool, alt: bool, repeat: bool) -> Event {
        Event::Key { key, physical_key: None, pressed: true, repeat, modifiers: egui::Modifiers { alt, ctrl: command, shift, mac_cmd: false, command } }
    }

    #[test]
    fn every_action_is_in_the_table_exactly_once() {
        let mut seen: HashMap<Action, usize> = HashMap::new();
        for d in DEFS {
            *seen.entry(d.action).or_default() += 1;
            assert!(!d.bindings.is_empty() || d.action == Action::OpenPalette, "{:?} has no shortcut", d.action);
        }
        for (a, n) in &seen {
            assert_eq!(*n, 1, "{a:?} is listed {n} times");
        }
        // `Action::def` must work for each of them (it panics otherwise).
        for d in DEFS {
            assert_eq!(d.action.def().action, d.action);
        }
    }

    #[test]
    fn no_shortcut_is_bound_twice() {
        let mut owner: HashMap<Binding, Action> = HashMap::new();
        for d in DEFS {
            for b in d.bindings {
                if let Some(prev) = owner.insert(*b, d.action) {
                    panic!("{} is bound to both {prev:?} and {:?}", b.label(), d.action);
                }
            }
        }
    }

    #[test]
    fn labels_exist_in_both_languages() {
        let s = Settings::default();
        for lang in [lumen400_core::settings::Language::French, lumen400_core::settings::Language::English] {
            let _guard = crate::i18n::test_lock();
            crate::i18n::set_language(lang);
            for d in DEFS {
                let label = d.action.label(&s);
                assert!(!label.is_empty() && label != t(T::Unknown), "{:?} has no label", d.action);
                assert!(!label.contains("{0}"), "{:?}: unfilled placeholder in {label}", d.action);
            }
        }
    }

    #[test]
    fn events_map_to_actions() {
        let ev = [key_event(Key::Space, false, false, false, false)];
        assert_eq!(actions_from_events(&ev), vec![Action::PlayPause]);
        // Exact modifiers: Shift+Left is the medium step, not the short one.
        assert_eq!(actions_from_events(&[key_event(Key::ArrowLeft, false, true, false, false)]), vec![Action::SeekBackMedium]);
        assert_eq!(actions_from_events(&[key_event(Key::ArrowLeft, true, false, false, false)]), vec![Action::SeekBackLong]);
        assert_eq!(actions_from_events(&[key_event(Key::ArrowLeft, false, false, false, false)]), vec![Action::SeekBackShort]);
        // An unbound combination does nothing.
        assert!(actions_from_events(&[key_event(Key::Space, true, false, false, false)]).is_empty());
        // Repeats: seeking repeats, toggling does not.
        assert_eq!(actions_from_events(&[key_event(Key::ArrowRight, false, false, false, true)]), vec![Action::SeekForwardShort]);
        assert!(actions_from_events(&[key_event(Key::Space, false, false, false, true)]).is_empty());
        // Typed characters: layout independent.
        assert_eq!(actions_from_events(&[Event::Text("[".into())]), vec![Action::SpeedDown]);
        assert_eq!(actions_from_events(&[Event::Text("?".into())]), vec![Action::ShowShortcuts]);
        assert!(actions_from_events(&[Event::Text("[]".into())]).is_empty());
        // Several events in one frame keep their order.
        let evs = [key_event(Key::M, false, false, false, false), key_event(Key::F, false, false, false, false)];
        assert_eq!(actions_from_events(&evs), vec![Action::ToggleMute, Action::ToggleFullscreen]);
    }

    #[test]
    fn percent_keys() {
        assert_eq!(percent_key(&key_event(Key::Num5, false, false, false, false)), Some(5));
        assert_eq!(percent_key(&key_event(Key::Num5, true, false, false, false)), None);
        assert_eq!(percent_key(&key_event(Key::A, false, false, false, false)), None);
        assert_eq!(percent_key(&key_event(Key::Num5, false, false, false, true)), None);
    }

    #[test]
    fn labels_are_readable() {
        let b = Binding::Key(Key::S, Mods::CTRL_SHIFT);
        let l = b.label();
        assert!(l.contains('S') && l.len() > 1, "{l}");
        assert_eq!(Binding::Char('[').label(), "[");
        assert_eq!(Binding::Key(Key::ArrowLeft, Mods::NONE).label(), "←");
        assert_eq!(Action::PlayPause.shortcuts_label(), "Space / K");
    }

    #[test]
    fn keys_that_read_the_same_are_listed_once() {
        let l = Action::ZoomIn.def().labels();
        let mut unique = l.clone();
        unique.dedup();
        assert_eq!(l, unique);
        assert_eq!(l.len(), 1, "{l:?}");
    }
}

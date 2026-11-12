//! A scripted driver for the interface, used to take screenshots and run end-to-end checks
//! without a human (`400_media_player --automation script.txt`, see `docs/DEVELOPMENT.md`).
//!
//! A script is a text file with one command per line (`#` starts a comment):
//!
//! ```text
//! wait 1.5                 # seconds
//! wait_play 3              # until the playback position reaches 3 s
//! key ctrl+k               # a key press (modifiers: ctrl, shift, alt)
//! text hello               # typed characters
//! move 400 300 / click 400 300 / rclick 400 300 / dclick 400 300
//! scroll -1                # mouse wheel, in notches
//! action ToggleInfo        # any command of the actions table
//! open /path/to/file.mp4   # replace the queue
//! tab about                # open the settings on a tab
//! set theme light          # theme dark|oled|light|system, accent <name>, lang fr|en
//! resize 1280 720
//! shot welcome.png         # window screenshot, next to the script unless absolute
//! expect state playing     # idle|opening|buffering|playing|paused|ended|failed
//! expect pos_ge 2.5
//! quit
//! ```
//!
//! Failed `expect` lines are reported on stderr and make the process exit with a non-zero code.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::time::Duration;

use egui::{Context, Event, Key, Modifiers, PointerButton, RawInput, pos2, vec2};
use media_player_400_core::settings::{Accent, Language, ThemeMode};
use media_player_400_engine::PlayState;

use crate::actions::{Action, DEFS};
use crate::app::MediaPlayer400App;
use crate::state::SettingsTab;

/// Exit code requested by the script (the number of failed expectations).
pub static EXIT_CODE: AtomicI32 = AtomicI32::new(0);

#[derive(Debug, PartialEq)]
enum Step {
    Wait(f64),
    WaitFrames(u32),
    WaitPlay(f64),
    Key(Key, Modifiers),
    Text(String),
    Move(f32, f32),
    Click(f32, f32, PointerButton),
    DoubleClick(f32, f32),
    Scroll(f32),
    Action(Action),
    Open(String),
    Tab(SettingsTab),
    SetTheme(ThemeMode),
    SetAccent(Accent),
    SetLang(Language),
    Resize(f32, f32),
    Shot(String),
    ExpectState(PlayState),
    ExpectPosGe(f64),
    Quit,
}

pub struct Automation {
    steps: Vec<Step>,
    next: usize,
    base_dir: PathBuf,
    wait_until: Option<f64>,
    wait_frames: u32,
    wait_play: Option<(f64, f64)>,
    /// One entry is injected per frame.
    pending: VecDeque<Vec<Event>>,
    shot_done: Option<Arc<AtomicBool>>,
    failures: i32,
}

fn parse_key(name: &str) -> Option<Key> {
    let n = name.trim();
    let alias = match n.to_ascii_lowercase().as_str() {
        "space" => "Space",
        "enter" | "return" => "Enter",
        "esc" | "escape" => "Escape",
        "left" => "ArrowLeft",
        "right" => "ArrowRight",
        "up" => "ArrowUp",
        "down" => "ArrowDown",
        "tab" => "Tab",
        "backspace" => "Backspace",
        "pageup" | "pgup" => "PageUp",
        "pagedown" | "pgdn" => "PageDown",
        "plus" => "Plus",
        "minus" => "Minus",
        "equals" => "Equals",
        "comma" => "Comma",
        "period" => "Period",
        _ => n,
    };
    Key::from_name(alias).or_else(|| Key::from_name(&alias.to_ascii_uppercase()))
}

/// `ctrl+shift+s` → (S, ctrl+shift).
fn parse_combo(spec: &str) -> Option<(Key, Modifiers)> {
    let mut mods = Modifiers::NONE;
    let mut key = None;
    for part in spec.split('+') {
        match part.trim().to_ascii_lowercase().as_str() {
            "ctrl" | "control" | "cmd" | "command" => {
                mods.ctrl = true;
                mods.command = true;
            }
            "shift" => mods.shift = true,
            "alt" => mods.alt = true,
            "" => {}
            other => key = parse_key(other),
        }
    }
    key.map(|k| (k, mods))
}

fn action_by_name(name: &str) -> Option<Action> {
    DEFS.iter().map(|d| d.action).find(|a| format!("{a:?}").eq_ignore_ascii_case(name))
}

fn parse_state(name: &str) -> Option<PlayState> {
    Some(match name.to_ascii_lowercase().as_str() {
        "idle" => PlayState::Idle,
        "opening" => PlayState::Opening,
        "buffering" => PlayState::Buffering,
        "playing" => PlayState::Playing,
        "paused" => PlayState::Paused,
        "ended" => PlayState::Ended,
        "failed" => PlayState::Failed,
        _ => return None,
    })
}

fn parse_accent(name: &str) -> Option<Accent> {
    Accent::ALL.into_iter().find(|a| a.name().eq_ignore_ascii_case(name))
}

fn parse_line(line: &str) -> Result<Option<Step>, String> {
    let line = line.split('#').next().unwrap_or("").trim();
    if line.is_empty() {
        return Ok(None);
    }
    let (cmd, rest) = line.split_once(char::is_whitespace).map_or((line, ""), |(c, r)| (c, r.trim()));
    let num = |s: &str| s.trim().parse::<f64>().map_err(|_| format!("not a number: {s:?}"));
    let two = |s: &str| -> Result<(f32, f32), String> {
        let mut it = s.split_whitespace();
        match (it.next(), it.next()) {
            (Some(a), Some(b)) => Ok((num(a)? as f32, num(b)? as f32)),
            _ => Err("two numbers expected".to_owned()),
        }
    };
    let step = match cmd.to_ascii_lowercase().as_str() {
        "wait" => Step::Wait(num(rest)?),
        "wait_frames" => Step::WaitFrames(num(rest)? as u32),
        "wait_play" => Step::WaitPlay(num(rest)?),
        "key" => {
            let (k, m) = parse_combo(rest).ok_or_else(|| format!("unknown key {rest:?}"))?;
            Step::Key(k, m)
        }
        "text" => Step::Text(rest.to_owned()),
        "move" => {
            let (x, y) = two(rest)?;
            Step::Move(x, y)
        }
        "click" => {
            let (x, y) = two(rest)?;
            Step::Click(x, y, PointerButton::Primary)
        }
        "rclick" => {
            let (x, y) = two(rest)?;
            Step::Click(x, y, PointerButton::Secondary)
        }
        "dclick" => {
            let (x, y) = two(rest)?;
            Step::DoubleClick(x, y)
        }
        "scroll" => Step::Scroll(num(rest)? as f32),
        "action" => Step::Action(action_by_name(rest).ok_or_else(|| format!("unknown action {rest:?}"))?),
        "open" => Step::Open(rest.to_owned()),
        "tab" => Step::Tab(match rest.to_ascii_lowercase().as_str() {
            "appearance" => SettingsTab::Appearance,
            "playback" => SettingsTab::Playback,
            "audio" => SettingsTab::Audio,
            "subtitles" => SettingsTab::Subtitles,
            "advanced" => SettingsTab::Advanced,
            "about" => SettingsTab::About,
            other => return Err(format!("unknown tab {other:?}")),
        }),
        "set" => {
            let (what, value) = rest.split_once(char::is_whitespace).map(|(a, b)| (a, b.trim())).ok_or("set needs a name and a value")?;
            match what.to_ascii_lowercase().as_str() {
                "theme" => Step::SetTheme(match value.to_ascii_lowercase().as_str() {
                    "dark" => ThemeMode::Dark,
                    "oled" => ThemeMode::Oled,
                    "light" => ThemeMode::Light,
                    "system" => ThemeMode::System,
                    other => return Err(format!("unknown theme {other:?}")),
                }),
                "accent" => Step::SetAccent(parse_accent(value).ok_or_else(|| format!("unknown accent {value:?}"))?),
                "lang" => Step::SetLang(match value.to_ascii_lowercase().as_str() {
                    "fr" => Language::French,
                    "en" => Language::English,
                    other => return Err(format!("unknown language {other:?}")),
                }),
                other => return Err(format!("cannot set {other:?}")),
            }
        }
        "resize" => {
            let (w, h) = two(rest)?;
            Step::Resize(w, h)
        }
        "shot" => Step::Shot(rest.to_owned()),
        "expect" => {
            let (what, value) = rest.split_once(char::is_whitespace).map(|(a, b)| (a, b.trim())).ok_or("expect needs a name and a value")?;
            match what {
                "state" => Step::ExpectState(parse_state(value).ok_or_else(|| format!("unknown state {value:?}"))?),
                "pos_ge" => Step::ExpectPosGe(num(value)?),
                other => return Err(format!("cannot expect {other:?}")),
            }
        }
        "quit" => Step::Quit,
        other => return Err(format!("unknown command {other:?}")),
    };
    Ok(Some(step))
}

impl Automation {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let base_dir = path.parent().map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        Self::parse(&text, base_dir)
    }

    pub fn parse(text: &str, base_dir: PathBuf) -> Result<Self, String> {
        let mut steps = Vec::new();
        for (i, line) in text.lines().enumerate() {
            match parse_line(line) {
                Ok(Some(s)) => steps.push(s),
                Ok(None) => {}
                Err(e) => return Err(format!("line {}: {e}", i + 1)),
            }
        }
        Ok(Self { steps, next: 0, base_dir, wait_until: None, wait_frames: 0, wait_play: None, pending: VecDeque::new(), shot_done: None, failures: 0 })
    }

    /// Adds the scripted events to the frame's raw input.
    pub fn inject(&mut self, _ctx: &Context, raw: &mut RawInput) {
        if let Some(events) = self.pending.pop_front() {
            raw.events.extend(events);
        }
    }

    fn fail(&mut self, what: &str) {
        self.failures += 1;
        EXIT_CODE.store(self.failures, Ordering::Relaxed);
        eprintln!("AUTOMATION FAIL: {what}");
    }

    /// Runs the script as far as it can this frame.
    pub fn step(&mut self, ctx: &Context, app: &mut MediaPlayer400App, now: f64) {
        if self.next >= self.steps.len() && self.pending.is_empty() {
            return;
        }
        // The script drives the UI frame by frame: never let the loop go to sleep.
        ctx.request_repaint_after(Duration::from_millis(16));

        if !self.pending.is_empty() {
            return;
        }
        if let Some(done) = &self.shot_done {
            if !done.load(Ordering::Acquire) {
                return;
            }
            self.shot_done = None;
        }
        if self.wait_frames > 0 {
            self.wait_frames -= 1;
            return;
        }
        if let Some(t) = self.wait_until {
            if now < t {
                return;
            }
            self.wait_until = None;
        }
        if let Some((target, deadline)) = self.wait_play {
            if app.snap.position >= target {
                self.wait_play = None;
            } else if now > deadline {
                self.wait_play = None;
                self.fail(&format!("playback did not reach {target:.1} s (at {:.1} s, state {:?})", app.snap.position, app.snap.state));
            } else {
                return;
            }
        }

        while self.next < self.steps.len() {
            let i = self.next;
            self.next += 1;
            let blocked = self.run_step(i, ctx, app, now);
            if blocked || !self.pending.is_empty() {
                break;
            }
        }
    }

    /// Executes step `i`; `true` when later steps have to wait for a following frame.
    fn run_step(&mut self, i: usize, ctx: &Context, app: &mut MediaPlayer400App, now: f64) -> bool {
        let mods_none = Modifiers::NONE;
        match &self.steps[i] {
            Step::Wait(s) => {
                self.wait_until = Some(now + s);
                true
            }
            Step::WaitFrames(n) => {
                self.wait_frames = *n;
                true
            }
            Step::WaitPlay(secs) => {
                self.wait_play = Some((*secs, now + 40.0));
                true
            }
            Step::Key(key, mods) => {
                let (key, mods) = (*key, *mods);
                self.pending.push_back(vec![Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers: mods }]);
                self.pending.push_back(vec![Event::Key { key, physical_key: Some(key), pressed: false, repeat: false, modifiers: mods }]);
                true
            }
            Step::Text(s) => {
                self.pending.push_back(vec![Event::Text(s.clone())]);
                true
            }
            Step::Move(x, y) => {
                self.pending.push_back(vec![Event::PointerMoved(pos2(*x, *y))]);
                true
            }
            Step::Click(x, y, button) => {
                let pos = pos2(*x, *y);
                self.pending.push_back(vec![Event::PointerMoved(pos)]);
                self.pending.push_back(vec![Event::PointerButton { pos, button: *button, pressed: true, modifiers: mods_none }]);
                self.pending.push_back(vec![Event::PointerButton { pos, button: *button, pressed: false, modifiers: mods_none }]);
                true
            }
            Step::DoubleClick(x, y) => {
                let pos = pos2(*x, *y);
                self.pending.push_back(vec![Event::PointerMoved(pos)]);
                for _ in 0..2 {
                    self.pending.push_back(vec![Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: mods_none }]);
                    self.pending.push_back(vec![Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: mods_none }]);
                }
                true
            }
            Step::Scroll(dy) => {
                self.pending.push_back(vec![Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Line,
                    delta: vec2(0.0, *dy),
                    phase: egui::TouchPhase::Move,
                    source: egui::MouseWheelSource::Wheel,
                    modifiers: mods_none,
                }]);
                true
            }
            Step::Action(a) => {
                app.run(*a);
                true
            }
            Step::Open(src) => {
                app.open_inputs(vec![src.clone()], false);
                true
            }
            Step::Tab(tab) => {
                app.windows.settings = Some(*tab);
                true
            }
            Step::SetTheme(m) => {
                app.settings.theme = *m;
                app.apply_look();
                true
            }
            Step::SetAccent(a) => {
                app.settings.accent = *a;
                app.apply_look();
                true
            }
            Step::SetLang(l) => {
                app.settings.language = *l;
                app.apply_look();
                true
            }
            Step::Resize(w, h) => {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(vec2(*w, *h)));
                self.wait_frames = 6;
                true
            }
            Step::Shot(name) => {
                let path = if Path::new(name).is_absolute() { PathBuf::from(name) } else { self.base_dir.join(name) };
                let done = Arc::new(AtomicBool::new(false));
                self.shot_done = Some(done.clone());
                let repaint = ctx.clone();
                ctx.request_screenshot(move |img| {
                    let [w, h] = img.size;
                    let rgba = img.as_raw().to_vec();
                    match image::RgbaImage::from_raw(w as u32, h as u32, rgba) {
                        Some(buf) => {
                            if let Some(dir) = path.parent() {
                                let _ = std::fs::create_dir_all(dir);
                            }
                            match buf.save(&path) {
                                Ok(()) => eprintln!("AUTOMATION shot {} ({w}×{h})", path.display()),
                                Err(e) => eprintln!("AUTOMATION FAIL: cannot write {}: {e}", path.display()),
                            }
                        }
                        None => eprintln!("AUTOMATION FAIL: bad screenshot buffer"),
                    }
                    done.store(true, Ordering::Release);
                    repaint.request_repaint();
                });
                true
            }
            Step::ExpectState(state) => {
                let actual = app.snap.state;
                if actual != *state {
                    let msg = format!("expected state {state:?}, got {actual:?}");
                    self.fail(&msg);
                }
                false
            }
            Step::ExpectPosGe(secs) => {
                let pos = app.snap.position;
                if pos + 1e-6 < *secs {
                    let msg = format!("expected position ≥ {secs:.2} s, got {pos:.2} s");
                    self.fail(&msg);
                }
                false
            }
            Step::Quit => {
                EXIT_CODE.store(self.failures, Ordering::Relaxed);
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_script() {
        let text = "
            # a comment
            wait 1.5
            key ctrl+k
            key space
            text hello world
            click 10 20
            action ToggleInfo
            tab about
            set theme light
            set accent rose
            shot a/b.png   # trailing comment
            expect state playing
            quit
        ";
        let a = Automation::parse(text, PathBuf::new()).unwrap();
        assert_eq!(a.steps.len(), 12);
        assert_eq!(a.steps[0], Step::Wait(1.5));
        assert!(matches!(a.steps[1], Step::Key(Key::K, m) if m.command && !m.shift));
        assert_eq!(a.steps[3], Step::Text("hello world".into()));
        assert_eq!(a.steps[5], Step::Action(Action::ToggleInfo));
        assert_eq!(a.steps[9], Step::Shot("a/b.png".into()));
        assert_eq!(a.steps[10], Step::ExpectState(PlayState::Playing));
    }

    #[test]
    fn reports_the_line_of_an_error() {
        let e = Automation::parse("wait 1\nbogus 3\n", PathBuf::new()).err().unwrap();
        assert!(e.starts_with("line 2:"), "{e}");
        assert!(Automation::parse("key nonsense+zz", PathBuf::new()).is_err());
        assert!(Automation::parse("action NoSuchAction", PathBuf::new()).is_err());
        assert!(Automation::parse("click 1", PathBuf::new()).is_err());
    }

    #[test]
    fn key_names() {
        assert_eq!(parse_combo("space"), Some((Key::Space, Modifiers::NONE)));
        assert_eq!(parse_combo("left").map(|c| c.0), Some(Key::ArrowLeft));
        assert_eq!(parse_combo("ctrl+shift+s").map(|c| (c.0, c.1.shift, c.1.command)), Some((Key::S, true, true)));
        assert_eq!(parse_combo("f11").map(|c| c.0), Some(Key::F11));
        assert_eq!(parse_combo("5").map(|c| c.0), Some(Key::Num5));
        assert_eq!(parse_combo("esc").map(|c| c.0), Some(Key::Escape));
    }

    #[test]
    fn action_names_are_case_insensitive() {
        assert_eq!(action_by_name("toggleinfo"), Some(Action::ToggleInfo));
        assert_eq!(action_by_name("Quit"), Some(Action::Quit));
        assert_eq!(action_by_name("nope"), None);
    }
}

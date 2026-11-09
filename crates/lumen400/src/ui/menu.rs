//! The popup menu: the context menu of the picture and the "more" button of the dock.
//!
//! Sub-menus are *pages* the menu drills into (with a back row on top) rather than fly-outs that
//! open sideways: they work the same with a mouse or a touch screen and can never fall off the
//! edge of the window.

use egui::{Align2, Context, CornerRadius, FontId, Id, Margin, Order, Rect, Sense, Ui, pos2, vec2};
use lumen400_core::lang;
use lumen400_core::playlist::RepeatMode;
use lumen400_core::types::{AudioStreamInfo, MediaInfo, SubtitleStreamInfo};

use crate::actions::Action;
use crate::app::LumenApp;
use crate::control::{ASPECTS, SPEEDS};
use crate::i18n::{self, Key as T, t, t1};
use crate::state::{AbLoop, Page, SubSource};
use crate::theme::{Palette, fade, fonts, mix};
use crate::ui::icons;
use crate::util::{channels_label, codec_display, ellipsize_middle, format_delay, format_speed};
use crate::video::FitMode;
use crate::widgets::{Lead, ThinSlider, menu_item, menu_separator};

const MENU_WIDTH: f32 = 296.0;

/// What a row asked for; applied once the menu has been drawn.
enum Cmd {
    Push(Page),
    Pop,
    /// Run an action and close the menu.
    Run(Action),
    /// Run an action and keep the menu open (steppers).
    RunKeep(Action),
    Audio(usize),
    Subs(SubSource),
    Speed(f64),
    Aspect(Option<f32>),
    Fit(FitMode),
    Repeat(RepeatMode),
    LoadPlaylist,
    SavePlaylist,
    ResetAudioDelay,
    ResetSubtitleDelay,
}

impl LumenApp {
    pub fn draw_menu(&mut self, ctx: &Context, now: f64) {
        let Some(mut menu) = self.menu.take() else { return };
        let p = self.pal.clone();
        let screen = ctx.content_rect();
        let page = menu.page();
        let appear = ctx.animate_bool_with_time(Id::unique(("menu-appear", menu.opened_at.to_bits())), true, 0.12);
        let rise = (1.0 - appear) * if menu.above { 8.0 } else { -8.0 };
        let pivot = if menu.above { Align2::CENTER_BOTTOM } else { Align2::LEFT_TOP };
        let mut cmds: Vec<Cmd> = Vec::new();

        let out = egui::Area::new(Id::unique("menu")).order(Order::Foreground).pivot(pivot).fixed_pos(menu.pos + vec2(0.0, rise)).constrain(true).show(ctx, |ui| {
            ui.set_opacity(appear);
            p.popup_frame().inner_margin(Margin::same(6)).show(ui, |ui| {
                ui.set_width(MENU_WIDTH);
                ui.spacing_mut().item_spacing = vec2(0.0, 1.0);
                let max_h = (screen.height() - 40.0).max(180.0);
                crate::ui::scroll_body(ui, max_h, |ui| match page {
                    Page::Root => self.page_root(ui, &p, &mut cmds),
                    Page::Open => self.page_open(ui, &p, &mut cmds),
                    Page::Playback => self.page_playback(ui, &p, &mut cmds),
                    Page::Subtitles => self.page_subtitles(ui, &p, &mut cmds),
                    Page::Audio => self.page_audio(ui, &p, &mut cmds),
                    Page::Video => self.page_video(ui, &p, &mut cmds),
                    Page::Aspect => self.page_aspect(ui, &p, &mut cmds),
                    Page::Picture => self.page_picture(ui, &p, &mut cmds),
                    Page::Speed => self.page_speed(ui, &p, &mut cmds),
                    Page::Tools => self.page_tools(ui, &p, &mut cmds),
                });
            });
        });

        // A press outside closes the menu — and must not also act on what is underneath.
        let rect = out.response.rect;
        let pressed_outside = ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|pos| !rect.contains(pos)));
        let mut keep = true;
        if pressed_outside && now - menu.opened_at > 0.05 {
            keep = false;
            self.swallow_click = true;
        }

        for cmd in cmds {
            match cmd {
                Cmd::Push(next) => menu.stack.push(next),
                Cmd::Pop => {
                    if menu.stack.len() > 1 {
                        menu.stack.pop();
                    } else {
                        keep = false;
                    }
                }
                Cmd::RunKeep(a) => self.run(a),
                Cmd::Run(a) => {
                    self.run(a);
                    keep = false;
                }
                Cmd::Audio(stream) => {
                    self.select_audio(stream);
                    keep = false;
                }
                Cmd::Subs(source) => {
                    self.select_subtitles(source);
                    keep = false;
                }
                Cmd::Speed(s) => {
                    self.set_speed(s);
                    keep = false;
                }
                Cmd::Aspect(a) => {
                    self.view.fit = FitMode::Fit;
                    self.set_aspect(a);
                    keep = false;
                }
                Cmd::Fit(f) => {
                    self.view.aspect = None;
                    self.set_fit(f);
                    keep = false;
                }
                Cmd::Repeat(m) => {
                    self.set_repeat(m);
                    keep = false;
                }
                Cmd::LoadPlaylist => {
                    let dir = self.settings.last_dir.clone();
                    self.dialogs.load_playlist(dir);
                    keep = false;
                }
                Cmd::SavePlaylist => {
                    let dir = self.settings.last_dir.clone();
                    self.dialogs.save_playlist(dir);
                    keep = false;
                }
                Cmd::ResetAudioDelay => {
                    self.shift_audio_delay(-self.audio_delay);
                }
                Cmd::ResetSubtitleDelay => {
                    self.shift_subtitle_delay(-self.sub_delay);
                }
            }
        }
        if keep && self.menu.is_none() {
            self.menu = Some(menu);
        }
    }

    // =============================================================================================
    // Pages
    // =============================================================================================

    fn page_root(&mut self, ui: &mut Ui, p: &Palette, cmds: &mut Vec<Cmd>) {
        let has_media = self.source.is_some();
        let media = self.media.clone();
        let has_video = media.as_ref().is_some_and(|m| m.has_video());
        if has_media {
            let (icon, label) = if self.is_playing() { (icons::PAUSE, t(T::Pause)) } else { (icons::PLAY, t(T::Play)) };
            action_row(ui, p, cmds, Lead::Icon(icon), label, Action::PlayPause, true);
            let (icon, label) = if self.fullscreen { (icons::ARROWS_IN, t(T::ExitFullscreen)) } else { (icons::ARROWS_OUT, t(T::Fullscreen)) };
            action_row(ui, p, cmds, Lead::Icon(icon), label, Action::ToggleFullscreen, true);
            menu_separator(ui, p);

            let repeat_hint = match self.playlist.repeat() {
                RepeatMode::Off => None,
                RepeatMode::All => Some(t(T::RepeatAllShort)),
                RepeatMode::One => Some(t(T::RepeatOneShort)),
            };
            drill(ui, p, cmds, icons::PLAY_CIRCLE, t(T::GroupPlayback), repeat_hint.map(str::to_owned), Page::Playback);
            let sub_hint = match self.sub_source {
                SubSource::Off => t(T::SubtitlesOff).to_owned(),
                SubSource::External => self.ext_sub.as_ref().map(|s| s.name.clone()).unwrap_or_default(),
                SubSource::Embedded(i) => media.as_ref().and_then(|m| m.subtitles.iter().position(|s| s.index == i).map(|n| subtitle_name(&m.subtitles[n], n))).unwrap_or_default(),
            };
            drill(ui, p, cmds, icons::SUBTITLES, t(T::Subtitles), Some(sub_hint), Page::Subtitles);
            let audio_hint = media.as_ref().and_then(|m| current_audio(m).map(|(n, a)| audio_name(a, n)));
            drill(ui, p, cmds, icons::HEADPHONES, t(T::AudioTrack), audio_hint, Page::Audio);
            if has_video {
                drill(ui, p, cmds, icons::MONITOR, t(T::Picture), None, Page::Video);
            }
            let speed = self.snap.speed;
            let speed_hint = if (speed - 1.0).abs() < 1e-3 { t(T::SpeedNormal).to_owned() } else { format!("×{}", format_speed(speed)) };
            drill(ui, p, cmds, icons::GAUGE, t(T::Speed), Some(speed_hint), Page::Speed);
            menu_separator(ui, p);

            if has_video {
                action_row(ui, p, cmds, Lead::Icon(icons::CAMERA), t(T::Screenshot), Action::Screenshot, true);
            }
            action_row(ui, p, cmds, Lead::Icon(icons::PLAYLIST), t(T::Playlist), Action::TogglePlaylist, true);
            action_row(ui, p, cmds, Lead::Icon(icons::EQUALIZER), t(T::Equalizer), Action::OpenEqualizer, true);
            menu_separator(ui, p);
        }
        drill(ui, p, cmds, icons::FOLDER_OPEN, t(T::Open), None, Page::Open);
        drill(ui, p, cmds, icons::TERMINAL, t(T::Tools), None, Page::Tools);
        action_row(ui, p, cmds, Lead::Icon(icons::GEAR), t(T::Settings), Action::OpenSettings, true);
        menu_separator(ui, p);
        action_row(ui, p, cmds, Lead::Icon(icons::X), t(T::Quit), Action::Quit, true);
    }

    fn page_open(&mut self, ui: &mut Ui, p: &Palette, cmds: &mut Vec<Cmd>) {
        back_row(ui, p, cmds, t(T::Open));
        action_row(ui, p, cmds, Lead::Icon(icons::FILM_STRIP), t(T::OpenFiles), Action::OpenFile, true);
        action_row(ui, p, cmds, Lead::Icon(icons::FOLDER_OPEN), t(T::OpenFolder), Action::OpenFolder, true);
        action_row(ui, p, cmds, Lead::Icon(icons::LINK), t(T::OpenUrl), Action::OpenUrl, true);
        menu_separator(ui, p);
        action_row(ui, p, cmds, Lead::Icon(icons::PLUS), t(T::AddFiles), Action::AddFiles, self.source.is_some());
        menu_separator(ui, p);
        if menu_item(ui, p, Lead::Icon(icons::LIST), t(T::LoadPlaylist), None, true).clicked() {
            cmds.push(Cmd::LoadPlaylist);
        }
        if menu_item(ui, p, Lead::Icon(icons::FLOPPY), t(T::SavePlaylist), None, !self.playlist.is_empty()).clicked() {
            cmds.push(Cmd::SavePlaylist);
        }
    }

    fn page_playback(&mut self, ui: &mut Ui, p: &Palette, cmds: &mut Vec<Cmd>) {
        back_row(ui, p, cmds, t(T::GroupPlayback));
        let chapters = self.media.as_ref().map_or(0, |m| m.chapters.len());
        let multi = self.playlist.len() > 1;
        action_row(ui, p, cmds, Lead::Icon(icons::STOP), t(T::Stop), Action::Stop, true);
        action_row(ui, p, cmds, Lead::Icon(icons::SKIP_BACK), t(T::Previous), Action::Previous, multi || self.snap.position > 3.0);
        action_row(ui, p, cmds, Lead::Icon(icons::SKIP_FORWARD), t(T::Next), Action::Next, multi);
        menu_separator(ui, p);
        action_row(ui, p, cmds, Lead::None, t(T::FrameNext), Action::FrameNext, self.media.as_ref().is_some_and(|m| m.has_video()));
        action_row(ui, p, cmds, Lead::None, t(T::FramePrevious), Action::FramePrevious, self.media.as_ref().is_some_and(|m| m.has_video()));
        if chapters > 1 {
            menu_separator(ui, p);
            action_row(ui, p, cmds, Lead::Icon(icons::BOOKMARK), t(T::PreviousChapter), Action::PreviousChapter, true);
            action_row(ui, p, cmds, Lead::Icon(icons::BOOKMARK), t(T::NextChapter), Action::NextChapter, true);
        }
        menu_separator(ui, p);
        let ab_label = match self.ab {
            AbLoop::Off => t(T::ABLoop).to_owned(),
            AbLoop::A(_) => t(T::ABLoopSetEnd).to_owned(),
            AbLoop::Active(a, b) => crate::i18n::tf(T::ABLoopOn, &[&lumen400_core::time::format_clock(a), &lumen400_core::time::format_clock(b)]),
        };
        action_row(ui, p, cmds, Lead::Check(self.ab != AbLoop::Off), &ab_label, Action::ABLoop, true);
        action_row(ui, p, cmds, Lead::Icon(icons::CLOCK), t(T::JumpToTime), Action::JumpToTime, true);
        menu_separator(ui, p);
        for (mode, key) in [(RepeatMode::Off, T::RepeatOff), (RepeatMode::All, T::RepeatAll), (RepeatMode::One, T::RepeatOne)] {
            if menu_item(ui, p, Lead::Radio(self.playlist.repeat() == mode), t(key), None, true).clicked() {
                cmds.push(Cmd::Repeat(mode));
            }
        }
        action_row(ui, p, cmds, Lead::Check(self.playlist.shuffle()), t(T::Shuffle), Action::ToggleShuffle, true);
    }

    fn page_subtitles(&mut self, ui: &mut Ui, p: &Palette, cmds: &mut Vec<Cmd>) {
        back_row(ui, p, cmds, t(T::Subtitles));
        if menu_item(ui, p, Lead::Radio(self.sub_source == SubSource::Off), t(T::SubtitlesOff), Some(&Action::CycleSubtitles.shortcuts_label()), true).clicked() {
            cmds.push(Cmd::Subs(SubSource::Off));
        }
        if let Some(m) = self.media.clone() {
            for (n, s) in m.subtitles.iter().enumerate() {
                let label = subtitle_label(s, n);
                let selected = self.sub_source == SubSource::Embedded(s.index);
                if menu_item(ui, p, Lead::Radio(selected), &ellipsize_middle(&label, 34), None, true).clicked() {
                    cmds.push(Cmd::Subs(SubSource::Embedded(s.index)));
                }
            }
        }
        if let Some(ext) = &self.ext_sub {
            let label = format!("{} · {}", ellipsize_middle(&ext.name, 26), t(T::SubtitleExternal));
            if menu_item(ui, p, Lead::Radio(self.sub_source == SubSource::External), &label, None, true).clicked() {
                cmds.push(Cmd::Subs(SubSource::External));
            }
        }
        menu_separator(ui, p);
        action_row(ui, p, cmds, Lead::Icon(icons::DOWNLOAD), t(T::LoadSubtitles), Action::LoadSubtitles, true);
        menu_separator(ui, p);
        match stepper(ui, p, icons::CLOCK, t(T::SubtitleDelay), &format_delay(self.sub_delay), self.sub_delay.abs() > 1e-3) {
            Step::Less => cmds.push(Cmd::RunKeep(Action::SubtitleDelayEarlier)),
            Step::More => cmds.push(Cmd::RunKeep(Action::SubtitleDelayLater)),
            Step::Reset => cmds.push(Cmd::ResetSubtitleDelay),
            Step::None => {}
        }
    }

    fn page_audio(&mut self, ui: &mut Ui, p: &Palette, cmds: &mut Vec<Cmd>) {
        back_row(ui, p, cmds, t(T::AudioTrack));
        let media = self.media.clone();
        match media.as_ref().filter(|m| !m.audio.is_empty()) {
            Some(m) => {
                let current = current_audio(m).map(|(_, a)| a.index);
                for (n, a) in m.audio.iter().enumerate() {
                    let label = audio_label(a, n);
                    if menu_item(ui, p, Lead::Radio(current == Some(a.index)), &ellipsize_middle(&label, 36), None, true).clicked() {
                        cmds.push(Cmd::Audio(a.index));
                    }
                }
            }
            None => {
                menu_item(ui, p, Lead::None, t(T::NoAudio), None, false);
            }
        }
        menu_separator(ui, p);
        match stepper(ui, p, icons::CLOCK, t(T::AudioDelay), &format_delay(self.audio_delay), self.audio_delay.abs() > 1e-3) {
            Step::Less => cmds.push(Cmd::RunKeep(Action::AudioDelayEarlier)),
            Step::More => cmds.push(Cmd::RunKeep(Action::AudioDelayLater)),
            Step::Reset => cmds.push(Cmd::ResetAudioDelay),
            Step::None => {}
        }
        menu_separator(ui, p);
        action_row(ui, p, cmds, Lead::Check(self.settings.night_mode), t(T::NightMode), Action::ToggleNightMode, true);
        action_row(ui, p, cmds, Lead::Icon(icons::EQUALIZER), t(T::Equalizer), Action::OpenEqualizer, true);
    }

    fn page_video(&mut self, ui: &mut Ui, p: &Palette, cmds: &mut Vec<Cmd>) {
        back_row(ui, p, cmds, t(T::Picture));
        let aspect_hint = self.aspect_label();
        drill(ui, p, cmds, icons::ASPECT_RATIO, t(T::AspectRatio), Some(aspect_hint), Page::Aspect);
        let zoom = format!("{} %", (self.view.zoom * 100.0).round() as i32);
        match stepper(ui, p, icons::ZOOM_IN, t(T::Zoom), &zoom, (self.view.zoom - 1.0).abs() > 1e-3) {
            Step::Less => cmds.push(Cmd::RunKeep(Action::ZoomOut)),
            Step::More => cmds.push(Cmd::RunKeep(Action::ZoomIn)),
            Step::Reset => cmds.push(Cmd::RunKeep(Action::ZoomReset)),
            Step::None => {}
        }
        action_row(ui, p, cmds, Lead::Icon(icons::ROTATE), t(T::RotateRight), Action::RotateRight, true);
        action_row(ui, p, cmds, Lead::Check(self.view.flip_h), t(T::FlipHorizontal), Action::FlipHorizontal, true);
        action_row(ui, p, cmds, Lead::Check(self.settings.ambient_mode), t(T::AmbientGlow), Action::ToggleAmbient, true);
        let deint_mode = match self.settings.deinterlace {
            lumen400_core::settings::Deinterlace::Auto => t(T::Automatic),
            lumen400_core::settings::Deinterlace::Off => t(T::DeinterlaceOff),
            lumen400_core::settings::Deinterlace::On => t(T::DeinterlaceOn),
        };
        action_row(ui, p, cmds, Lead::Icon(icons::TELEVISION), &t1(T::DeinterlaceValue, deint_mode), Action::CycleDeinterlace, true);
        menu_separator(ui, p);
        drill(ui, p, cmds, icons::SLIDERS, t(T::PictureAdjustments), None, Page::Picture);
        let dirty = !self.view.is_neutral() || !self.view.adjust.is_identity();
        action_row(ui, p, cmds, Lead::Icon(icons::ARROW_CCW), t(T::ResetPicture), Action::ResetPicture, dirty);
    }

    fn page_aspect(&mut self, ui: &mut Ui, p: &Palette, cmds: &mut Vec<Cmd>) {
        back_row(ui, p, cmds, t(T::AspectRatio));
        let fit = self.view.fit;
        let aspect = self.view.aspect;
        if menu_item(ui, p, Lead::Radio(fit == FitMode::Fit && aspect.is_none()), t(T::AspectAuto), Some(&Action::CycleAspect.shortcuts_label()), true).clicked() {
            cmds.push(Cmd::Aspect(None));
        }
        for (label, ratio) in ASPECTS {
            let selected = fit == FitMode::Fit && aspect.is_some_and(|a| (a - ratio).abs() < 1e-3);
            if menu_item(ui, p, Lead::Radio(selected), label, None, true).clicked() {
                cmds.push(Cmd::Aspect(Some(ratio)));
            }
        }
        menu_separator(ui, p);
        for (mode, key) in [(FitMode::Fill, T::AspectFill), (FitMode::Stretch, T::AspectStretch), (FitMode::Original, T::AspectOriginal)] {
            if menu_item(ui, p, Lead::Radio(fit == mode), t(key), None, true).clicked() {
                cmds.push(Cmd::Fit(mode));
            }
        }
    }

    fn page_picture(&mut self, ui: &mut Ui, p: &Palette, cmds: &mut Vec<Cmd>) {
        back_row(ui, p, cmds, t(T::PictureAdjustments));
        let a = &mut self.view.adjust;
        ui.add_space(4.0);
        adjust_row(ui, p, t(T::Brightness), &mut a.brightness, -0.5..=0.5, 0.0, |v| format!("{:+}", (v * 100.0).round() as i32));
        adjust_row(ui, p, t(T::Contrast), &mut a.contrast, 0.5..=1.5, 1.0, |v| format!("{} %", (v * 100.0).round() as i32));
        adjust_row(ui, p, t(T::Saturation), &mut a.saturation, 0.0..=2.0, 1.0, |v| format!("{} %", (v * 100.0).round() as i32));
        adjust_row(ui, p, t(T::Gamma), &mut a.gamma, 0.5..=2.0, 1.0, |v| format!("{v:.2}"));
        ui.add_space(4.0);
    }

    fn page_speed(&mut self, ui: &mut Ui, p: &Palette, cmds: &mut Vec<Cmd>) {
        back_row(ui, p, cmds, t(T::Speed));
        let cur = self.snap.speed;
        for s in SPEEDS {
            let label = if (s - 1.0).abs() < 1e-9 { t(T::SpeedNormal).to_owned() } else { format!("×{}", format_speed(s)) };
            if menu_item(ui, p, Lead::Radio((cur - s).abs() < 1e-3), &label, None, true).clicked() {
                cmds.push(Cmd::Speed(s));
            }
        }
    }

    fn page_tools(&mut self, ui: &mut Ui, p: &Palette, cmds: &mut Vec<Cmd>) {
        back_row(ui, p, cmds, t(T::Tools));
        let has_media = self.source.is_some();
        action_row(ui, p, cmds, Lead::Icon(icons::INFO), t(T::MediaInfo), Action::ToggleInfo, has_media);
        action_row(ui, p, cmds, Lead::Icon(icons::CHART_BAR), t(T::Statistics), Action::ToggleStats, has_media);
        menu_separator(ui, p);
        action_row(ui, p, cmds, Lead::Icon(icons::COMMAND), t(T::CommandPalette), Action::OpenPalette, true);
        action_row(ui, p, cmds, Lead::Icon(icons::KEYBOARD), t(T::Shortcuts), Action::ShowShortcuts, true);
        menu_separator(ui, p);
        action_row(ui, p, cmds, Lead::Check(self.always_on_top), t(T::AlwaysOnTop), Action::ToggleAlwaysOnTop, true);
        action_row(ui, p, cmds, Lead::Check(self.settings.show_remaining_time), t(T::ShowRemaining), Action::ToggleRemaining, true);
    }

    /// Name of the current aspect-ratio / fit mode.
    fn aspect_label(&self) -> String {
        match (self.view.fit, self.view.aspect) {
            (FitMode::Fill, _) => t(T::AspectFill).to_owned(),
            (FitMode::Stretch, _) => t(T::AspectStretch).to_owned(),
            (FitMode::Original, _) => t(T::AspectOriginal).to_owned(),
            (FitMode::Fit, None) => t(T::AspectAuto).to_owned(),
            (FitMode::Fit, Some(a)) => ASPECTS.iter().find(|(_, r)| (r - a).abs() < 1e-3).map_or_else(|| format!("{a:.2}"), |(l, _)| (*l).to_owned()),
        }
    }
}

// =================================================================================================
// Labels
// =================================================================================================

/// `(index in media.audio, stream)` of the audio track being played.
fn current_audio(m: &MediaInfo) -> Option<(usize, &AudioStreamInfo)> {
    let selected = m.selected_audio?;
    m.audio.iter().enumerate().find(|(_, a)| a.index == selected)
}

fn language_name(code: &str) -> Option<String> {
    let c = code.trim();
    if c.is_empty() || c.eq_ignore_ascii_case("und") {
        return None;
    }
    Some(lang::display_name(c, i18n::current()).map_or_else(|| c.to_uppercase(), str::to_owned))
}

/// The short name of a track: its language and title, or `fallback` when it has neither.
fn track_name(language: Option<&str>, title: Option<&str>, fallback: impl FnOnce() -> String) -> String {
    let lang = language.and_then(language_name);
    let title = title.map(str::trim).filter(|s| !s.is_empty());
    match (&lang, title) {
        (Some(l), Some(t)) if !t.eq_ignore_ascii_case(l) && !t.to_lowercase().contains(&l.to_lowercase()) => format!("{l} ({t})"),
        (Some(l), _) => l.clone(),
        (None, Some(t)) => t.to_owned(),
        (None, None) => fallback(),
    }
}

/// The short name of an audio track (language, else title): enough for a one-line summary.
pub fn audio_name(a: &AudioStreamInfo, n: usize) -> String {
    track_name(a.language.as_deref(), a.title.as_deref(), || t1(T::AudioTrackN, n + 1))
}

/// The name of an audio track: language, title, codec and layout.
pub fn audio_label(a: &AudioStreamInfo, n: usize) -> String {
    let mut name = audio_name(a, n);
    let tech = [codec_display(&a.codec), channels_label(a.channels)].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ");
    if !tech.is_empty() {
        name.push_str(" · ");
        name.push_str(&tech);
    }
    name
}

/// The short name of a subtitle track (language, else title).
pub fn subtitle_name(s: &SubtitleStreamInfo, n: usize) -> String {
    track_name(s.language.as_deref(), s.title.as_deref(), || t1(T::SubtitleTrackN, n + 1))
}

/// The name of a subtitle track.
pub fn subtitle_label(s: &SubtitleStreamInfo, n: usize) -> String {
    let mut name = subtitle_name(s, n);
    let mut notes: Vec<String> = Vec::new();
    if s.is_forced {
        notes.push(t(T::SubtitleForced).to_owned());
    }
    if !s.is_text {
        notes.push(t(T::SubtitleBitmap).to_owned());
    }
    if !notes.is_empty() {
        name.push_str(&format!(" ({})", notes.join(", ")));
    }
    name
}

// =================================================================================================
// Rows
// =================================================================================================

/// A row that runs an [`Action`] and shows its shortcut.
fn action_row(ui: &mut Ui, p: &Palette, cmds: &mut Vec<Cmd>, lead: Lead, label: &str, action: Action, enabled: bool) {
    let shortcut = action.shortcut_label();
    if menu_item(ui, p, lead, label, shortcut.as_deref(), enabled).clicked() {
        cmds.push(Cmd::Run(action));
    }
}

/// A row that opens a page, with the current value on the right.
fn drill(ui: &mut Ui, p: &Palette, cmds: &mut Vec<Cmd>, icon: char, label: &str, hint: Option<String>, page: Page) {
    let resp = menu_item(ui, p, Lead::Icon(icon), label, None, true);
    let right = resp.rect.right() - 10.0;
    ui.painter().text(pos2(right, resp.rect.center().y), Align2::RIGHT_CENTER, icons::CARET_RIGHT.to_string(), FontId::new(14.0, fonts::icons()), p.text_faint);
    if let Some(h) = hint.filter(|h| !h.is_empty()) {
        let h = ellipsize_middle(&h, 22);
        ui.painter().text(pos2(right - 20.0, resp.rect.center().y), Align2::RIGHT_CENTER, h, FontId::new(12.5, fonts::medium()), p.text_faint);
    }
    if resp.clicked() {
        cmds.push(Cmd::Push(page));
    }
}

/// The first row of a page: "‹ Title".
fn back_row(ui: &mut Ui, p: &Palette, cmds: &mut Vec<Cmd>, title: &str) {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::click());
    let hover = ui.ctx().animate_bool_with_time(resp.id.with("h"), resp.hovered(), 0.08);
    if hover > 0.0 {
        ui.painter().rect_filled(rect, CornerRadius::same(8), fade(p.hover, hover));
    }
    ui.painter().text(
        pos2(rect.left() + 18.0, rect.center().y),
        Align2::CENTER_CENTER,
        icons::CARET_LEFT.to_string(),
        FontId::new(15.0, fonts::icons_bold()),
        mix(p.text_dim, p.text, hover),
    );
    ui.painter().text(pos2(rect.left() + 38.0, rect.center().y), Align2::LEFT_CENTER, title, FontId::new(14.5, fonts::semibold()), p.text);
    if resp.clicked() {
        cmds.push(Cmd::Pop);
    }
    ui.add_space(2.0);
    let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(line.shrink2(vec2(8.0, 0.0)), 0.0, p.border);
    ui.add_space(4.0);
}

enum Step {
    None,
    Less,
    More,
    Reset,
}

/// "label  [−] value [+]": clicking the value resets it.
fn stepper(ui: &mut Ui, p: &Palette, icon: char, label: &str, value: &str, can_reset: bool) -> Step {
    let height = 36.0;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let mut out = Step::None;
    ui.painter().text(pos2(rect.left() + 18.0, rect.center().y), Align2::CENTER_CENTER, icon.to_string(), FontId::new(17.0, fonts::icons()), p.text_dim);
    ui.painter().text(pos2(rect.left() + 38.0, rect.center().y), Align2::LEFT_CENTER, label, FontId::new(14.0, fonts::medium()), p.text);

    let btn = 28.0;
    let value_w = 66.0;
    let more = Rect::from_center_size(pos2(rect.right() - 8.0 - btn * 0.5, rect.center().y), vec2(btn, btn));
    let val = Rect::from_center_size(pos2(more.left() - value_w * 0.5 - 2.0, rect.center().y), vec2(value_w, btn));
    let less = Rect::from_center_size(pos2(val.left() - 2.0 - btn * 0.5, rect.center().y), vec2(btn, btn));
    for (r, glyph, step) in [(less, icons::MINUS, Step::Less), (more, icons::PLUS, Step::More)] {
        let resp = ui.interact(r, Id::unique(("stepper", label, glyph as u32)), Sense::click());
        let h = ui.ctx().animate_bool_with_time(resp.id.with("h"), resp.hovered(), 0.08);
        ui.painter().rect_filled(r, CornerRadius::same(8), mix(p.raised, p.hover, h));
        ui.painter().text(r.center(), Align2::CENTER_CENTER, glyph.to_string(), FontId::new(14.0, fonts::icons_bold()), mix(p.text_dim, p.text, h));
        if resp.clicked() {
            out = step;
        }
    }
    let vresp = ui.interact(val, Id::unique(("stepper-value", label)), if can_reset { Sense::click() } else { Sense::hover() });
    let color = if can_reset { p.accent } else { p.text_dim };
    ui.painter().text(val.center(), Align2::CENTER_CENTER, value, FontId::new(12.5, fonts::mono()), color);
    if can_reset {
        let vresp = vresp.on_hover_ui(|ui| crate::widgets::tooltip(ui, p, t(T::Reset), None));
        if vresp.clicked() {
            out = Step::Reset;
        }
    }
    out
}

/// A labelled slider of the picture page: label and value on top, the slider below.
fn adjust_row(ui: &mut Ui, p: &Palette, label: &str, value: &mut f32, range: std::ops::RangeInclusive<f32>, neutral: f32, show: impl Fn(f32) -> String) {
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.label(egui::RichText::new(label).font(FontId::new(13.5, fonts::medium())).color(p.text));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(12.0);
            let changed = (*value - neutral).abs() > 1e-3;
            let (rect, resp) = ui.allocate_exact_size(vec2(56.0, 20.0), if changed { Sense::click() } else { Sense::hover() });
            ui.painter().text(rect.right_center(), Align2::RIGHT_CENTER, show(*value), FontId::new(12.5, fonts::mono()), if changed { p.accent } else { p.text_faint });
            if changed && resp.on_hover_ui(|ui| crate::widgets::tooltip(ui, p, t(T::Reset), None)).clicked() {
                *value = neutral;
            }
        });
    });
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        let len = ui.available_width() - 8.0;
        let marks = [neutral];
        ThinSlider::new(value, range).length(len).marks(&marks).detent(neutral).origin(neutral).step(0.01).show(ui, p);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen400_core::settings::Language;

    fn audio(lang: Option<&str>, title: Option<&str>, codec: &str, channels: u16) -> AudioStreamInfo {
        AudioStreamInfo { index: 1, codec: codec.into(), channels, language: lang.map(str::to_owned), title: title.map(str::to_owned), ..Default::default() }
    }

    #[test]
    fn audio_labels() {
        let _guard = crate::i18n::test_lock();
        i18n::set_language(Language::English);
        assert_eq!(audio_label(&audio(Some("eng"), None, "ac3", 6), 0), "English · AC-3 5.1");
        assert_eq!(audio_label(&audio(Some("fra"), Some("Commentary"), "aac", 2), 1), "French (Commentary) · AAC stereo");
        assert_eq!(audio_label(&audio(None, None, "opus", 2), 2), "Track 3 · Opus stereo");
        assert_eq!(audio_label(&audio(Some("und"), Some("Main"), "flac", 1), 0), "Main · FLAC mono");
        i18n::set_language(Language::French);
        assert_eq!(audio_label(&audio(Some("eng"), None, "ac3", 2), 0), "Anglais · AC-3 stéréo");
    }

    #[test]
    fn subtitle_labels() {
        let _guard = crate::i18n::test_lock();
        i18n::set_language(Language::English);
        let s = SubtitleStreamInfo { index: 3, codec: "subrip".into(), is_text: true, language: Some("fra".into()), title: None, is_default: false, is_forced: true };
        assert_eq!(subtitle_label(&s, 0), "French (forced)");
        let bitmap = SubtitleStreamInfo { index: 4, codec: "hdmv_pgs_subtitle".into(), is_text: false, language: Some("eng".into()), ..Default::default() };
        assert_eq!(subtitle_label(&bitmap, 1), "English (bitmap)");
        let anon = SubtitleStreamInfo { index: 5, is_text: true, ..Default::default() };
        assert_eq!(subtitle_label(&anon, 2), "Subtitles 3");
    }
}

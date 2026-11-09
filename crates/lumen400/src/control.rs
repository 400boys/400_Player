//! The behaviour of the player: opening media, the playlist, seeking, tracks, subtitles, the
//! picture, the window, history. The interface only calls these methods; nothing here paints.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::ViewportCommand;
use lumen400_core::media_kind::{PathKind, classify, file_url_to_path, find_sibling_subtitles, is_file_url, is_url, scan_folder, subtitle_language_hint};
use lumen400_core::playlist::{ImportedEntry, ItemId, RepeatMode, read_playlist_file};
use lumen400_core::settings::Settings;
use lumen400_core::subtitle::{Cue, SubtitleTrack};
use lumen400_core::time::{format_clock, parse_timecode};
use lumen400_engine::{EngineConfig, Event, LangPrefs, Notice, PlayState, SeekMode, StreamSelection};

use crate::actions::{AUDIO_DELAY_STEP, Action, SUBTITLE_DELAY_STEP};
use crate::app::LumenApp;
use crate::files::{self, Answer};
use crate::i18n::{Key as T, t, t1, tf};
use crate::state::{AbLoop, Dialog, ExternalSub, Osd, PaletteState, SettingsTab, SubSource, Toast, ToastAction, ToastKind};
use crate::ui::icons;
use crate::util::{file_name_of, format_delay, format_speed};
use crate::video::{FitMode, display_size, total_rotation};

/// Playback speeds offered by the shortcuts and the speed menu.
pub const SPEEDS: [f64; 10] = [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0, 3.0, 4.0];

/// Forced aspect ratios offered by the aspect menu: label and width / height.
pub const ASPECTS: [(&str, f32); 6] = [("16:9", 16.0 / 9.0), ("4:3", 4.0 / 3.0), ("21:9", 21.0 / 9.0), ("1:1", 1.0), ("5:4", 5.0 / 4.0), ("16:10", 1.6)];

/// How long the controls stay after the last sign of life, besides the user's own delay.
const PROGRESS_SAVE_INTERVAL: f64 = 10.0;

/// Engine configuration derived from the user's settings.
pub fn engine_config(s: &Settings, allow_16bit: bool, virtual_audio: bool) -> EngineConfig {
    EngineConfig {
        hw_decode: s.hw_decode == lumen400_core::settings::HwDecode::Auto,
        audio_device: s.audio_device.clone(),
        force_virtual_audio: virtual_audio,
        allow_16bit_textures: allow_16bit,
        lang_prefs: LangPrefs::from_settings(&s.preferred_audio_langs, &s.preferred_sub_langs, s.auto_load_subtitles),
        generate_thumbnails: s.generate_thumbnails,
        ..EngineConfig::default()
    }
}

fn entry_of(source: String) -> ImportedEntry {
    ImportedEntry { source, title: None, duration: None }
}

fn path_entry(path: &Path) -> ImportedEntry {
    entry_of(path.to_string_lossy().into_owned())
}

impl LumenApp {
    // =============================================================================================
    // Messages
    // =============================================================================================

    /// The interface clock (seconds since the window opened).
    pub fn now(&self) -> f64 {
        self.ctx.input(|i| i.time)
    }

    pub fn toast(&mut self, kind: ToastKind, title: impl Into<String>, detail: Option<String>) {
        self.toast_with(kind, title, detail, None);
    }

    pub fn toast_with(&mut self, kind: ToastKind, title: impl Into<String>, detail: Option<String>, action: Option<ToastAction>) {
        let title = title.into();
        // The same message twice in a row is one message.
        if self.toasts.iter().any(|t| t.title == title && t.detail == detail) {
            return;
        }
        self.toast_seq += 1;
        let ttl = match kind {
            ToastKind::Error => 7.0,
            ToastKind::Warning => 5.5,
            _ => 3.5,
        };
        let born = self.now();
        self.toasts.push(Toast { id: self.toast_seq, kind, title, detail, born, ttl, action });
        if self.toasts.len() > 4 {
            self.toasts.remove(0);
        }
        self.ctx.request_repaint();
    }

    pub fn osd(&mut self, icon: char, text: impl Into<String>, progress: Option<f32>) {
        self.osd_with_tick(icon, text, progress, None);
    }

    pub fn osd_with_tick(&mut self, icon: char, text: impl Into<String>, progress: Option<f32>, tick: Option<f32>) {
        let born = self.now();
        self.osd = Some(Osd { icon, text: text.into(), progress, tick, born });
        self.ctx.request_repaint();
    }

    /// Any sign of life from the user keeps the controls on screen.
    pub fn wake_controls(&mut self) {
        self.activity_at = self.now();
    }

    // =============================================================================================
    // Settings
    // =============================================================================================

    /// Marks the settings as changed; they are written to disk shortly after.
    pub fn touch_settings(&mut self) {
        if self.settings_dirty_since.is_none() {
            self.settings_dirty_since = Some(self.now());
        }
    }

    pub fn save_settings_now(&mut self) {
        self.settings_dirty_since = None;
        if !self.ephemeral
            && let Err(e) = self.settings.save()
        {
            tracing::warn!("cannot save the settings: {e}");
        }
    }

    /// Re-applies theme, language and zoom after a settings change.
    pub fn apply_look(&mut self) {
        crate::i18n::set_language(self.settings.language);
        let system_dark = self.ctx.system_theme().is_none_or(|t| t == egui::Theme::Dark);
        self.pal = crate::theme::Palette::new(self.settings.theme, self.settings.accent, system_dark);
        crate::theme::apply(&self.ctx, &self.pal, self.settings.ui_scale);
        if let Some(rs) = &self.render_state
            && let Some(gpu) = rs.renderer.write().callback_resources.get_mut::<crate::video::VideoGpu>()
        {
            gpu.invalidate_ambient();
        }
    }

    /// Sends the audio-related settings to the engine.
    pub fn apply_audio(&mut self) {
        self.player.set_volume(self.settings.volume);
        self.player.set_muted(self.settings.muted);
        self.player.set_equalizer(&self.settings.eq);
        self.player.set_night_mode(self.settings.night_mode);
    }

    /// Tells the engine what to do about interlaced pictures. The engine applies it to the pictures it
    /// decodes from now on: a paused picture is decoded again so that the change shows at once.
    pub fn apply_deinterlace(&mut self) {
        self.player.set_deinterlace(self.settings.deinterlace);
        if self.snap.state == PlayState::Paused && self.snap.has_video {
            self.seek_to(self.snap.position, SeekMode::Accurate);
        }
    }

    /// Sends the engine-related settings to the engine.
    pub fn apply_engine_config(&mut self) {
        let cfg = engine_config(&self.settings, self.gpu.supports_16bit, self.virtual_audio);
        self.player.set_config(cfg);
    }

    // =============================================================================================
    // Opening media
    // =============================================================================================

    /// Opens what the user chose: files, folders, playlists, URLs, subtitle files. With `append`
    /// they are added to the queue; otherwise they replace it and the first one starts playing.
    pub fn open_inputs(&mut self, inputs: Vec<String>, append: bool) {
        let mut entries: Vec<ImportedEntry> = Vec::new();
        let mut subtitle: Option<PathBuf> = None;
        let mut missing: Vec<String> = Vec::new();
        let mut single_local: Option<PathBuf> = None;
        let count = inputs.len();
        for input in inputs {
            let input = input.trim().to_owned();
            if input.is_empty() {
                continue;
            }
            if is_url(&input) && !is_file_url(&input) {
                entries.push(entry_of(input));
                continue;
            }
            let path = if is_file_url(&input) { file_url_to_path(&input).unwrap_or_else(|| PathBuf::from(&input)) } else { PathBuf::from(&input) };
            if path.is_dir() {
                let found = scan_folder(&path, true);
                if found.is_empty() {
                    missing.push(file_name_of(&input));
                }
                entries.extend(found.iter().map(|p| path_entry(p)));
                continue;
            }
            if !path.exists() {
                missing.push(file_name_of(&input));
                continue;
            }
            match classify(&path) {
                PathKind::Subtitle => subtitle = Some(path),
                PathKind::Playlist => match read_playlist_file(&path) {
                    Ok(list) => entries.extend(list),
                    Err(e) => self.toast(ToastKind::Error, tf(T::CannotOpen, &[&file_name_of(&input)]), Some(e.to_string())),
                },
                // Anything else is handed to FFmpeg, which knows more formats than any list.
                _ => {
                    if count == 1 {
                        single_local = Some(path.clone());
                    }
                    entries.push(path_entry(&path));
                }
            }
        }
        for name in missing {
            self.toast(ToastKind::Warning, tf(T::UnsupportedFile, &[&name]), None);
        }
        if let Some(sub) = subtitle
            && entries.is_empty()
        {
            if self.media.is_some() {
                self.load_subtitle_file(&sub, false);
            } else {
                self.toast(ToastKind::Info, t(T::SubtitlesNeedMedia), None);
            }
            return;
        }
        if entries.is_empty() {
            return;
        }
        self.remember_dir(entries.first().map(|e| e.source.as_str()));

        if append {
            let was_empty = self.playlist.is_empty();
            let ids = self.playlist.extend(entries);
            self.toast(ToastKind::Success, tf(T::AddedToPlaylist, &[&ids.len()]), None);
            if (was_empty || self.snap.state == PlayState::Idle)
                && let Some(&first) = ids.first()
            {
                self.play_item(first);
            }
            return;
        }

        self.playlist.clear();
        let mut start_id: Option<ItemId> = None;
        if let (Some(file), true) = (single_local.as_ref(), self.settings.queue_folder_siblings) {
            // A lone file: queue its folder so Next / Previous walk through the season.
            let siblings = file.parent().map(|d| scan_folder(d, false)).unwrap_or_default();
            if siblings.len() > 1 && siblings.iter().any(|p| p == file) {
                let target = file.to_string_lossy().into_owned();
                for p in &siblings {
                    let id = self.playlist.add_path(p);
                    if p.to_string_lossy() == target {
                        start_id = Some(id);
                    }
                }
            }
        }
        if start_id.is_none() {
            let ids = self.playlist.extend(entries);
            start_id = ids.first().copied();
        }
        if let Some(id) = start_id {
            self.play_item(id);
        }
    }

    fn remember_dir(&mut self, source: Option<&str>) {
        let Some(source) = source else { return };
        if is_url(source) && !is_file_url(source) {
            return;
        }
        if let Some(dir) = Path::new(source).parent().and_then(|d| d.to_str())
            && self.settings.last_dir.as_deref() != Some(dir)
        {
            self.settings.last_dir = Some(dir.to_owned());
            self.touch_settings();
        }
    }

    /// Plays a playlist item (and makes it the current one).
    pub fn play_item(&mut self, id: ItemId) {
        let Some(item) = self.playlist.get(id).cloned() else { return };
        self.playlist.set_current(id);
        self.play_source(&item.source, None);
    }

    /// Opens `source` right now. `start` overrides the remembered resume position.
    pub fn play_source(&mut self, source: &str, start: Option<f64>) {
        let cli_start = self.pending_start.take();
        self.record_history(false);
        self.reset_media_state();
        let start = start.or(cli_start).or_else(|| if self.settings.resume_playback { self.history.resume_position(source) } else { None });
        self.source = Some(source.to_owned());
        self.opening_since = Some(self.now());
        self.pending_start = start;
        let paused = std::mem::take(&mut self.boot_paused);
        self.player.open_with(source, start, paused, StreamSelection::default());
        self.selection = StreamSelection::default();
        if let Some(s) = start {
            self.osd(icons::CLOCK, tf(T::ResumeAt, &[&format_clock(s)]), None);
        }
        self.wake_controls();
        self.update_title();
    }

    /// Forgets everything that belongs to the previous media.
    fn reset_media_state(&mut self) {
        self.media = None;
        self.cover = None;
        self.cover_image = None;
        self.ext_sub = None;
        self.sub_source = SubSource::Off;
        self.sub_delay = 0.0;
        self.audio_delay = 0.0;
        self.player.set_audio_delay(0.0);
        self.ab = AbLoop::Off;
        self.view.reset_geometry();
        self.seek_drag = None;
        self.ended_handled = false;
        self.pending_start = None;
        self.thumbs.clear();
        self.sub_bitmaps.clear();
        self.spectrum = Default::default();
    }

    pub fn next_item(&mut self) {
        match self.playlist.go_next() {
            Some(id) => self.play_item(id),
            None => {
                if self.playlist.len() > 1 {
                    self.osd(icons::SKIP_FORWARD, t(T::PlaylistEnded), None);
                }
            }
        }
    }

    pub fn previous_item(&mut self) {
        // Like every player: a long way into the track, "previous" restarts it.
        if self.snap.position > 3.0 || self.playlist.len() < 2 {
            self.seek_to(0.0, SeekMode::Accurate);
            return;
        }
        match self.playlist.go_previous() {
            Some(id) => self.play_item(id),
            None => self.seek_to(0.0, SeekMode::Accurate),
        }
    }

    pub fn stop(&mut self) {
        self.record_history(false);
        self.player.stop();
        self.reset_media_state();
        self.source = None;
        self.frame = None;
        self.opening_since = None;
        self.update_title();
    }

    fn on_ended(&mut self) {
        if self.ended_handled {
            return;
        }
        self.ended_handled = true;
        self.record_history(true);
        self.failures = 0;
        match self.playlist.advance(true) {
            Some(id) if Some(id) == self.current_playing_item() => {
                // Repeat one: restart in place, no need to reopen the file.
                self.ended_handled = false;
                self.player.play();
            }
            Some(id) => self.play_item(id),
            None => {
                if self.playlist.len() > 1 {
                    self.toast(ToastKind::Info, t(T::PlaylistEnded), None);
                }
            }
        }
    }

    /// The playlist item whose source is playing.
    pub fn current_playing_item(&self) -> Option<ItemId> {
        let src = self.source.as_deref()?;
        self.playlist.current().filter(|i| i.source == src).map(|i| i.id)
    }

    // =============================================================================================
    // Engine events
    // =============================================================================================

    /// Drains the engine: events, snapshot, the frame to show. Runs once per frame, before the UI.
    pub fn pump(&mut self) {
        let now = self.now();
        for ev in self.player.poll_events() {
            self.on_event(ev, now);
        }
        self.snap = self.player.snapshot();
        let state = self.snap.state;
        if let Some(f) = self.player.poll_frame(self.lookahead) {
            self.frame = Some(f);
        }
        if state == PlayState::Idle || state == PlayState::Failed {
            // Keep the last picture while a new media opens, never after a stop or a failure.
            if self.opening_since.is_none() {
                self.frame = None;
            }
        }
        if state != PlayState::Opening && state != PlayState::Idle {
            self.opening_since = None;
        }

        // The screen stays on while a video plays (never in tests and demos).
        let need =
            if self.settings.prevent_sleep && !self.ephemeral { crate::power::Need::of(state == PlayState::Playing, self.snap.has_video) } else { crate::power::Need::Nothing };
        self.awake.set(need);
        self.pump_desktop();

        // A–B loop.
        if let (AbLoop::Active(a, b), PlayState::Playing) = (self.ab, state)
            && self.snap.position >= b
            && now >= self.ab_cooldown_until
        {
            self.ab_cooldown_until = now + 0.6;
            self.player.seek(a, SeekMode::Accurate);
        }

        // Remember where we are, a few times a minute.
        if state == PlayState::Playing && now - self.progress_saved_at >= PROGRESS_SAVE_INTERVAL {
            self.progress_saved_at = now;
            self.record_history(false);
        }
        if self.history_dirty && now - self.history_saved_at >= 15.0 {
            self.save_history_now();
        }
        if let Some(since) = self.settings_dirty_since
            && now - since >= 1.0
        {
            self.save_settings_now();
        }

        // The spectrum only costs something when it is visible.
        let want_spectrum = self.media.as_ref().is_some_and(|m| !m.has_video()) && matches!(state, PlayState::Playing | PlayState::Paused);
        if want_spectrum != self.spectrum_on {
            self.spectrum_on = want_spectrum;
            self.player.set_spectrum_enabled(want_spectrum);
        }
        if want_spectrum {
            let dt = self.ctx.input(|i| i.unstable_dt).clamp(0.001, 0.1);
            if let Some(s) = self.player.spectrum(dt) {
                self.spectrum = s;
            }
        }
    }

    fn on_event(&mut self, ev: Event, now: f64) {
        match ev {
            Event::Opened(info) => {
                let new_media = self.media.as_ref().is_none_or(|m| m.source != info.source);
                if new_media {
                    self.on_new_media(&info);
                }
                if !info.has_video() {
                    self.frame = None;
                }
                self.media = Some(info);
                self.update_title();
            }
            Event::Cover(img) => {
                let image = egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.data);
                self.cover = Some(self.ctx.load_texture("cover", image, egui::TextureOptions::LINEAR));
                self.cover_image = Some(img);
            }
            Event::State(s) => {
                if s == PlayState::Failed {
                    self.frame = None;
                }
                if matches!(s, PlayState::Playing | PlayState::Paused) {
                    self.failures = 0;
                }
            }
            Event::Ended => self.on_ended(),
            Event::Error(detail) => {
                let name = self.source.as_deref().map(file_name_of).unwrap_or_default();
                self.toast(ToastKind::Error, tf(T::CannotOpen, &[&name]), Some(detail));
                self.failures += 1;
                // In a playlist, a broken file must not stop the show — but never loop forever.
                if self.playlist.len() > 1
                    && (self.failures as usize) < self.playlist.len()
                    && let Some(id) = self.playlist.go_next()
                {
                    self.play_item(id);
                }
            }
            Event::Warning(n) => self.on_notice(n),
            Event::AudioDevice { name, is_virtual } => {
                tracing::info!("audio output: {name}{}", if is_virtual { " (virtual)" } else { "" });
            }
        }
        let _ = now;
    }

    fn on_notice(&mut self, n: Notice) {
        match n {
            Notice::NoAudioOutput => self.toast(ToastKind::Warning, t(T::NoSoundCard), None),
            Notice::AudioDeviceLost => self.toast(ToastKind::Warning, t(T::AudioDeviceLost), None),
            Notice::NotSeekable => self.osd(icons::LINK, t(T::NotSeekable), None),
            Notice::HardwareDecodeFailed => self.toast(ToastKind::Warning, t(T::HwDecodeFailed), None),
            Notice::StreamSkipped(why) => tracing::warn!("stream skipped: {why}"),
            Notice::Other(msg) => tracing::debug!("engine: {msg}"),
        }
    }

    /// First `Opened` event of a media.
    fn on_new_media(&mut self, info: &Arc<lumen400_core::types::MediaInfo>) {
        if let Some(id) = self.current_playing_item() {
            if let Some(d) = info.duration {
                self.playlist.set_duration(id, d);
            }
            if let Some(title) = info.display_title()
                && self.playlist.get(id).is_some_and(|i| i.title.is_none())
            {
                self.playlist.set_title(id, Some(title));
            }
        }
        self.sub_source = match info.selected_subtitle {
            Some(i) => SubSource::Embedded(i),
            None => SubSource::Off,
        };
        self.selection = StreamSelection::default();
        if info.selected_subtitle.is_none() && self.settings.auto_load_subtitles {
            self.auto_load_sibling_subtitles();
        }
        // The entry exists from the first second, so "recent" is never behind.
        let title = info.display_title();
        if let Some(src) = self.source.clone() {
            let pos = self.pending_start.unwrap_or(0.0);
            self.history.record(&src, title.as_deref(), pos, info.duration.unwrap_or(0.0));
            self.history_dirty = true;
        }
    }

    fn auto_load_sibling_subtitles(&mut self) {
        let Some(src) = self.source.clone() else { return };
        let path = if is_file_url(&src) {
            file_url_to_path(&src)
        } else if is_url(&src) {
            None
        } else {
            Some(PathBuf::from(&src))
        };
        let Some(path) = path else { return };
        let candidates = find_sibling_subtitles(&path);
        if candidates.is_empty() {
            return;
        }
        let prefs: Vec<String> = self.settings.preferred_sub_langs.split([',', ';', ' ']).map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect();
        let rank = |p: &PathBuf| -> usize {
            let hint = subtitle_language_hint(p);
            hint.and_then(|h| lumen400_core::lang::preference_rank(&prefs, &h)).unwrap_or(usize::MAX - 1)
        };
        let best = if prefs.is_empty() { candidates.first() } else { candidates.iter().min_by_key(|p| rank(p)) };
        if let Some(p) = best.cloned() {
            self.load_subtitle_file(&p, true);
        }
    }

    // =============================================================================================
    // History
    // =============================================================================================

    /// Writes the current position into the history (`finished` resets it).
    pub fn record_history(&mut self, finished: bool) {
        let Some(src) = self.source.clone() else { return };
        if !matches!(self.snap.state, PlayState::Playing | PlayState::Paused | PlayState::Ended | PlayState::Buffering) {
            return;
        }
        let duration = self.snap.duration.unwrap_or(0.0);
        let pos = if finished || duration <= 0.0 { 0.0 } else { self.snap.position };
        let title = self.media.as_ref().and_then(|m| m.display_title());
        self.history.record(&src, title.as_deref(), pos, duration);
        self.history_dirty = true;
    }

    pub fn save_history_now(&mut self) {
        self.history_dirty = false;
        self.history_saved_at = self.now();
        if !self.ephemeral
            && let Err(e) = self.history.save()
        {
            tracing::warn!("cannot save the history: {e}");
        }
    }

    // =============================================================================================
    // Titles
    // =============================================================================================

    /// What to call the current media.
    pub fn display_title(&self) -> String {
        let name = self
            .media
            .as_ref()
            .and_then(|m| m.display_title())
            .or_else(|| self.current_playing_item().and_then(|id| self.playlist.get(id)).map(|i| i.display_name()))
            .or_else(|| self.source.as_deref().map(file_name_of))
            .unwrap_or_else(|| t(T::Untitled).to_owned());
        match self.media.as_ref().and_then(|m| m.artist().map(str::to_owned)) {
            Some(artist) if self.media.as_ref().is_some_and(|m| !m.has_video()) => format!("{artist} — {name}"),
            _ => name,
        }
    }

    pub fn update_title(&mut self) {
        let title = if self.source.is_some() { format!("{} — {}", self.display_title(), t(T::AppName)) } else { t(T::AppName).to_owned() };
        if title != self.window_title {
            self.ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }
    }

    // =============================================================================================
    // Transport
    // =============================================================================================

    pub fn is_playing(&self) -> bool {
        matches!(self.snap.state, PlayState::Playing)
    }

    pub fn has_media(&self) -> bool {
        self.source.is_some() && !matches!(self.snap.state, PlayState::Idle | PlayState::Failed) || matches!(self.snap.state, PlayState::Opening)
    }

    pub fn toggle_play(&mut self) {
        match self.snap.state {
            PlayState::Idle | PlayState::Failed => {
                // Nothing loaded: resume the queue, else offer the file dialog.
                if let Some(item) = self.playlist.current().or_else(|| self.playlist.items().first()).cloned() {
                    self.play_item(item.id);
                } else {
                    self.run(Action::OpenFile);
                }
            }
            PlayState::Ended => self.player.play(),
            _ => {
                self.player.toggle_pause();
                let playing_next = !matches!(self.snap.state, PlayState::Playing);
                self.osd(if playing_next { icons::PLAY } else { icons::PAUSE }, if playing_next { t(T::Play) } else { t(T::Pause) }, None);
            }
        }
    }

    pub fn seek_to(&mut self, t_secs: f64, mode: SeekMode) {
        let mut target = if t_secs.is_finite() { t_secs.max(0.0) } else { 0.0 };
        if let Some(d) = self.snap.duration {
            target = target.min(d);
        }
        self.player.seek(target, mode);
    }

    pub fn seek_by(&mut self, delta: f64) {
        if !self.player.is_seekable() && self.snap.state != PlayState::Opening {
            self.osd(icons::LINK, t(T::NotSeekable), None);
            return;
        }
        self.player.seek_relative(delta);
        let sign = if delta >= 0.0 { "+" } else { "−" };
        let target = (self.snap.position + delta).max(0.0);
        let target = self.snap.duration.map_or(target, |d| target.min(d));
        let text = match self.snap.duration {
            Some(d) => format!("{sign}{} s  ·  {} / {}", format_secs(delta.abs()), format_clock(target), format_clock(d)),
            None => format!("{sign}{} s", format_secs(delta.abs())),
        };
        self.osd(if delta >= 0.0 { icons::FAST_FORWARD } else { icons::REWIND }, text, self.snap.duration.map(|d| (target / d.max(1e-6)) as f32));
    }

    /// Seek to `n × 10 %` of the duration (number keys).
    pub fn seek_percent(&mut self, n: u8) {
        if let Some(d) = self.snap.duration {
            let target = d * f64::from(n) / 10.0;
            self.seek_to(target, SeekMode::Accurate);
            self.osd(icons::GAUGE, format!("{} / {}", format_clock(target), format_clock(d)), Some(f32::from(n) / 10.0));
        }
    }

    pub fn frame_step(&mut self, forward: bool) {
        if !self.has_media() || !self.media.as_ref().is_some_and(|m| m.has_video()) {
            return;
        }
        if self.is_playing() {
            self.player.pause();
        }
        self.player.step_frame(forward);
        self.osd(if forward { icons::SKIP_FORWARD } else { icons::SKIP_BACK }, t(if forward { T::FrameNext } else { T::FramePrevious }), None);
    }

    pub fn set_volume(&mut self, v: f32, show_osd: bool) {
        let v = if v.is_finite() { v.clamp(0.0, 2.0) } else { 1.0 };
        self.settings.volume = (v * 100.0).round() / 100.0;
        if v > 0.0 && self.settings.muted {
            self.settings.muted = false;
        }
        self.player.set_volume(self.settings.volume);
        self.player.set_muted(self.settings.muted);
        self.touch_settings();
        if show_osd {
            let pct = (self.settings.volume * 100.0).round() as i32;
            let icon = volume_icon(self.settings.volume, self.settings.muted);
            self.osd_with_tick(icon, t1(T::VolumeValue, pct), Some(self.settings.volume / 2.0), Some(0.5));
        }
    }

    pub fn change_volume(&mut self, steps: f32) {
        let step = self.settings.volume_step;
        let v = self.settings.volume + step * steps;
        // Land exactly on 100 % when crossing it, so the neutral gain is easy to find again.
        let crossed = (self.settings.volume - 1.0) * (v - 1.0) < 0.0;
        self.set_volume(if crossed { 1.0 } else { v }, true);
    }

    /// Night mode: loud passages come down, quiet ones come up.
    pub fn toggle_night_mode(&mut self) {
        self.settings.night_mode = !self.settings.night_mode;
        self.player.set_night_mode(self.settings.night_mode);
        self.touch_settings();
        self.osd(icons::MOON, t(if self.settings.night_mode { T::NightModeOn } else { T::NightModeOff }), None);
    }

    pub fn toggle_mute(&mut self) {
        self.settings.muted = !self.settings.muted;
        self.player.set_muted(self.settings.muted);
        self.touch_settings();
        let icon = volume_icon(self.settings.volume, self.settings.muted);
        let text = if self.settings.muted { t(T::Muted).to_owned() } else { t1(T::VolumeValue, (self.settings.volume * 100.0).round() as i32) };
        self.osd_with_tick(icon, text, Some(if self.settings.muted { 0.0 } else { self.settings.volume / 2.0 }), Some(0.5));
    }

    pub fn set_speed(&mut self, s: f64) {
        let s = s.clamp(0.25, 4.0);
        self.player.set_speed(s);
        self.snap.speed = s;
        self.osd(icons::GAUGE, t1(T::SpeedValue, format_speed(s)), None);
    }

    pub fn step_speed(&mut self, dir: i32) {
        let cur = self.snap.speed.max(0.25);
        let idx = SPEEDS.iter().position(|&s| (s - cur).abs() < 1e-3);
        let next = match (idx, dir) {
            (Some(i), d) if d > 0 => SPEEDS.get(i + 1).copied(),
            (Some(i), _) => i.checked_sub(1).map(|j| SPEEDS[j]),
            (None, d) if d > 0 => SPEEDS.iter().copied().find(|&s| s > cur),
            (None, _) => SPEEDS.iter().copied().rev().find(|&s| s < cur),
        };
        if let Some(s) = next {
            self.set_speed(s);
        }
    }

    pub fn toggle_ab(&mut self) {
        let pos = self.snap.position;
        match self.ab {
            AbLoop::Off => {
                self.ab = AbLoop::A(pos);
                self.osd(icons::REPEAT, tf(T::ABLoopA, &[&format_clock(pos)]), None);
            }
            AbLoop::A(a) => {
                if pos > a + 0.25 {
                    self.ab = AbLoop::Active(a, pos);
                    self.osd(icons::REPEAT, tf(T::ABLoopOn, &[&format_clock(a), &format_clock(pos)]), None);
                } else {
                    self.ab = AbLoop::Off;
                    self.osd(icons::REPEAT, t(T::ABLoopOff), None);
                }
            }
            AbLoop::Active(..) => {
                self.ab = AbLoop::Off;
                self.osd(icons::REPEAT, t(T::ABLoopOff), None);
            }
        }
    }

    pub fn jump_chapter(&mut self, dir: i32) {
        let Some(media) = self.media.clone() else { return };
        if media.chapters.is_empty() {
            return;
        }
        let pos = self.snap.position;
        let cur = media.chapter_at(pos);
        let target = match (cur, dir) {
            (Some(i), d) if d > 0 => media.chapters.get(i + 1).map(|c| (i + 1, c)),
            (None, d) if d > 0 => media.chapters.first().map(|c| (0, c)),
            (Some(i), _) => {
                if pos - media.chapters[i].start > 3.0 {
                    Some((i, &media.chapters[i]))
                } else {
                    i.checked_sub(1).map(|j| (j, &media.chapters[j]))
                }
            }
            (None, _) => None,
        };
        if let Some((i, c)) = target {
            self.seek_to(c.start, SeekMode::Accurate);
            let name = if c.title.trim().is_empty() { tf(T::ChapterN, &[&(i + 1)]) } else { c.title.clone() };
            self.osd(icons::BOOKMARK, name, None);
        }
    }

    pub fn cycle_repeat(&mut self) {
        let mode = self.playlist.repeat().cycle();
        self.set_repeat(mode);
    }

    pub fn set_repeat(&mut self, mode: RepeatMode) {
        self.playlist.set_repeat(mode);
        self.settings.repeat = mode;
        self.touch_settings();
        let (icon, key) = match mode {
            RepeatMode::Off => (icons::REPEAT, T::RepeatOff),
            RepeatMode::All => (icons::REPEAT, T::RepeatAll),
            RepeatMode::One => (icons::REPEAT_ONCE, T::RepeatOne),
        };
        self.osd(icon, t(key), None);
    }

    pub fn toggle_shuffle(&mut self) {
        let on = !self.playlist.shuffle();
        self.playlist.set_shuffle(on);
        self.settings.shuffle = on;
        self.touch_settings();
        self.osd(icons::SHUFFLE, t(if on { T::ShuffleOn } else { T::ShuffleOff }), None);
    }

    // =============================================================================================
    // Tracks and subtitles
    // =============================================================================================

    pub fn select_audio(&mut self, stream: usize) {
        let Some(media) = self.media.clone() else { return };
        if media.selected_audio == Some(stream) {
            return;
        }
        self.selection.audio = Some(Some(stream));
        self.player.select_tracks(self.selection);
        let name = media.audio.iter().position(|a| a.index == stream).map_or_else(String::new, |n| crate::ui::menu::audio_label(&media.audio[n], n));
        self.osd(icons::HEADPHONES, t1(T::AudioTrackValue, name), None);
    }

    pub fn cycle_audio(&mut self) {
        let Some(media) = self.media.clone() else { return };
        if media.audio.len() < 2 {
            self.osd(icons::HEADPHONES, t(if media.audio.is_empty() { T::NoAudio } else { T::NoTracks }), None);
            return;
        }
        let cur = media.selected_audio.and_then(|s| media.audio.iter().position(|a| a.index == s)).unwrap_or(0);
        let next = &media.audio[(cur + 1) % media.audio.len()];
        self.select_audio(next.index);
    }

    /// Chooses what the subtitle layer shows.
    pub fn select_subtitles(&mut self, source: SubSource) {
        match source {
            SubSource::Off => {
                self.sub_source = SubSource::Off;
                self.osd(icons::SUBTITLES, t1(T::SubtitlesValue, t(T::SubtitlesOff)), None);
            }
            SubSource::External => {
                if self.ext_sub.is_some() {
                    self.sub_source = SubSource::External;
                    let name = self.ext_sub.as_ref().map(|s| s.name.clone()).unwrap_or_default();
                    self.osd(icons::SUBTITLES, t1(T::SubtitlesValue, name), None);
                }
            }
            SubSource::Embedded(stream) => {
                let Some(media) = self.media.clone() else { return };
                if media.selected_subtitle != Some(stream) {
                    // The engine decodes one embedded track at a time: switching restarts it.
                    self.selection.subtitle = Some(Some(stream));
                    self.player.select_tracks(self.selection);
                }
                self.sub_source = SubSource::Embedded(stream);
                let label = media.subtitles.iter().position(|s| s.index == stream).map_or_else(String::new, |n| crate::ui::menu::subtitle_label(&media.subtitles[n], n));
                self.osd(icons::SUBTITLES, t1(T::SubtitlesValue, label), None);
            }
        }
    }

    /// Next subtitle choice: off → each embedded track (text or bitmap) → the external file → off.
    pub fn cycle_subtitles(&mut self) {
        let mut choices: Vec<SubSource> = vec![SubSource::Off];
        if let Some(m) = &self.media {
            choices.extend(m.subtitles.iter().map(|s| SubSource::Embedded(s.index)));
        }
        if self.ext_sub.is_some() {
            choices.push(SubSource::External);
        }
        if choices.len() < 2 {
            self.osd(icons::SUBTITLES, t(T::NoSubtitles), None);
            return;
        }
        let cur = choices.iter().position(|c| *c == self.sub_source).unwrap_or(0);
        self.select_subtitles(choices[(cur + 1) % choices.len()]);
    }

    pub fn load_subtitle_file(&mut self, path: &Path, quiet: bool) {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                self.toast(ToastKind::Error, tf(T::SubtitleLoadFailed, &[&name]), Some(e.to_string()));
                return;
            }
        };
        let ext = path.extension().and_then(|e| e.to_str());
        let locale = sys_locale::get_locale();
        match SubtitleTrack::parse(&bytes, ext, locale.as_deref()) {
            Ok(track) if !track.is_empty() => {
                self.ext_sub = Some(ExternalSub { name: name.clone(), track });
                self.sub_source = SubSource::External;
                if !quiet {
                    self.toast(ToastKind::Success, tf(T::LoadedSubtitles, &[&name]), None);
                }
            }
            Ok(_) => self.toast(ToastKind::Error, tf(T::SubtitleLoadFailed, &[&name]), Some(t(T::SubtitleEmpty).to_owned())),
            Err(e) => self.toast(ToastKind::Error, tf(T::SubtitleLoadFailed, &[&name]), Some(e.to_string())),
        }
    }

    /// Cues to draw at the current position (the subtitle delay applied).
    pub fn active_cues(&self) -> Vec<Cue> {
        let video_offset = if self.snap.has_video { self.audio_delay } else { 0.0 };
        let t = self.snap.position + video_offset - self.sub_delay;
        match self.sub_source {
            SubSource::Off => Vec::new(),
            SubSource::External => self.ext_sub.as_ref().map(|s| s.track.active_at(t).into_iter().cloned().collect()).unwrap_or_default(),
            SubSource::Embedded(_) => {
                let track = self.player.embedded_subtitles();
                let guard = track.lock();
                guard.active_at(t).into_iter().cloned().collect()
            }
        }
    }

    pub fn shift_audio_delay(&mut self, by: f64) {
        self.audio_delay = ((self.audio_delay + by) * 1000.0).round() / 1000.0;
        self.audio_delay = self.audio_delay.clamp(-5.0, 5.0);
        self.player.set_audio_delay(self.audio_delay);
        self.osd(icons::HEADPHONES, t1(T::AudioDelayValue, format_delay(self.audio_delay)), None);
    }

    pub fn shift_subtitle_delay(&mut self, by: f64) {
        self.sub_delay = ((self.sub_delay + by) * 1000.0).round() / 1000.0;
        self.sub_delay = self.sub_delay.clamp(-30.0, 30.0);
        self.osd(icons::SUBTITLES, t1(T::SubtitleDelayValue, format_delay(self.sub_delay)), None);
    }

    // =============================================================================================
    // Picture
    // =============================================================================================

    pub fn cycle_aspect(&mut self) {
        let cur = self.view.aspect.and_then(|a| ASPECTS.iter().position(|(_, r)| (r - a).abs() < 1e-3));
        let next = match (self.view.aspect, cur) {
            (None, _) => Some(0),
            (Some(_), Some(i)) if i + 1 < ASPECTS.len() => Some(i + 1),
            _ => None,
        };
        self.set_aspect(next.map(|i| ASPECTS[i].1));
    }

    pub fn set_aspect(&mut self, aspect: Option<f32>) {
        self.view.aspect = aspect;
        let label = match aspect {
            None => t(T::AspectAuto).to_owned(),
            Some(a) => ASPECTS.iter().find(|(_, r)| (r - a).abs() < 1e-3).map_or_else(|| format!("{a:.2}"), |(l, _)| (*l).to_owned()),
        };
        self.osd(icons::ASPECT_RATIO, tf(T::AspectValue, &[&label]), None);
    }

    pub fn set_fit(&mut self, fit: FitMode) {
        self.view.fit = fit;
        if fit != FitMode::Original {
            self.view.pan = egui::Vec2::ZERO;
        }
        let key = match fit {
            FitMode::Fit => T::AspectAuto,
            FitMode::Fill => T::AspectFill,
            FitMode::Stretch => T::AspectStretch,
            FitMode::Original => T::AspectOriginal,
        };
        self.osd(icons::FRAME_CORNERS, t(key), None);
    }

    pub fn cycle_fit(&mut self) {
        let next = match self.view.fit {
            FitMode::Fit => FitMode::Fill,
            FitMode::Fill => FitMode::Stretch,
            FitMode::Stretch => FitMode::Original,
            FitMode::Original => FitMode::Fit,
        };
        self.set_fit(next);
    }

    pub fn rotate_right(&mut self) {
        self.view.rotation = (self.view.rotation + 90) % 360;
        self.osd(icons::ROTATE, format!("{}°", self.view.rotation), None);
    }

    pub fn flip_horizontal(&mut self) {
        self.view.flip_h = !self.view.flip_h;
        self.osd(icons::FLIP_H, t(T::FlipHorizontal), None);
    }

    pub fn zoom_by(&mut self, factor: f32) {
        self.view.zoom = (self.view.zoom * factor).clamp(0.25, 8.0);
        if (self.view.zoom - 1.0).abs() < 0.02 {
            self.view.zoom = 1.0;
            self.view.pan = egui::Vec2::ZERO;
        }
        self.osd(icons::ZOOM_IN, t1(T::ZoomValue, (self.view.zoom * 100.0).round() as i32), None);
    }

    pub fn reset_zoom(&mut self) {
        self.view.zoom = 1.0;
        self.view.pan = egui::Vec2::ZERO;
        self.osd(icons::ZOOM_IN, t1(T::ZoomValue, 100), None);
    }

    pub fn reset_picture(&mut self) {
        self.view.reset_geometry();
        self.view.adjust = Default::default();
        self.osd(icons::SLIDERS, t(T::ResetPicture), None);
    }

    pub fn toggle_ambient(&mut self) {
        self.settings.ambient_mode = !self.settings.ambient_mode;
        self.touch_settings();
        self.osd(icons::SPARKLE, t(if self.settings.ambient_mode { T::AmbientOn } else { T::AmbientOff }), None);
    }

    pub fn cycle_deinterlace(&mut self) {
        use lumen400_core::settings::Deinterlace;
        self.settings.deinterlace = self.settings.deinterlace.cycle();
        self.apply_deinterlace();
        self.touch_settings();
        let mode = match self.settings.deinterlace {
            Deinterlace::Auto => t(T::Automatic),
            Deinterlace::Off => t(T::DeinterlaceOff),
            Deinterlace::On => t(T::DeinterlaceOn),
        };
        self.osd(icons::TELEVISION, t1(T::DeinterlaceValue, mode), None);
    }

    /// Saves the picture on screen — as the renderer produces it, adjustments included — as PNG.
    pub fn screenshot(&mut self) {
        let (Some(frame), Some(rs)) = (self.frame.clone(), self.render_state.clone()) else {
            self.toast(ToastKind::Info, t(T::ScreenshotNoVideo), None);
            return;
        };
        let stream_rotation = self.media.as_ref().and_then(|m| m.primary_video().map(|v| v.rotation)).unwrap_or(0);
        let rotation = total_rotation(stream_rotation, self.view.rotation);
        let disp = display_size(frame.width, frame.height, frame.sar, rotation, self.view.aspect);
        let (w, h) = (disp.x.round().max(1.0) as u32, disp.y.round().max(1.0) as u32);
        let view = crate::video::gpu::FrameView::from_frame(&frame);
        let z = self.view.zoom.clamp(0.1, 16.0);
        let px = if self.stage_rect.width() > 1.0 { 2.0 * self.view.pan.x / self.stage_rect.width() } else { 0.0 };
        let py = if self.stage_rect.height() > 1.0 { -2.0 * self.view.pan.y / self.stage_rect.height() } else { 0.0 };
        let dst = [px - z, py - z, px + z, py + z];
        let params = crate::video::DrawParams { dst, rotation, flip_h: self.view.flip_h, adjust: self.view.adjust, ..Default::default() };
        match crate::video::gpu::render_to_rgba(&rs.device, &rs.queue, &view, &params, w, h) {
            Some(rgba) => {
                let stem = format!("{} {}", self.display_title(), format_clock(frame.pts).replace(':', "-"));
                let dir = self.settings.screenshot_directory();
                files::save_png(&self.dialogs, dir, stem, rgba, w, h);
                self.osd(icons::CAMERA, t(T::Screenshot), None);
            }
            None => self.toast(ToastKind::Error, tf(T::ScreenshotFailed, &[&"GPU"]), None),
        }
    }

    // =============================================================================================
    // Window
    // =============================================================================================

    pub fn toggle_fullscreen(&mut self) {
        self.set_fullscreen(!self.fullscreen);
    }

    pub fn set_fullscreen(&mut self, on: bool) {
        self.fullscreen = on;
        self.ctx.send_viewport_cmd(ViewportCommand::Fullscreen(on));
        self.wake_controls();
    }

    pub fn toggle_always_on_top(&mut self) {
        self.always_on_top = !self.always_on_top;
        let level = if self.always_on_top { egui::WindowLevel::AlwaysOnTop } else { egui::WindowLevel::Normal };
        self.ctx.send_viewport_cmd(ViewportCommand::WindowLevel(level));
        self.osd(icons::PUSH_PIN, format!("{} : {}", t(T::AlwaysOnTop), t(if self.always_on_top { T::On } else { T::Off })), None);
    }

    // =============================================================================================
    // Dialog answers
    // =============================================================================================

    pub fn handle_answers(&mut self) {
        for answer in self.dialogs.poll() {
            match answer {
                Answer::Files { paths, append } => {
                    let inputs = paths.iter().map(|p| p.to_string_lossy().into_owned()).collect();
                    self.open_inputs(inputs, append);
                }
                Answer::Folder { path, append } => self.open_inputs(vec![path.to_string_lossy().into_owned()], append),
                Answer::Subtitle(p) => self.load_subtitle_file(&p, false),
                Answer::LoadPlaylist(p) => self.open_inputs(vec![p.to_string_lossy().into_owned()], false),
                Answer::SavePlaylist(p) => self.save_playlist_to(&p),
                Answer::ScreenshotFolder(p) => {
                    self.settings.screenshot_dir = Some(p.to_string_lossy().into_owned());
                    self.touch_settings();
                }
                Answer::ScreenshotSaved(p) => {
                    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    self.toast_with(ToastKind::Success, tf(T::ScreenshotSaved, &[&name]), Some(t(T::ShowInFolder).to_owned()), Some(ToastAction::Reveal(p)));
                }
                Answer::ScreenshotFailed(e) => self.toast(ToastKind::Error, tf(T::ScreenshotFailed, &[&e]), None),
            }
        }
    }

    pub fn save_playlist_to(&mut self, path: &Path) {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        let text = if ext == "xspf" { self.playlist.to_xspf(t(T::Playlist)) } else { self.playlist.to_m3u8() };
        match lumen400_core::settings::write_atomic(path, text.as_bytes()) {
            Ok(()) => {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                self.toast(ToastKind::Success, tf(T::PlaylistSaved, &[&name]), None);
            }
            Err(e) => self.toast(ToastKind::Error, t(T::PlaylistSaveFailed), Some(e.to_string())),
        }
    }

    /// Jump to the time typed in the dialog. Returns `false` when it cannot be understood.
    pub fn jump_to_text(&mut self, text: &str) -> bool {
        match parse_timecode(text) {
            Some(secs) => {
                self.seek_to(secs, SeekMode::Accurate);
                self.osd(icons::CLOCK, format_clock(secs), None);
                true
            }
            None => false,
        }
    }

    // =============================================================================================
    // Commands
    // =============================================================================================

    /// Runs a command (shortcut, menu entry or palette item).
    pub fn run(&mut self, action: Action) {
        use Action::*;
        let steps = self.settings.seek_steps;
        let last_dir = self.settings.last_dir.clone();
        self.wake_controls();
        match action {
            PlayPause => self.toggle_play(),
            Stop => self.stop(),
            Next => self.next_item(),
            Previous => self.previous_item(),
            SeekBackShort => self.seek_by(-steps[0]),
            SeekForwardShort => self.seek_by(steps[0]),
            SeekBackMedium => self.seek_by(-steps[1]),
            SeekForwardMedium => self.seek_by(steps[1]),
            SeekBackLong => self.seek_by(-steps[2]),
            SeekForwardLong => self.seek_by(steps[2]),
            FrameNext => self.frame_step(true),
            FramePrevious => self.frame_step(false),
            SpeedUp => self.step_speed(1),
            SpeedDown => self.step_speed(-1),
            SpeedReset => self.set_speed(1.0),
            NextChapter => self.jump_chapter(1),
            PreviousChapter => self.jump_chapter(-1),
            ABLoop => self.toggle_ab(),
            JumpToTime => self.windows.dialog = Some(Dialog::JumpToTime { text: String::new(), error: false, first_frame: true }),
            ToggleRepeat => self.cycle_repeat(),
            ToggleShuffle => self.toggle_shuffle(),
            VolumeUp => self.change_volume(1.0),
            VolumeDown => self.change_volume(-1.0),
            ToggleMute => self.toggle_mute(),
            CycleAudio => self.cycle_audio(),
            AudioDelayEarlier => self.shift_audio_delay(-AUDIO_DELAY_STEP),
            AudioDelayLater => self.shift_audio_delay(AUDIO_DELAY_STEP),
            OpenEqualizer => self.windows.equalizer = !self.windows.equalizer,
            ToggleNightMode => self.toggle_night_mode(),
            CycleSubtitles => self.cycle_subtitles(),
            LoadSubtitles => self.dialogs.open_subtitle(last_dir),
            SubtitleDelayEarlier => self.shift_subtitle_delay(-SUBTITLE_DELAY_STEP),
            SubtitleDelayLater => self.shift_subtitle_delay(SUBTITLE_DELAY_STEP),
            ToggleFullscreen => self.toggle_fullscreen(),
            CycleAspect => self.cycle_aspect(),
            CycleFit => self.cycle_fit(),
            RotateRight => self.rotate_right(),
            FlipHorizontal => self.flip_horizontal(),
            ZoomIn => self.zoom_by(1.1),
            ZoomOut => self.zoom_by(1.0 / 1.1),
            ZoomReset => self.reset_zoom(),
            ResetPicture => self.reset_picture(),
            Screenshot => self.screenshot(),
            ToggleAmbient => self.toggle_ambient(),
            CycleDeinterlace => self.cycle_deinterlace(),
            OpenFile => self.dialogs.open_files(last_dir, false),
            OpenFolder => self.dialogs.open_folder(last_dir, false),
            OpenUrl => self.windows.dialog = Some(Dialog::OpenUrl { text: String::new(), append: false, first_frame: true }),
            AddFiles => self.dialogs.open_files(last_dir, true),
            TogglePlaylist => self.windows.playlist = !self.windows.playlist,
            OpenSettings => self.windows.settings = if self.windows.settings.is_some() { None } else { Some(SettingsTab::Appearance) },
            ToggleInfo => self.windows.info = !self.windows.info,
            ToggleStats => self.windows.stats = !self.windows.stats,
            ShowShortcuts => self.windows.shortcuts = !self.windows.shortcuts,
            OpenPalette => {
                self.windows.palette = if self.windows.palette.is_some() { None } else { Some(PaletteState { query: String::new(), selected: 0, first_frame: true }) };
            }
            ToggleAlwaysOnTop => self.toggle_always_on_top(),
            ToggleRemaining => {
                self.settings.show_remaining_time = !self.settings.show_remaining_time;
                self.touch_settings();
            }
            Quit => self.ctx.send_viewport_cmd(ViewportCommand::Close),
        }
    }

    /// Closes the topmost transient layer. Returns `true` if there was one.
    pub fn dismiss_top_layer(&mut self) -> bool {
        if self.menu.is_some() {
            self.menu = None;
        } else if self.windows.palette.is_some() {
            self.windows.palette = None;
        } else if self.windows.dialog.is_some() {
            self.windows.dialog = None;
        } else if self.windows.shortcuts {
            self.windows.shortcuts = false;
        } else if self.windows.settings.is_some() {
            self.windows.settings = None;
        } else if self.windows.equalizer {
            self.windows.equalizer = false;
        } else if self.windows.info {
            self.windows.info = false;
        } else if self.windows.playlist {
            self.windows.playlist = false;
        } else {
            return false;
        }
        true
    }
}

fn format_secs(v: f64) -> String {
    if (v - v.round()).abs() < 1e-6 { format!("{}", v.round() as i64) } else { format!("{v:.1}") }
}

/// Speaker glyph for a volume level.
pub fn volume_icon(volume: f32, muted: bool) -> char {
    if muted || volume <= 0.001 {
        icons::SPEAKER_X
    } else if volume < 0.34 {
        icons::SPEAKER_NONE
    } else if volume < 0.8 {
        icons::SPEAKER_LOW
    } else {
        icons::SPEAKER_HIGH
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speeds_are_sorted_and_contain_normal_speed() {
        assert!(SPEEDS.windows(2).all(|w| w[0] < w[1]));
        assert!(SPEEDS.contains(&1.0));
        assert!(SPEEDS.iter().all(|s| (0.25..=4.0).contains(s)));
    }

    #[test]
    fn volume_icons() {
        assert_eq!(volume_icon(1.0, true), icons::SPEAKER_X);
        assert_eq!(volume_icon(0.0, false), icons::SPEAKER_X);
        assert_eq!(volume_icon(0.2, false), icons::SPEAKER_NONE);
        assert_eq!(volume_icon(0.5, false), icons::SPEAKER_LOW);
        assert_eq!(volume_icon(1.0, false), icons::SPEAKER_HIGH);
    }

    #[test]
    fn seconds_format() {
        assert_eq!(format_secs(5.0), "5");
        assert_eq!(format_secs(2.5), "2.5");
    }

    #[test]
    fn engine_config_follows_the_settings() {
        let s = Settings { preferred_audio_langs: "fr, en".into(), hw_decode: lumen400_core::settings::HwDecode::Auto, generate_thumbnails: false, ..Settings::default() };
        let c = engine_config(&s, true, false);
        assert!(c.hw_decode && !c.generate_thumbnails && c.allow_16bit_textures);
        assert_eq!(c.lang_prefs.audio, vec!["fr".to_owned(), "en".to_owned()]);
    }
}

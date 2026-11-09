//! The player's side of the desktop integration: what it tells the system, and how it obeys.

use std::sync::Arc;

use egui::{UserAttentionType, ViewportCommand};
use lumen400_core::media_kind::{is_file_url, is_url, path_to_file_url};
use lumen400_engine::{PlayState, SeekMode};

use super::{Command, State, Status, Track};
use crate::actions::Action;
use crate::app::LumenApp;
use crate::cli::Cli;
use crate::i18n::{Key as T, t};
use crate::ui::icons;
use crate::util::file_name_of;

impl LumenApp {
    /// Once per frame: carries out what the system asked for, then tells it how things are.
    pub fn pump_desktop(&mut self) {
        let Some(service) = self.desktop.as_mut() else { return };
        let commands = service.poll();
        for command in commands {
            self.on_desktop_command(command);
        }
        self.sync_desktop_track();
        let state = self.desktop_state();
        if let Some(service) = self.desktop.as_mut() {
            service.publish(state);
        }
    }

    fn on_desktop_command(&mut self, command: Command) {
        use PlayState::{Buffering, Ended, Failed, Idle, Opening, Paused, Playing};
        match command {
            Command::Play => {
                if matches!(self.snap.state, Paused | Ended | Idle | Failed) {
                    self.toggle_play();
                }
            }
            Command::Pause => {
                if matches!(self.snap.state, Playing | Buffering | Opening) {
                    self.player.pause();
                    self.osd(icons::PAUSE, t(T::Pause), None);
                }
            }
            Command::PlayPause => self.toggle_play(),
            Command::Stop => self.stop(),
            Command::Next => self.next_item(),
            Command::Previous => self.previous_item(),
            Command::SeekBy(secs) => self.seek_by(secs),
            Command::SeekTo { track, secs } => {
                if track == self.track.id {
                    self.seek_to(secs, SeekMode::Accurate);
                }
            }
            Command::SetVolume(v) => self.set_volume(v as f32, true),
            Command::SetRate(rate) => self.set_speed(rate),
            Command::SetRepeat(mode) => {
                if mode != self.playlist.repeat() {
                    self.set_repeat(mode);
                }
            }
            Command::SetShuffle(on) => {
                if on != self.playlist.shuffle() {
                    self.toggle_shuffle();
                }
            }
            Command::SetFullscreen(on) => self.set_fullscreen(on),
            Command::OpenUri(uri) => self.open_inputs(vec![uri], false),
            Command::Raise => self.raise_window(),
            Command::Quit => self.run(Action::Quit),
            Command::Open(cli) => {
                self.apply_cli(&cli, true);
                self.raise_window();
            }
        }
    }

    /// Brings the window forward (another launch, a click on a media widget).
    pub fn raise_window(&mut self) {
        self.ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
        self.ctx.send_viewport_cmd(ViewportCommand::Focus);
        self.ctx.send_viewport_cmd(ViewportCommand::RequestUserAttention(UserAttentionType::Informational));
        self.ctx.request_repaint();
    }

    /// Applies what a command line asks for. `remote`: it comes from another launch of the player,
    /// whose look options have not been applied yet (`main` does it at start-up for the first one).
    pub fn apply_cli(&mut self, cli: &Cli, remote: bool) {
        if remote {
            let mut look = false;
            if let Some(theme) = cli.theme {
                self.settings.theme = theme.into();
                look = true;
            }
            if let Some(accent) = cli.accent {
                self.settings.accent = accent.into();
                look = true;
            }
            if let Some(lang) = cli.lang {
                self.settings.language = lang.into();
                look = true;
            }
            if look {
                self.apply_look();
                self.touch_settings();
            }
            if cli.paused && !cli.inputs.is_empty() {
                self.boot_paused = true;
            }
        }
        if cli.fullscreen {
            self.set_fullscreen(true);
        }
        if let Some(volume) = cli.volume {
            self.set_volume((volume / 100.0).clamp(0.0, 2.0), false);
        }
        if cli.mute {
            self.settings.muted = true;
            self.player.set_muted(true);
        }
        if !cli.inputs.is_empty() {
            if !cli.enqueue {
                self.pending_start = cli.start_seconds();
            }
            self.open_inputs(cli.inputs.clone(), cli.enqueue);
        }
    }

    /// Rebuilds what is announced about the media when the media (or its cover) changed.
    fn sync_desktop_track(&mut self) {
        let media_key = self.media.as_ref().map(|m| Arc::as_ptr(m) as usize);
        let cover_key = self.cover_image.as_ref().map(|c| Arc::as_ptr(c) as usize);
        if self.track_key.source.as_deref() == self.source.as_deref() && self.track_key.media == media_key && self.track_key.cover == cover_key {
            return;
        }
        self.track_key = crate::app::TrackKey { source: self.source.clone(), media: media_key, cover: cover_key };
        let Some(source) = self.source.clone() else {
            self.track = Arc::new(Track::default());
            return;
        };
        let url = if is_url(&source) || is_file_url(&source) { source.clone() } else { path_to_file_url(std::path::Path::new(&source)) };
        // The same media keeps its identity while its details arrive.
        let id = if self.track.url.as_deref() == Some(url.as_str()) && self.track.id != 0 {
            self.track.id
        } else {
            self.track_counter += 1;
            self.track_counter
        };
        let media = self.media.clone();
        let title = media
            .as_ref()
            .and_then(|m| m.display_title())
            .or_else(|| self.current_playing_item().and_then(|item| self.playlist.get(item)).map(|item| item.display_name()))
            .unwrap_or_else(|| file_name_of(&source));
        self.track = Arc::new(Track {
            id,
            title,
            artists: media.as_ref().and_then(|m| m.artist()).map(|a| vec![a.to_owned()]).unwrap_or_default(),
            album: media.as_ref().and_then(|m| m.tag("album")).unwrap_or_default().to_owned(),
            url: Some(url),
            length: media.as_ref().and_then(|m| m.duration).filter(|d| d.is_finite() && *d > 0.0),
            cover: self.cover_image.clone(),
        });
    }

    fn desktop_state(&self) -> State {
        let snap = &self.snap;
        let status = match snap.state {
            PlayState::Playing | PlayState::Buffering | PlayState::Opening => Status::Playing,
            PlayState::Paused => Status::Paused,
            PlayState::Idle | PlayState::Ended | PlayState::Failed => Status::Stopped,
        };
        let has_media = self.has_media();
        State {
            status,
            position: if status == Status::Stopped { 0.0 } else { snap.position },
            rate: snap.speed,
            volume: if self.settings.muted { 0.0 } else { f64::from(self.settings.volume) },
            repeat: self.playlist.repeat(),
            shuffle: self.playlist.shuffle(),
            fullscreen: self.fullscreen,
            can_next: self.playlist.len() > 1,
            // "Previous" restarts the track when it is far in, so it is always meaningful with media.
            can_previous: has_media || self.playlist.len() > 1,
            can_seek: has_media && self.media.as_ref().is_some_and(|m| m.is_seekable),
            can_play: has_media || !self.playlist.is_empty(),
            can_pause: has_media,
            track: self.track.clone(),
        }
    }
}

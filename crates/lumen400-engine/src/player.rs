//! The public [`Player`] handle and the controller thread behind it.
//!
//! The UI calls cheap, non-blocking methods; a controller thread owns the audio device and the
//! current session and runs the playback state machine:
//!
//! ```text
//!            open                 media ready              buffers primed
//!   Idle ───────────► Opening ───────────────► Buffering ────────────────► Playing ⇄ Paused
//!     ▲                                           ▲  │ seek                    │
//!     │ stop                                      └──┘◄────────────────────────┤
//!     └────────────────────────────── Ended ◄───────────────────────────────────┘
//! ```
//!
//! Every seek starts a new *epoch*. Work tagged with an older epoch (packets, frames, audio blocks)
//! is dropped wherever it is found, which is what makes seeking feel instantaneous.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, unbounded};
use lumen400_core::dsp::eq::EqSettings;
use lumen400_core::dsp::spectrum::SpectrumAnalyzer;
use lumen400_core::settings::Deinterlace;
use lumen400_core::subtitle::SubtitleTrack;
use parking_lot::Mutex;

use crate::audio_out::AudioDevice;
use crate::demux::SeekReq;
use crate::error::EngineError;
use crate::frame::VideoFrame;
use crate::probe::StreamSelection;
use crate::session::{AudioSink, SessionArgs, SessionHandle, SessionMsg, SessionReady};
use crate::shared::{AudioHandle, EngineConfig, Event, Notice, PlayState, PlayerStats, SeekMode, Shared, Snapshot};
use crate::thumbs::{ThumbStore, Thumbnail, Thumbnailer};

/// Give up opening after this long (network stalls).
const OPEN_TIMEOUT: Duration = Duration::from_secs(45);
/// …and priming the buffers after this long.
const BUFFER_TIMEOUT: Duration = Duration::from_secs(30);
/// Audio that must be queued before playback starts, in seconds.
const PREROLL_SECONDS: f64 = 0.25;

enum Cmd {
    Open { source: String, start: Option<f64>, paused: bool, selection: StreamSelection },
    Play,
    Pause,
    Toggle,
    Stop,
    Seek { pos: f64, mode: SeekMode },
    SeekRelative(f64),
    SetSpeed(f64),
    SetTracks(StreamSelection),
    Step(bool),
    SetAudioDevice(Option<String>),
    SetConfig(EngineConfig),
    Quit,
}

/// Facts about the open media that the UI thread reads without asking the controller.
#[derive(Default)]
struct UiInfo {
    duration: Option<f64>,
    has_video: bool,
    has_audio: bool,
    seekable: bool,
}

/// Spectrum data for the audio visualiser.
#[derive(Clone, Debug, Default)]
pub struct SpectrumView {
    pub bands: Vec<f32>,
    pub peaks: Vec<f32>,
    /// Overall level `0.0 ..= 1.0`.
    pub level: f32,
}

/// Handle on the playback engine. Cheap to use from the UI thread: nothing here blocks.
pub struct Player {
    shared: Arc<Shared>,
    tx: Sender<Cmd>,
    events: Receiver<Event>,
    subtitles: Arc<Mutex<SubtitleTrack>>,
    ui: Arc<Mutex<UiInfo>>,
    thumbs: Arc<ThumbStore>,
    thread: Option<JoinHandle<()>>,
}

impl Player {
    /// Starts the engine. `waker` is called (from any thread) whenever something changed that the
    /// UI should redraw for — typically `ctx.request_repaint()`.
    pub fn new(cfg: EngineConfig, waker: Option<Arc<dyn Fn() + Send + Sync>>) -> Player {
        crate::init();
        let (ev_tx, ev_rx) = unbounded();
        let shared = Arc::new(Shared::new(ev_tx));
        *shared.waker.lock() = waker;
        let (tx, rx) = unbounded();
        let (msg_tx, msg_rx) = unbounded();
        let subtitles = Arc::new(Mutex::new(SubtitleTrack::default()));
        let ui = Arc::new(Mutex::new(UiInfo::default()));
        let thumbs = Arc::new(ThumbStore::default());
        let controller = Controller {
            shared: shared.clone(),
            cfg,
            rx,
            msg_tx,
            msg_rx,
            audio: None,
            session: None,
            ready: None,
            next_id: 0,
            source: String::new(),
            selection: StreamSelection::default(),
            phase: Phase::Idle,
            want_play: true,
            ui: ui.clone(),
            subtitles: subtitles.clone(),
            sys_clock: false,
            pending_seek: None,
            pending_step: None,
            start_accurate: false,
            thumbs: thumbs.clone(),
            thumbnailer: None,
            thumb_source: String::new(),
            media_counter: 0,
            hw_blocked: None,
        };
        let thread = thread::Builder::new().name("lumen400-ctl".into()).stack_size(16 * 1024 * 1024).spawn(move || controller.run()).expect("spawn controller");
        Player { shared, tx, events: ev_rx, subtitles, ui, thumbs, thread: Some(thread) }
    }

    fn send(&self, c: Cmd) {
        let _ = self.tx.send(c);
    }

    // --- transport ------------------------------------------------------------------------------

    /// Opens `source` (path, `file://` or network URL) and starts playing it.
    pub fn open(&self, source: &str) {
        self.open_with(source, None, false, StreamSelection::default());
    }

    /// Opens `source`, optionally resuming at `start` seconds and/or paused.
    pub fn open_with(&self, source: &str, start: Option<f64>, paused: bool, selection: StreamSelection) {
        self.send(Cmd::Open { source: source.to_owned(), start, paused, selection });
    }

    pub fn play(&self) {
        self.send(Cmd::Play);
    }
    pub fn pause(&self) {
        self.send(Cmd::Pause);
    }
    pub fn toggle_pause(&self) {
        self.send(Cmd::Toggle);
    }
    /// Closes the media.
    pub fn stop(&self) {
        self.send(Cmd::Stop);
    }
    pub fn seek(&self, pos: f64, mode: SeekMode) {
        self.send(Cmd::Seek { pos, mode });
    }
    /// Jumps `delta` seconds from the current position (negative = backwards).
    pub fn seek_relative(&self, delta: f64) {
        self.send(Cmd::SeekRelative(delta));
    }
    /// Playback speed (`0.25 ..= 4.0`); pitch is preserved.
    pub fn set_speed(&self, speed: f64) {
        self.send(Cmd::SetSpeed(speed));
    }
    /// Shows the next (`true`) or previous (`false`) frame while paused.
    pub fn step_frame(&self, forward: bool) {
        self.send(Cmd::Step(forward));
    }
    /// Picks tracks explicitly; restarts the pipeline at the current position.
    pub fn select_tracks(&self, sel: StreamSelection) {
        self.send(Cmd::SetTracks(sel));
    }
    pub fn set_audio_device(&self, id: Option<String>) {
        self.send(Cmd::SetAudioDevice(id));
    }
    /// Replaces the configuration (applies to the next media, except the audio device).
    pub fn set_config(&self, cfg: EngineConfig) {
        self.send(Cmd::SetConfig(cfg));
    }

    // --- audio (applied immediately, no round trip) ------------------------------------------------

    /// Volume `0.0 ..= 2.0` (`1.0` = 100 %).
    pub fn set_volume(&self, v: f32) {
        self.shared.volume.store(v, Ordering::Relaxed);
        if let Some(h) = self.shared.audio.read().as_ref() {
            h.shared.set_volume(v);
        }
    }

    pub fn set_muted(&self, m: bool) {
        self.shared.muted.store(m, Ordering::Relaxed);
        if let Some(h) = self.shared.audio.read().as_ref() {
            h.shared.set_muted(m);
        }
    }

    /// Night mode: a compressor that turns loud passages down and quiet ones up.
    pub fn set_night_mode(&self, on: bool) {
        self.shared.night.store(on, Ordering::Relaxed);
        if let Some(h) = self.shared.audio.read().as_ref() {
            h.shared.set_night(on);
        }
    }

    /// What to do about interlaced pictures. It applies from the next picture decoded: when the
    /// player is paused, seek (to the same position) to see the change on the picture on screen.
    pub fn set_deinterlace(&self, mode: Deinterlace) {
        self.shared.set_deinterlace(mode);
    }

    pub fn set_equalizer(&self, eq: &EqSettings) {
        *self.shared.eq.lock() = eq.clone();
        if let Some(h) = self.shared.audio.read().as_ref() {
            h.shared.set_eq(eq);
        }
    }

    /// A/V offset in seconds: positive values delay the sound relative to the picture.
    pub fn set_audio_delay(&self, seconds: f64) {
        self.shared.audio_delay.store(seconds.clamp(-5.0, 5.0), Ordering::Relaxed);
    }

    /// Enables / disables the spectrum tap (costs a little CPU in the audio callback).
    pub fn set_spectrum_enabled(&self, on: bool) {
        self.shared.spectrum_on.store(on, Ordering::Relaxed);
        if let Some(h) = self.shared.audio.read().as_ref() {
            h.shared.set_spectrum(on);
        }
    }

    /// Advances the analyser with the audio produced since the last call and returns its bands.
    pub fn spectrum(&self, dt: f32) -> Option<SpectrumView> {
        let h = self.shared.audio.read().clone()?;
        let mut an = h.analyzer.lock();
        {
            let mut tap = h.tap.lock();
            let n = tap.slots();
            if n > 0
                && let Ok(chunk) = tap.read_chunk(n)
            {
                let (a, b) = chunk.as_slices();
                an.push(a);
                an.push(b);
                chunk.commit_all();
            }
        }
        an.update(dt);
        Some(SpectrumView { bands: an.bands().to_vec(), peaks: an.peaks().to_vec(), level: an.level() })
    }

    // --- presentation -----------------------------------------------------------------------------

    /// The frame to display now, if it changed since the last call. `lookahead` (seconds) is how
    /// far in the future the picture will actually reach the screen (≈ one refresh interval).
    pub fn poll_frame(&self, lookahead: f64) -> Option<Arc<VideoFrame>> {
        let sh = &self.shared;
        if sh.show_next.swap(false, Ordering::AcqRel)
            && let Some(f) = sh.frames.take_front()
        {
            sh.last_pts.store(f.pts, Ordering::Relaxed);
            sh.counters.presented.fetch_add(1, Ordering::Relaxed);
            return Some(f);
        }
        if sh.state() != PlayState::Playing {
            return None;
        }
        let deadline = sh.clock.now() + sh.audio_delay.load(Ordering::Relaxed) + lookahead.max(0.0);
        let t = sh.frames.take_due(deadline);
        if t.dropped > 0 {
            sh.counters.dropped_late.fetch_add(u64::from(t.dropped), Ordering::Relaxed);
        }
        if let Some(f) = &t.frame {
            sh.last_pts.store(f.pts, Ordering::Relaxed);
            sh.counters.presented.fetch_add(1, Ordering::Relaxed);
        }
        t.frame
    }

    /// Seconds until the next queued frame is due, so the UI can sleep until then instead of
    /// redrawing at every refresh. `None` when nothing is playing or no frame is queued yet.
    pub fn next_frame_in(&self) -> Option<f64> {
        let sh = &self.shared;
        if sh.state() != PlayState::Playing {
            return None;
        }
        let pts = sh.frames.peek_pts()?;
        let now = sh.clock.now() + sh.audio_delay.load(Ordering::Relaxed);
        Some(((pts - now) / sh.speed.load(Ordering::Relaxed).max(0.1)).max(0.0))
    }

    /// Drains the notifications queued since the last call.
    pub fn poll_events(&self) -> Vec<Event> {
        self.events.try_iter().collect()
    }

    /// Cues decoded so far from the embedded subtitle track in use.
    pub fn embedded_subtitles(&self) -> Arc<Mutex<SubtitleTrack>> {
        self.subtitles.clone()
    }

    /// The preview picture closest to `t` seconds, if the generator has produced one yet.
    ///
    /// Previews are only made for local, seekable video files (and when
    /// [`EngineConfig::generate_thumbnails`] is on); they arrive coarse to fine while the file plays.
    pub fn thumbnail(&self, t: f64) -> Option<Thumbnail> {
        self.thumbs.nearest(t)
    }

    /// `(previews ready, previews planned)` for the current media.
    pub fn thumbnail_progress(&self) -> (usize, usize) {
        self.thumbs.progress()
    }

    // --- state ------------------------------------------------------------------------------------

    pub fn state(&self) -> PlayState {
        self.shared.state()
    }

    /// Playback position in seconds (frozen while paused, equal to the seek target while seeking).
    pub fn position(&self) -> f64 {
        let p = self.shared.clock.now().max(0.0);
        match self.ui.lock().duration {
            Some(d) => p.min(d),
            None => p,
        }
    }

    pub fn is_seekable(&self) -> bool {
        self.ui.lock().seekable
    }

    pub fn snapshot(&self) -> Snapshot {
        let sh = &self.shared;
        let ui = self.ui.lock();
        let c = &sh.counters;
        let audio = sh.audio.read().clone();
        let (audio_buffer_ms, underruns, latency_ms, device, is_virtual) = match &audio {
            Some(h) => (
                h.shared.buffered_ms(),
                h.shared.underruns.load(Ordering::Relaxed),
                h.shared.latency().as_secs_f32() * 1000.0,
                h.name.clone(),
                h.is_virtual,
            ),
            None => (0.0, 0, 0.0, String::new(), false),
        };
        let pos = {
            let p = sh.clock.now().max(0.0);
            ui.duration.map_or(p, |d| p.min(d))
        };
        let state = sh.state();
        let size = {
            let v = sh.video_size.load(Ordering::Relaxed);
            (v != 0).then_some(((v >> 32) as u32, v as u32))
        };
        Snapshot {
            state,
            position: pos,
            duration: ui.duration,
            speed: sh.speed.load(Ordering::Relaxed),
            volume: sh.volume.load(Ordering::Relaxed),
            muted: sh.muted.load(Ordering::Relaxed),
            has_video: ui.has_video,
            has_audio: ui.has_audio,
            video_size: size,
            stats: PlayerStats {
                decoded_frames: c.decoded_frames.load(Ordering::Relaxed),
                dropped_late: c.dropped_late.load(Ordering::Relaxed),
                dropped_decoder: c.dropped_decoder.load(Ordering::Relaxed),
                presented: c.presented.load(Ordering::Relaxed),
                decode_errors: c.decode_errors.load(Ordering::Relaxed),
                bytes_read: c.bytes_read.load(Ordering::Relaxed),
                frame_queue: sh.frames.len(),
                audio_buffer_ms,
                audio_underruns: underruns,
                audio_latency_ms: latency_ms,
                audio_device: device,
                audio_virtual: is_virtual,
                av_diff_ms: (state == PlayState::Playing && ui.has_video && ui.has_audio).then(|| {
                    let expected_pts = sh.clock.now() + sh.audio_delay.load(Ordering::Relaxed);
                    ((expected_pts - sh.last_pts.load(Ordering::Relaxed)) * 1000.0) as f32
                }),
                hw_decoder: *c.hw_device.lock(),
                deinterlacer: *c.deint.lock(),
            },
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Quit);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

// =================================================================================================
// Controller
// =================================================================================================

#[derive(Clone, Copy, Debug)]
enum Phase {
    Idle,
    Opening { since: Instant },
    Buffering { epoch: u32, since: Instant, target: f64, accurate: bool },
    Playing,
    Paused,
    Ended,
}

struct Controller {
    shared: Arc<Shared>,
    cfg: EngineConfig,
    rx: Receiver<Cmd>,
    msg_tx: Sender<SessionMsg>,
    msg_rx: Receiver<SessionMsg>,
    audio: Option<AudioDevice>,
    session: Option<SessionHandle>,
    ready: Option<SessionReady>,
    next_id: u64,
    source: String,
    selection: StreamSelection,
    phase: Phase,
    /// What the user wants: `true` = play as soon as possible.
    want_play: bool,
    ui: Arc<Mutex<UiInfo>>,
    subtitles: Arc<Mutex<SubtitleTrack>>,
    /// The clock is driven by the system time (no audio, or the audio already ended).
    sys_clock: bool,
    /// A seek asked for while the media was still opening.
    pending_seek: Option<(f64, SeekMode)>,
    /// A frame step waiting for its frame.
    pending_step: Option<bool>,
    /// Whether the seek that started the session must be sample accurate.
    start_accurate: bool,
    thumbs: Arc<ThumbStore>,
    thumbnailer: Option<Thumbnailer>,
    /// The source the running (or finished) generator works on.
    thumb_source: String,
    media_counter: u64,
    /// A source whose hardware decoding failed: it plays in software from now on.
    hw_blocked: Option<String>,
}

impl Controller {
    fn run(mut self) {
        self.open_audio();
        let rx = self.rx.clone();
        let msg_rx = self.msg_rx.clone();
        loop {
            let busy = matches!(self.phase, Phase::Opening { .. } | Phase::Buffering { .. } | Phase::Playing) || self.pending_step.is_some();
            let timeout = Duration::from_millis(if busy { 3 } else { 40 });
            crossbeam_channel::select! {
                recv(rx) -> cmd => match cmd {
                    Ok(Cmd::Quit) | Err(_) => break,
                    Ok(c) => self.handle(c),
                },
                recv(msg_rx) -> m => {
                    if let Ok(m) = m {
                        self.on_msg(m);
                    }
                },
                default(timeout) => {},
            }
            self.tick();
        }
        self.close_session();
        self.stop_thumbnails(true);
    }

    // --- helpers ------------------------------------------------------------------------------------

    fn set_state(&self, s: PlayState) {
        self.shared.set_state(s);
    }

    fn speed(&self) -> f64 {
        self.shared.speed.load(Ordering::Relaxed)
    }

    fn position(&self) -> f64 {
        self.shared.clock.now().max(0.0)
    }

    fn duration(&self) -> Option<f64> {
        self.ui.lock().duration
    }

    fn bump_epoch(&mut self) -> u32 {
        let e = self.shared.epoch.load(Ordering::Acquire).wrapping_add(1).max(1);
        self.shared.epoch.store(e, Ordering::Release);
        self.shared.frames.flush(e);
        self.shared.show_next.store(false, Ordering::Release);
        if let Some(a) = &self.audio {
            a.shared.gate.store(false, Ordering::Release);
            a.shared.begin_epoch(e);
        }
        e
    }

    fn open_audio(&mut self) {
        self.audio = None;
        *self.shared.audio.write() = None;
        let dev = AudioDevice::open(self.cfg.audio_device.as_deref(), self.shared.clock.clone(), self.cfg.force_virtual_audio);
        dev.shared.set_volume(self.shared.volume.load(Ordering::Relaxed));
        dev.shared.set_muted(self.shared.muted.load(Ordering::Relaxed));
        dev.shared.set_eq(&self.shared.eq.lock());
        dev.shared.set_night(self.shared.night.load(Ordering::Relaxed));
        dev.shared.set_spectrum(self.shared.spectrum_on.load(Ordering::Relaxed));
        dev.shared.epoch.store(self.shared.current_epoch(), Ordering::Release);
        *self.shared.audio.write() = Some(AudioHandle {
            shared: dev.shared.clone(),
            tap: dev.tap.clone(),
            analyzer: Arc::new(Mutex::new(SpectrumAnalyzer::new(dev.rate, 48))),
            name: dev.name.clone(),
            is_virtual: dev.is_virtual,
        });
        self.shared.emit(Event::AudioDevice { name: dev.name.clone(), is_virtual: dev.is_virtual });
        if dev.is_virtual && !self.cfg.force_virtual_audio {
            self.shared.emit(Event::Warning(Notice::NoAudioOutput));
        }
        self.audio = Some(dev);
    }

    fn sink(&self) -> Option<AudioSink> {
        self.audio.as_ref().map(|a| AudioSink { dev: a.shared.clone(), link: a.link.clone(), rate: a.rate })
    }

    fn close_session(&mut self) {
        if let Some(s) = self.session.take() {
            s.shutdown();
        }
        self.ready = None;
        if let Some(a) = &self.audio {
            a.shared.gate.store(false, Ordering::Release);
        }
    }

    fn start_session(&mut self, pos: f64, accurate: bool) {
        let epoch = self.bump_epoch();
        self.shared.clock.hold(pos);
        self.sys_clock = false;
        self.start_accurate = accurate;
        self.next_id += 1;
        let Some(audio) = self.sink() else { return };
        let mut cfg = self.cfg.clone();
        cfg.hw_decode &= self.hw_blocked.as_deref() != Some(self.source.as_str());
        *self.shared.counters.hw_device.lock() = None;
        *self.shared.counters.deint.lock() = None;
        self.shared.hw_failed.store(false, Ordering::Release);
        let args = SessionArgs {
            id: self.next_id,
            source: self.source.clone(),
            selection: self.selection,
            start: SeekReq { epoch, target: pos, accurate },
            cfg,
            shared: self.shared.clone(),
            audio,
            msgs: self.msg_tx.clone(),
            subtitle_track: self.subtitles.clone(),
        };
        self.session = Some(SessionHandle::spawn(args));
        self.phase = Phase::Opening { since: Instant::now() };
        self.set_state(PlayState::Opening);
    }

    fn fail(&mut self, text: String) {
        self.close_session();
        self.phase = Phase::Idle;
        self.set_state(PlayState::Failed);
        self.shared.emit(Event::Error(text));
    }

    // --- previews -----------------------------------------------------------------------------------

    /// Starts the preview generator for the media that just became ready (once per source: a
    /// session restart for a track change keeps the pictures it already has).
    fn start_thumbnails(&mut self, r: &SessionReady) {
        if !self.cfg.generate_thumbnails || !r.has_video || !r.seekable || r.info.is_live {
            return;
        }
        if self.thumbnailer.is_some() && self.thumb_source == self.source {
            return;
        }
        let Some(duration) = r.info.duration else { return };
        if lumen400_core::media_kind::is_url(&crate::probe::normalize_source(&self.source)) {
            return; // never open a second connection to a remote server just for previews
        }
        self.stop_thumbnails(false);
        self.media_counter += 1;
        self.thumbnailer = Thumbnailer::spawn(self.source.clone(), duration, self.thumbs.clone(), self.media_counter, self.shared.clone());
        self.thumb_source = if self.thumbnailer.is_some() { self.source.clone() } else { String::new() };
    }

    fn stop_thumbnails(&mut self, wait: bool) {
        if let Some(t) = self.thumbnailer.take() {
            if wait {
                t.join();
            } else {
                t.cancel();
            }
        }
        self.thumb_source.clear();
        self.thumbs.clear();
    }

    // --- commands -----------------------------------------------------------------------------------

    fn handle(&mut self, c: Cmd) {
        match c {
            Cmd::Open { source, start, paused, selection } => {
                self.close_session();
                {
                    let mut ui = self.ui.lock();
                    *ui = UiInfo::default();
                }
                self.pending_seek = None;
                self.pending_step = None;
                if source != self.source {
                    self.stop_thumbnails(false);
                }
                self.source = source;
                self.selection = selection;
                self.want_play = !paused;
                let start = start.filter(|s| *s > 0.0);
                self.shared.counters.decoded_frames.store(0, Ordering::Relaxed);
                self.shared.counters.dropped_late.store(0, Ordering::Relaxed);
                self.shared.counters.presented.store(0, Ordering::Relaxed);
                self.shared.counters.bytes_read.store(0, Ordering::Relaxed);
                self.shared.counters.decode_errors.store(0, Ordering::Relaxed);
                self.start_session(start.unwrap_or(0.0), start.is_some());
            }
            Cmd::Play => self.play(),
            Cmd::Pause => self.pause(),
            Cmd::Toggle => match self.phase {
                Phase::Playing => self.pause(),
                Phase::Buffering { .. } | Phase::Opening { .. } if self.want_play => self.pause(),
                _ => self.play(),
            },
            Cmd::Stop => {
                self.close_session();
                self.stop_thumbnails(false);
                self.source.clear();
                self.bump_epoch();
                self.shared.clock.hold(0.0);
                self.phase = Phase::Idle;
                *self.ui.lock() = UiInfo::default();
                self.subtitles.lock().clear();
                self.set_state(PlayState::Idle);
            }
            Cmd::Seek { pos, mode } => self.seek(pos, mode),
            Cmd::SeekRelative(d) => {
                let base = match self.phase {
                    Phase::Buffering { target, .. } => target,
                    _ => self.position(),
                };
                self.seek(base + d, SeekMode::Accurate);
            }
            Cmd::SetSpeed(s) => {
                let s = if s.is_finite() { s.clamp(0.25, 4.0) } else { 1.0 };
                self.shared.speed.store(s, Ordering::Relaxed);
                if self.sys_clock && matches!(self.phase, Phase::Playing) {
                    let p = self.shared.clock.now();
                    self.shared.clock.set(p, s);
                }
            }
            Cmd::SetTracks(sel) => {
                if self.session.is_some() {
                    let pos = match self.phase {
                        Phase::Buffering { target, .. } => target,
                        _ => self.position(),
                    };
                    self.close_session();
                    self.selection = sel;
                    self.start_session(pos, true);
                } else {
                    self.selection = sel;
                }
            }
            Cmd::Step(forward) => self.step(forward),
            Cmd::SetAudioDevice(dev) => {
                self.cfg.audio_device = dev;
                self.reopen_audio();
            }
            Cmd::SetConfig(cfg) => {
                let device_changed = cfg.audio_device != self.cfg.audio_device || cfg.force_virtual_audio != self.cfg.force_virtual_audio;
                let thumbs_off = self.cfg.generate_thumbnails && !cfg.generate_thumbnails;
                self.cfg = cfg;
                if thumbs_off {
                    self.stop_thumbnails(false);
                }
                if device_changed {
                    self.reopen_audio();
                }
            }
            Cmd::Quit => {}
        }
    }

    /// Reopens the audio output and resumes the current media where it was.
    fn reopen_audio(&mut self) {
        let had_session = self.session.is_some();
        let pos = match self.phase {
            Phase::Buffering { target, .. } => target,
            _ => self.position(),
        };
        self.close_session();
        self.open_audio();
        if had_session {
            self.start_session(pos, true);
        }
    }

    fn play(&mut self) {
        self.want_play = true;
        match self.phase {
            Phase::Paused => {
                let pos = self.shared.clock.now();
                self.start_clock(pos);
                self.phase = Phase::Playing;
                self.set_state(PlayState::Playing);
            }
            Phase::Ended => self.seek(0.0, SeekMode::Accurate),
            _ => {}
        }
    }

    fn pause(&mut self) {
        self.want_play = false;
        if matches!(self.phase, Phase::Playing) {
            if let Some(a) = &self.audio {
                a.shared.gate.store(false, Ordering::Release);
            }
            let p = self.shared.clock.now();
            self.shared.clock.hold(p);
            self.phase = Phase::Paused;
            self.set_state(PlayState::Paused);
        }
    }

    /// Starts the master clock at `pos`.
    fn start_clock(&mut self, pos: f64) {
        let has_audio = self.ready.as_ref().is_some_and(|r| r.has_audio);
        match (&self.audio, has_audio) {
            (Some(a), true) => {
                self.sys_clock = false;
                self.shared.clock.set_audio_driven(true);
                // Frozen at `pos` until the first audio callback anchors it.
                self.shared.clock.hold(pos);
                self.shared.clock.release();
                a.shared.gate.store(true, Ordering::Release);
            }
            _ => {
                self.sys_clock = true;
                self.shared.clock.set_audio_driven(false);
                self.shared.clock.set(pos, self.speed());
            }
        }
    }

    fn seek(&mut self, pos: f64, mode: SeekMode) {
        let Some(r) = self.ready.as_ref() else {
            if self.session.is_some() {
                self.pending_seek = Some((pos, mode));
            }
            return;
        };
        if !r.seekable {
            self.shared.emit(Event::Warning(Notice::NotSeekable));
            return;
        }
        let mut pos = if pos.is_finite() { pos.max(0.0) } else { 0.0 };
        if let Some(d) = self.duration() {
            pos = pos.min((d - 0.05).max(0.0));
        }
        let has_audio = r.has_audio;
        let epoch = self.bump_epoch();
        self.shared.clock.hold(pos);
        self.shared.clock.set_audio_driven(has_audio);
        self.sys_clock = !has_audio;
        self.pending_step = None;
        let accurate = mode == SeekMode::Accurate;
        if let Some(s) = &self.session {
            s.seek(SeekReq { epoch, target: pos, accurate });
        }
        self.phase = Phase::Buffering { epoch, since: Instant::now(), target: pos, accurate };
        self.set_state(PlayState::Buffering);
    }

    fn step(&mut self, forward: bool) {
        if !matches!(self.phase, Phase::Paused | Phase::Ended) {
            return;
        }
        if forward {
            self.pending_step = Some(true);
        } else if let Some(r) = &self.ready {
            let last = self.shared.last_pts.load(Ordering::Relaxed);
            let target = (last - r.frame_dur).max(0.0);
            // Stepping back always lands on a paused picture.
            self.want_play = false;
            self.seek(target, SeekMode::Accurate);
        }
    }

    // --- session messages -----------------------------------------------------------------------------

    fn on_msg(&mut self, m: SessionMsg) {
        let current = self.session.as_ref().map(|s| s.id);
        match m {
            SessionMsg::Ready(r) if Some(r.id) == current => self.on_ready(r),
            SessionMsg::Failed { id, error } if Some(id) == current => {
                let text = match &error {
                    EngineError::Open { input, reason } => format!("cannot open \"{input}\": {reason}"),
                    e => e.to_string(),
                };
                self.fail(text);
            }
            _ => {}
        }
    }

    fn on_ready(&mut self, r: SessionReady) {
        {
            let mut ui = self.ui.lock();
            ui.duration = r.info.duration;
            ui.has_video = r.has_video;
            ui.has_audio = r.has_audio;
            ui.seekable = r.seekable;
        }
        self.shared.frame_dur.store(r.frame_dur, Ordering::Relaxed);
        self.shared.emit(Event::Opened(r.info.clone()));
        if let Some(c) = r.cover.clone() {
            self.shared.emit(Event::Cover(Arc::new(c)));
        }
        self.start_thumbnails(&r);
        self.shared.clock.set_audio_driven(r.has_audio);
        self.sys_clock = !r.has_audio;
        let epoch = self.shared.current_epoch();
        let target = self.shared.clock.anchor().pts;
        self.ready = Some(r);
        self.phase = Phase::Buffering { epoch, since: Instant::now(), target, accurate: self.start_accurate };
        self.set_state(PlayState::Buffering);
        if let Some((pos, mode)) = self.pending_seek.take() {
            self.seek(pos, mode);
        }
    }

    // --- periodic work ---------------------------------------------------------------------------------

    fn tick(&mut self) {
        if self.shared.hw_failed.swap(false, Ordering::AcqRel) && self.session.is_some() && self.cfg.hw_decode {
            // The video worker could not use the hardware decoder: replay from here in software.
            tracing::warn!("hardware decoding failed; restarting in software");
            self.hw_blocked = Some(self.source.clone());
            self.shared.emit(Event::Warning(Notice::HardwareDecodeFailed));
            let pos = match self.phase {
                Phase::Buffering { target, .. } => target,
                _ => self.position(),
            };
            self.close_session();
            self.start_session(pos, true);
        }
        if let Some(a) = &self.audio
            && a.shared.device_error.swap(false, Ordering::AcqRel)
        {
            tracing::warn!("audio device lost; reopening");
            self.shared.emit(Event::Warning(Notice::AudioDeviceLost));
            self.cfg.audio_device = None;
            self.reopen_audio();
        }
        match self.phase {
            Phase::Opening { since } => {
                if since.elapsed() > OPEN_TIMEOUT {
                    self.fail("timed out while opening the media".into());
                }
            }
            Phase::Buffering { .. } => self.check_buffering(),
            Phase::Playing => self.tick_playing(),
            Phase::Paused | Phase::Ended => self.tick_stepping(),
            Phase::Idle => {}
        }
    }

    fn check_buffering(&mut self) {
        let Phase::Buffering { epoch, since, target, accurate } = self.phase else { return };
        let Some(r) = self.ready.as_ref() else { return };
        let frames = &self.shared.frames;
        let video_ok = !r.has_video
            || (!frames.is_empty() && ((accurate && frames.len() >= 2) || frames.len() >= 6 || since.elapsed() > Duration::from_millis(1000)))
            || frames.drained();
        let audio_ok = !r.has_audio
            || self.audio.as_ref().is_none_or(|a| {
                let s = &a.shared;
                let need_sec = if accurate { 0.05 } else { PREROLL_SECONDS };
                s.produced_epoch.load(Ordering::Acquire) == epoch
                    && (s.produced_frames.load(Ordering::Acquire) as f64 >= need_sec * f64::from(a.rate) || s.produced_eof.load(Ordering::Acquire))
            });
        if video_ok && audio_ok {
            let start = if accurate { target } else { frames.peek_pts().unwrap_or(target) };
            let has_video = r.has_video;
            self.shared.clock.hold(start);
            if has_video {
                self.shared.show_next.store(true, Ordering::Release);
            }
            if self.want_play {
                self.start_clock(start);
                self.phase = Phase::Playing;
                self.set_state(PlayState::Playing);
            } else {
                self.phase = Phase::Paused;
                self.set_state(PlayState::Paused);
            }
            self.shared.wake();
        } else if since.elapsed() > BUFFER_TIMEOUT {
            self.fail("timed out while buffering the media".into());
        }
    }

    fn tick_playing(&mut self) {
        let Some(r) = self.ready.as_ref() else { return };
        let (has_audio, has_video, frame_dur) = (r.has_audio, r.has_video, r.frame_dur);
        let epoch = self.shared.current_epoch();
        let audio_done = !has_audio || self.audio.as_ref().is_none_or(|a| a.shared.finished(epoch));

        // Keep the pipeline flowing even if nobody presents frames (window hidden…).
        if let Some(pts) = self.shared.frames.peek_pts() {
            let now = self.shared.clock.now();
            if now > pts + 2.0 {
                let t = self.shared.frames.take_due(now);
                let dropped = u64::from(t.dropped) + u64::from(t.frame.is_some());
                self.shared.counters.dropped_late.fetch_add(dropped, Ordering::Relaxed);
            }
        }
        let video_done = !has_video || (self.shared.frames.drained() && self.shared.clock.now() >= self.shared.last_pts.load(Ordering::Relaxed) + 0.5 * frame_dur);

        // The sound ended before the picture: let the system clock carry on.
        if has_audio && audio_done && has_video && !video_done && !self.sys_clock {
            let p = self.shared.clock.now();
            self.shared.clock.set_audio_driven(false);
            self.shared.clock.set(p, self.speed());
            self.sys_clock = true;
        }
        if audio_done && video_done {
            self.on_ended();
        }
    }

    fn tick_stepping(&mut self) {
        let Some(forward) = self.pending_step else { return };
        let _ = forward;
        if let Some(pts) = self.shared.frames.peek_pts() {
            self.shared.clock.hold(pts);
            self.shared.show_next.store(true, Ordering::Release);
            self.pending_step = None;
            self.shared.wake();
        } else if self.shared.frames.drained() {
            self.pending_step = None;
        }
    }

    fn on_ended(&mut self) {
        if let Some(a) = &self.audio {
            a.shared.gate.store(false, Ordering::Release);
        }
        // Show the full bar once the end is reached (the clock may stop a few ms short).
        let now = self.shared.clock.now();
        let end = match self.duration() {
            Some(d) if (d - now).abs() < 2.0 => d,
            _ => now,
        };
        self.shared.clock.hold(end);
        self.phase = Phase::Ended;
        self.set_state(PlayState::Ended);
        self.shared.emit(Event::Ended);
    }
}

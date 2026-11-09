//! State and data types shared by the controller, the worker threads and the UI.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};

use crossbeam_channel::Sender;
use lumen400_core::dsp::eq::EqSettings;
use lumen400_core::dsp::spectrum::SpectrumAnalyzer;
use lumen400_core::settings::Deinterlace;
use lumen400_core::sync::{AtomicF32, AtomicF64};
use lumen400_core::types::MediaInfo;
use parking_lot::{Mutex, RwLock};
use rtrb::Consumer;

use crate::audio_out::AudioShared;
use crate::frame::RgbaImage;
use crate::probe::LangPrefs;
use crate::queue::FrameQueue;
use crate::util::PlaybackClock;

/// High level playback state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum PlayState {
    /// Nothing loaded.
    #[default]
    Idle = 0,
    /// The container is being opened / analysed.
    Opening = 1,
    /// Filling the buffers after an open or a seek.
    Buffering = 2,
    Playing = 3,
    Paused = 4,
    /// The end of the media was reached.
    Ended = 5,
    /// The media could not be played (see the error event).
    Failed = 6,
}

impl PlayState {
    pub(crate) fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Opening,
            2 => Self::Buffering,
            3 => Self::Playing,
            4 => Self::Paused,
            5 => Self::Ended,
            6 => Self::Failed,
            _ => Self::Idle,
        }
    }

    /// `true` while a media is loaded and not finished.
    pub fn is_active(self) -> bool {
        matches!(self, Self::Opening | Self::Buffering | Self::Playing | Self::Paused)
    }
}

/// How precisely a seek must land.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SeekMode {
    /// Land exactly on the requested time (decodes from the previous keyframe).
    #[default]
    Accurate,
    /// Land on the closest keyframe before it: instant, used while scrubbing.
    Fast,
}

/// A recoverable problem. The UI decides which ones deserve a message and translates them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
    /// No sound card could be opened: the media plays silently.
    NoAudioOutput,
    /// The sound card went away (unplugged, switched…); the engine keeps trying.
    AudioDeviceLost,
    /// The source cannot be seeked (live stream).
    NotSeekable,
    /// One stream could not be decoded and was skipped (FFmpeg's reason).
    StreamSkipped(String),
    /// The hardware decoder did not work on this media; it now plays in software.
    HardwareDecodeFailed,
    /// Anything else; worth a log line, not a message.
    Other(String),
}

/// Notifications for the UI (drained with [`Player::poll_events`](crate::Player::poll_events)).
#[derive(Clone, Debug)]
pub enum Event {
    /// A media was opened; streams and metadata are known.
    Opened(Arc<MediaInfo>),
    /// Cover art found in an audio file.
    Cover(Arc<RgbaImage>),
    /// The playback state changed.
    State(PlayState),
    /// The media played to its end.
    Ended,
    /// The media could not be opened or died; the text is for display / logs.
    Error(String),
    /// Something degraded but playback continues (no sound card, seek failure…).
    Warning(Notice),
    /// The audio output was (re)opened.
    AudioDevice { name: String, is_virtual: bool },
}

/// Engine settings (the application copies them from its persisted settings).
#[derive(Clone, Debug)]
pub struct EngineConfig {
    /// Try hardware decoding (experimental).
    pub hw_decode: bool,
    /// Output device id as returned by [`list_output_devices`](crate::list_output_devices).
    pub audio_device: Option<String>,
    /// Never touch the sound card (tests, benchmarks, CI).
    pub force_virtual_audio: bool,
    /// The renderer can sample 16-bit normalised textures (10/12-bit video without conversion).
    pub allow_16bit_textures: bool,
    /// Memory budget of the decoded-frame queue.
    pub frame_queue_bytes: usize,
    /// Audio read-ahead, in milliseconds (also the delay before a speed change is heard).
    pub audio_buffer_ms: u32,
    /// Track selection preferences.
    pub lang_prefs: LangPrefs,
    /// Video decoder threads (`0` = automatic).
    pub decoder_threads: usize,
    /// Generate seek-bar preview thumbnails in the background.
    pub generate_thumbnails: bool,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            hw_decode: false,
            audio_device: None,
            force_virtual_audio: false,
            allow_16bit_textures: true,
            frame_queue_bytes: 192 << 20,
            audio_buffer_ms: 300,
            lang_prefs: LangPrefs { audio: Vec::new(), subtitles: Vec::new(), auto_subtitles: true },
            decoder_threads: 0,
            generate_thumbnails: true,
        }
    }
}

/// Counters updated by the worker threads.
#[derive(Default)]
pub(crate) struct Counters {
    pub decoded_frames: AtomicU64,
    /// Frames the renderer skipped because they were already late.
    pub dropped_late: AtomicU64,
    /// Frames discarded by the decoder (accurate seeks do not count).
    pub dropped_decoder: AtomicU64,
    pub presented: AtomicU64,
    /// Container bytes read since the session started.
    pub bytes_read: AtomicU64,
    /// The hardware device the decoder is really using (`None`: software).
    pub hw_device: Mutex<Option<&'static str>>,
    /// The deinterlacing filter at work (`None`: pictures go through untouched).
    pub deint: Mutex<Option<&'static str>>,
    pub decode_errors: AtomicU64,
}

pub(crate) type Waker = Arc<dyn Fn() + Send + Sync>;

/// What the UI side needs from the running audio output.
#[derive(Clone)]
pub(crate) struct AudioHandle {
    pub shared: Arc<AudioShared>,
    pub tap: Arc<Mutex<Consumer<f32>>>,
    pub analyzer: Arc<Mutex<SpectrumAnalyzer>>,
    pub name: String,
    pub is_virtual: bool,
}

/// Everything the threads of the player share.
pub(crate) struct Shared {
    pub clock: Arc<PlaybackClock>,
    pub frames: FrameQueue,
    /// Current seek epoch; work tagged with another epoch is stale.
    pub epoch: AtomicU32,
    pub speed: AtomicF64,
    /// User A/V offset in seconds (positive: show video earlier, i.e. delay the sound).
    pub audio_delay: AtomicF64,
    /// Present the next queued frame regardless of its timestamp (first frame after a seek,
    /// frame stepping).
    pub show_next: AtomicBool,
    /// PTS of the frame handed to the renderer last.
    pub last_pts: AtomicF64,
    pub state: AtomicU8,
    pub counters: Counters,
    /// The video worker could not use the hardware decoder: restart the media in software.
    pub hw_failed: AtomicBool,
    pub events: Sender<Event>,
    pub waker: Mutex<Option<Waker>>,
    pub frame_dur: AtomicF64,
    /// Size of the last decoded picture, `width << 32 | height` (0 = none yet).
    pub video_size: AtomicU64,
    /// Running audio output (published by the controller).
    pub audio: RwLock<Option<AudioHandle>>,
    /// User audio settings; re-applied whenever the output is reopened.
    pub volume: AtomicF32,
    pub muted: AtomicBool,
    /// Night mode: loud passages are turned down, quiet ones up.
    pub night: AtomicBool,
    /// A [`Deinterlace`] policy, as `Deinterlace as u8`; the video decoder reads it for every picture.
    pub deint: AtomicU8,
    pub eq: Mutex<EqSettings>,
    pub spectrum_on: AtomicBool,
}

impl Shared {
    pub fn new(events: Sender<Event>) -> Self {
        Self {
            clock: Arc::new(PlaybackClock::new()),
            frames: FrameQueue::new(8),
            epoch: AtomicU32::new(1),
            speed: AtomicF64::new(1.0),
            audio_delay: AtomicF64::new(0.0),
            show_next: AtomicBool::new(false),
            last_pts: AtomicF64::new(0.0),
            state: AtomicU8::new(PlayState::Idle as u8),
            counters: Counters::default(),
            hw_failed: AtomicBool::new(false),
            events,
            waker: Mutex::new(None),
            frame_dur: AtomicF64::new(1.0 / 25.0),
            video_size: AtomicU64::new(0),
            audio: RwLock::new(None),
            volume: AtomicF32::new(1.0),
            muted: AtomicBool::new(false),
            night: AtomicBool::new(false),
            deint: AtomicU8::new(Deinterlace::default() as u8),
            eq: Mutex::new(EqSettings::default()),
            spectrum_on: AtomicBool::new(false),
        }
    }

    pub fn state(&self) -> PlayState {
        PlayState::from_u8(self.state.load(Ordering::Acquire))
    }

    pub fn wake(&self) {
        if let Some(w) = self.waker.lock().as_ref() {
            w();
        }
    }

    pub fn emit(&self, e: Event) {
        let _ = self.events.send(e);
        self.wake();
    }

    pub fn set_state(&self, s: PlayState) {
        if self.state.swap(s as u8, Ordering::AcqRel) != s as u8 {
            self.emit(Event::State(s));
        }
    }

    pub fn current_epoch(&self) -> u32 {
        self.epoch.load(Ordering::Acquire)
    }

    pub fn deinterlace(&self) -> Deinterlace {
        match self.deint.load(Ordering::Relaxed) {
            x if x == Deinterlace::Off as u8 => Deinterlace::Off,
            x if x == Deinterlace::On as u8 => Deinterlace::On,
            _ => Deinterlace::Auto,
        }
    }

    pub fn set_deinterlace(&self, mode: Deinterlace) {
        self.deint.store(mode as u8, Ordering::Relaxed);
    }
}

/// Measurements for the statistics overlay.
#[derive(Clone, Debug, Default)]
pub struct PlayerStats {
    pub decoded_frames: u64,
    pub dropped_late: u64,
    pub dropped_decoder: u64,
    pub presented: u64,
    pub decode_errors: u64,
    /// Container bytes read since the media was opened.
    pub bytes_read: u64,
    /// Decoded frames waiting for presentation.
    pub frame_queue: usize,
    /// Audio waiting in the output ring, in milliseconds.
    pub audio_buffer_ms: f32,
    pub audio_underruns: u64,
    /// Output latency of the sound device in milliseconds.
    pub audio_latency_ms: f32,
    pub audio_device: String,
    pub audio_virtual: bool,
    /// Master clock minus the PTS of the frame on screen, in milliseconds (≈ 0 when in sync).
    pub av_diff_ms: Option<f32>,
    /// The hardware decoder in use (`"vaapi"`, `"cuda"`…); `None` when decoding in software.
    pub hw_decoder: Option<&'static str>,
    /// The filter that is rebuilding whole pictures from interlaced fields (`"bwdif"`, `"yadif"`);
    /// `None` when the picture is progressive or deinterlacing is off.
    pub deinterlacer: Option<&'static str>,
}

/// A consistent view of the player for one UI frame.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub state: PlayState,
    /// Playback position in seconds.
    pub position: f64,
    pub duration: Option<f64>,
    pub speed: f64,
    pub volume: f32,
    pub muted: bool,
    pub has_video: bool,
    pub has_audio: bool,
    /// Decoded picture size (`None` before the first frame).
    pub video_size: Option<(u32, u32)>,
    pub stats: PlayerStats,
}

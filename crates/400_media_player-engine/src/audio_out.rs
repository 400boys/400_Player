//! Audio output.
//!
//! The audio decoder thread writes blocks of interleaved stereo `f32` into a lock-free ring
//! (`rtrb`), each announced by a [`Mark`] in a second ring that says which media-time span the
//! block represents and which seek *epoch* it belongs to. The device callback consumes both rings:
//!
//! * blocks of an outdated epoch (everything queued before a seek) are discarded without being
//!   heard, so a seek is audible after at most one callback period;
//! * every period the callback re-anchors the shared [`PlaybackClock`] with the media position of
//!   the sample that is *about to be heard* (device latency included) — audio is the master clock;
//! * volume, mute, the equaliser, the limiter and the pause fade run here, after time stretching,
//!   so they react within one period.
//!
//! When no sound card can be opened (headless server, CI, sandbox) a *virtual device* drives the
//! very same renderer from a paced thread: playback timing stays correct, only the sound goes
//! nowhere.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
use media_player_400_core::dsp::compressor::Compressor;
use media_player_400_core::dsp::eq::{EqSettings, Equalizer};
use media_player_400_core::dsp::{soft_clip, volume_to_gain};
use media_player_400_core::sync::AtomicF32;
use parking_lot::Mutex;
use rtrb::{Consumer, Producer, RingBuffer};

use crate::util::{PlaybackClock, mono_ns};

/// Announces one block of samples in the sample ring.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Mark {
    /// Seek epoch the block belongs to.
    pub epoch: u32,
    /// Stereo frames in the block.
    pub frames: u32,
    /// Media time (seconds) of the first / one past the last frame.
    pub pts_start: f64,
    pub pts_end: f64,
    /// Last block of the stream.
    pub eof: bool,
}

/// State shared between the renderer (real-time thread), the decoder and the controller.
pub(crate) struct AudioShared {
    /// Current epoch; blocks carrying another epoch are discarded.
    pub epoch: AtomicU32,
    /// `true` while sound may be produced (playing); closing it fades the output out.
    pub gate: AtomicBool,
    gain: AtomicF32,
    muted: AtomicBool,
    night: AtomicBool,
    eq_version: AtomicU32,
    eq_slot: Mutex<EqSettings>,
    spectrum_on: AtomicBool,
    /// Epoch whose final block was consumed, and when (monotonic ns) it will have been heard.
    pub ended_epoch: AtomicU32,
    pub ended_at_ns: AtomicU64,
    /// Decoder → controller: frames produced for `produced_epoch` (pre-roll detection).
    pub produced_epoch: AtomicU32,
    pub produced_frames: AtomicU64,
    pub consumed_frames: AtomicU64,
    pub produced_eof: AtomicBool,
    pub callbacks: AtomicU64,
    pub underruns: AtomicU64,
    /// Last measured output latency in microseconds.
    pub latency_us: AtomicU32,
    /// Set by the device error callback (unplugged device…).
    pub device_error: AtomicBool,
    pub sample_rate: u32,
    clock: Arc<PlaybackClock>,
}

impl AudioShared {
    fn new(sample_rate: u32, clock: Arc<PlaybackClock>) -> Self {
        Self {
            epoch: AtomicU32::new(1),
            gate: AtomicBool::new(false),
            gain: AtomicF32::new(1.0),
            muted: AtomicBool::new(false),
            night: AtomicBool::new(false),
            eq_version: AtomicU32::new(0),
            eq_slot: Mutex::new(EqSettings::default()),
            spectrum_on: AtomicBool::new(false),
            ended_epoch: AtomicU32::new(0),
            ended_at_ns: AtomicU64::new(0),
            produced_epoch: AtomicU32::new(0),
            produced_frames: AtomicU64::new(0),
            consumed_frames: AtomicU64::new(0),
            produced_eof: AtomicBool::new(false),
            callbacks: AtomicU64::new(0),
            underruns: AtomicU64::new(0),
            latency_us: AtomicU32::new(0),
            device_error: AtomicBool::new(false),
            sample_rate,
            clock,
        }
    }

    /// Sets the UI volume (`0.0 ..= 2.0`).
    pub fn set_volume(&self, v: f32) {
        self.gain.store(volume_to_gain(v), Ordering::Relaxed);
    }

    pub fn set_muted(&self, m: bool) {
        self.muted.store(m, Ordering::Relaxed);
    }

    pub fn set_night(&self, on: bool) {
        self.night.store(on, Ordering::Relaxed);
    }

    pub fn set_eq(&self, s: &EqSettings) {
        *self.eq_slot.lock() = s.clone();
        self.eq_version.fetch_add(1, Ordering::Release);
    }

    pub fn set_spectrum(&self, on: bool) {
        self.spectrum_on.store(on, Ordering::Relaxed);
    }

    pub fn latency(&self) -> Duration {
        Duration::from_micros(u64::from(self.latency_us.load(Ordering::Relaxed)))
    }

    /// Moves to epoch `e` (everything queued so far becomes stale).
    pub fn buffered_ms(&self) -> f32 {
        let prod = self.produced_frames.load(Ordering::Relaxed);
        let cons = self.consumed_frames.load(Ordering::Relaxed);
        let queued = prod.saturating_sub(cons);
        (queued as f64 * 1000.0 / f64::from(self.sample_rate.max(1))) as f32
    }

    pub fn begin_epoch(&self, e: u32) {
        self.produced_frames.store(0, Ordering::Release);
        self.consumed_frames.store(0, Ordering::Release);
        self.produced_eof.store(false, Ordering::Release);
        self.produced_epoch.store(e, Ordering::Release);
        self.ended_at_ns.store(0, Ordering::Release);
        self.epoch.store(e, Ordering::Release);
    }

    /// Convenience for tests: starts the epoch after the current one and returns it.
    #[cfg(test)]
    pub fn next_epoch(&self) -> u32 {
        let e = self.epoch.load(Ordering::Acquire).wrapping_add(1).max(1);
        self.begin_epoch(e);
        e
    }

    /// `true` when the final block of `epoch` has been consumed *and* heard.
    pub fn finished(&self, epoch: u32) -> bool {
        self.ended_epoch.load(Ordering::Acquire) == epoch && mono_ns() >= self.ended_at_ns.load(Ordering::Acquire)
    }
}

/// Producer side of the rings, used by the audio decoder thread.
pub(crate) struct AudioLink {
    samples: Producer<f32>,
    marks: Producer<Mark>,
    capacity_frames: usize,
}

impl AudioLink {
    /// Frames of free space in the sample ring.
    pub fn free_frames(&self) -> usize {
        self.samples.slots() / 2
    }

    /// Frames currently waiting in the ring (stale ones included).
    pub fn queued_frames(&self) -> usize {
        self.capacity_frames - self.free_frames()
    }

    #[cfg(test)]
    pub fn capacity_frames(&self) -> usize {
        self.capacity_frames
    }

    /// Queues a block. Returns `false` (and queues nothing) when the rings are full.
    pub fn try_push(&mut self, block: &[f32], mark: Mark) -> bool {
        debug_assert_eq!(block.len(), mark.frames as usize * 2);
        if self.marks.is_full() || self.samples.slots() < block.len() {
            return false;
        }
        // Samples first: the consumer only looks at the data once the mark is visible.
        if !block.is_empty() && self.samples.push_entire_slice(block).is_err() {
            return false;
        }
        self.marks.push(mark).is_ok()
    }
}

struct Cur {
    epoch: u32,
    total: u32,
    left: u32,
    pts_start: f64,
    pts_end: f64,
    eof: bool,
}

/// The real-time half: turns queued blocks into device samples.
pub(crate) struct Renderer {
    shared: Arc<AudioShared>,
    samples: Consumer<f32>,
    marks: Consumer<Mark>,
    cur: Option<Cur>,
    eq: Equalizer,
    night: Compressor,
    eq_version: u32,
    vol_gain: f32,
    play_gain: f32,
    fade_step: f32,
    anchored: bool,
    tap: Option<Producer<f32>>,
    /// Mono mix handed to the spectrum tap.
    mono: Vec<f32>,
    /// Stereo mix produced for the device callback.
    out_buf: Vec<f32>,
    /// Epoch whose final block has been consumed (an empty ring afterwards is not an underrun).
    eof_done_epoch: u32,
    rate: f64,
}

/// Creates the two halves of a ring pair able to hold `ring_frames` stereo frames.
pub(crate) fn make_pair(shared: Arc<AudioShared>, ring_frames: usize, tap: Option<Producer<f32>>) -> (AudioLink, Renderer) {
    let ring_frames = ring_frames.max(1024);
    let (sp, sc) = RingBuffer::new(ring_frames * 2);
    let (mp, mc) = RingBuffer::new(512);
    let rate = shared.sample_rate;
    let link = AudioLink { samples: sp, marks: mp, capacity_frames: ring_frames };
    let renderer = Renderer {
        eq: Equalizer::new(rate),
        night: Compressor::night(rate),
        eq_version: 0,
        vol_gain: shared.gain.load(Ordering::Relaxed),
        play_gain: 0.0,
        fade_step: 1.0 / (rate as f32 * 0.008).max(1.0),
        anchored: false,
        cur: None,
        samples: sc,
        marks: mc,
        tap,
        mono: Vec::with_capacity(8192),
        out_buf: Vec::with_capacity(16384),
        eof_done_epoch: 0,
        rate: f64::from(rate),
        shared,
    };
    (link, renderer)
}

impl Renderer {
    fn discard(&mut self, n_samples: usize) {
        let n = n_samples.min(self.samples.slots());
        if n > 0
            && let Ok(c) = self.samples.read_chunk(n)
        {
            c.commit_all();
        }
    }

    fn read_into(&mut self, dst: &mut [f32]) -> bool {
        match self.samples.read_chunk(dst.len()) {
            Ok(chunk) => {
                let (a, b) = chunk.as_slices();
                dst[..a.len()].copy_from_slice(a);
                dst[a.len()..].copy_from_slice(b);
                chunk.commit_all();
                true
            }
            Err(_) => false,
        }
    }

    /// Drops the blocks of older epochs waiting at the head of the ring, so the decoder can refill
    /// it after a seek even while the output is silent (paused, buffering).
    fn drop_stale(&mut self, epoch: u32) {
        while let Ok(m) = self.marks.peek().copied() {
            if m.epoch == epoch {
                break;
            }
            let _ = self.marks.pop();
            self.discard(m.frames as usize * 2);
        }
    }

    /// Pops the next block of the current epoch, dropping stale ones on the way.
    fn next_block(&mut self, epoch: u32) -> Option<Cur> {
        loop {
            let m = *self.marks.peek().ok()?;
            let _ = self.marks.pop();
            if m.epoch != epoch {
                self.discard(m.frames as usize * 2);
                continue;
            }
            return Some(Cur { epoch: m.epoch, total: m.frames, left: m.frames, pts_start: m.pts_start, pts_end: m.pts_end, eof: m.eof });
        }
    }

    fn sync_eq(&mut self) {
        let v = self.shared.eq_version.load(Ordering::Acquire);
        if v != self.eq_version
            && let Some(s) = self.shared.eq_slot.try_lock()
        {
            self.eq.apply(&s);
            self.eq_version = v;
        }
    }

    /// Anchors the master clock on the sample that will be heard `at_ns`.
    fn anchor(&mut self, pos: f64, at_ns: u64, rate: f64) {
        let clock = &self.shared.clock;
        let mut p = pos;
        if self.anchored {
            let old = clock.anchor();
            if old.rate != 0.0 {
                let predicted = old.pts + (at_ns as f64 - old.at_ns as f64) * 1e-9 * old.rate;
                let err = pos - predicted;
                // Smooth out the jitter of the backend's latency estimate; jump on real changes.
                if err.abs() < 0.020 {
                    p = predicted + err * 0.25;
                }
            }
        }
        if clock.try_set_at(p, at_ns, rate) {
            self.anchored = true;
        }
    }

    /// Fills `out` (interleaved stereo) with the next `out.len() / 2` frames.
    pub fn render_stereo(&mut self, out: &mut [f32], latency_ns: u64) {
        self.render_stereo_at(out, mono_ns(), latency_ns);
    }

    /// Same, with an explicit "now" (monotonic ns) so tests can simulate real time.
    pub(crate) fn render_stereo_at(&mut self, out: &mut [f32], now_ns: u64, latency_ns: u64) {
        debug_assert!(out.len().is_multiple_of(2));
        let sh = Arc::clone(&self.shared);
        let frames = out.len() / 2;
        sh.callbacks.fetch_add(1, Ordering::Relaxed);
        sh.latency_us.store((latency_ns / 1000).min(u64::from(u32::MAX)) as u32, Ordering::Relaxed);
        let epoch = sh.epoch.load(Ordering::Acquire);
        let gate = sh.gate.load(Ordering::Acquire);

        // A block that was half played when a seek happened is dropped.
        if self.cur.as_ref().is_some_and(|c| c.epoch != epoch) {
            let left = self.cur.take().map_or(0, |c| c.left as usize);
            self.discard(left * 2);
        }
        if !gate {
            self.anchored = false;
        }
        if !gate && self.play_gain <= 0.0 {
            self.drop_stale(epoch);
        }
        self.sync_eq();

        let mut written = 0usize;
        let mut underrun = false;
        if gate || self.play_gain > 0.0 {
            while written < frames {
                if self.cur.is_none() {
                    match self.next_block(epoch) {
                        // A block without samples only carries the end-of-stream flag.
                        Some(c) if c.total == 0 => {
                            if c.eof {
                                let at = now_ns + latency_ns + (written as f64 / self.rate * 1e9) as u64;
                                sh.ended_at_ns.store(at, Ordering::Release);
                                sh.ended_epoch.store(c.epoch, Ordering::Release);
                                self.eof_done_epoch = c.epoch;
                            }
                            continue;
                        }
                        Some(c) => self.cur = Some(c),
                        None => {
                            underrun = gate && self.eof_done_epoch != epoch;
                            break;
                        }
                    }
                }
                let (n, pos, rate) = {
                    let c = self.cur.as_ref().expect("block present");
                    let n = (c.left as usize).min(frames - written);
                    let span = c.pts_end - c.pts_start;
                    let done = f64::from(c.total - c.left);
                    let pos = c.pts_start + span * (done / f64::from(c.total));
                    let rate = span / (f64::from(c.total) / self.rate);
                    (n, pos, rate)
                };
                if !self.read_into(&mut out[written * 2..(written + n) * 2]) {
                    self.cur = None;
                    underrun = gate;
                    break;
                }
                if !self.anchored || written == 0 {
                    let at = now_ns + latency_ns + (written as f64 / self.rate * 1e9) as u64;
                    self.anchor(pos, at, rate);
                }
                let c = self.cur.as_mut().expect("block present");
                c.left -= n as u32;
                written += n;
                if c.left == 0 {
                    if c.eof {
                        let at = now_ns + latency_ns + (written as f64 / self.rate * 1e9) as u64;
                        sh.ended_at_ns.store(at, Ordering::Release);
                        sh.ended_epoch.store(c.epoch, Ordering::Release);
                        self.eof_done_epoch = c.epoch;
                    }
                    self.cur = None;
                }
            }
        }
        if underrun {
            self.anchored = false;
            sh.underruns.fetch_add(1, Ordering::Relaxed);
        }
        if written > 0 {
            sh.consumed_frames.fetch_add(written as u64, Ordering::Relaxed);
        }
        out[written * 2..].fill(0.0);

        // --- processing of the audible part ---------------------------------------------------
        let pend = if gate { 1.0f32 } else { 0.0 };
        let pstep = if gate { self.fade_step } else { -self.fade_step };
        let vt = if sh.muted.load(Ordering::Relaxed) { 0.0 } else { sh.gain.load(Ordering::Relaxed) };
        if written > 0 {
            let audible = &mut out[..written * 2];
            self.eq.process_stereo(audible);
            self.night.set_enabled(sh.night.load(Ordering::Relaxed));
            self.night.process_stereo(audible);
            if sh.spectrum_on.load(Ordering::Relaxed)
                && let Some(tap) = self.tap.as_mut()
            {
                self.mono.clear();
                self.mono.extend(audible.chunks_exact(2).map(|f| 0.5 * (f[0] + f[1])));
                let _ = tap.push_partial_slice(&self.mono);
            }
            let mut vg = self.vol_gain;
            let vstep = (vt - vg) / written as f32;
            let mut pg = self.play_gain;
            if vg == 1.0 && vt == 1.0 && pg == 1.0 && pend == 1.0 {
                for s in audible.iter_mut() {
                    *s = soft_clip(*s);
                }
            } else {
                for f in audible.chunks_exact_mut(2) {
                    vg += vstep;
                    pg = (pg + pstep).clamp(0.0, 1.0);
                    let g = vg * pg;
                    f[0] = soft_clip(f[0] * g);
                    f[1] = soft_clip(f[1] * g);
                }
                self.play_gain = pg;
            }
            self.vol_gain = vt;
        } else {
            self.vol_gain = vt;
        }
        if written < frames {
            let rest = (frames - written) as f32;
            self.play_gain = if gate { (self.play_gain + rest * self.fade_step).min(pend) } else { (self.play_gain - rest * self.fade_step).max(pend) };
        } else if written > 0 && !(self.play_gain == 1.0 && pend == 1.0) {
            self.play_gain = self.play_gain.clamp(0.0, 1.0);
        }
    }

    /// Device callback body for any sample type / channel count.
    fn callback<T: SizedSample + FromSample<f32>>(&mut self, data: &mut [T], channels: usize, floor_latency: Duration, info: &cpal::OutputCallbackInfo) {
        let ts = info.timestamp();
        let reported = ts.playback.duration_since(ts.callback);
        let frames = data.len() / channels.max(1);
        let period = Duration::from_secs_f64(frames as f64 / self.rate);
        // Some backends report no latency at all; a buffer is never heard sooner than one period.
        let latency = reported.max(period).max(floor_latency).min(Duration::from_millis(500));
        let mut stereo = std::mem::take(&mut self.out_buf);
        stereo.clear();
        stereo.resize(frames * 2, 0.0);
        self.render_stereo(&mut stereo, latency.as_nanos() as u64);
        match channels {
            2 => {
                for (d, s) in data.iter_mut().zip(stereo.iter()) {
                    *d = T::from_sample(*s);
                }
            }
            1 => {
                for (d, f) in data.iter_mut().zip(stereo.chunks_exact(2)) {
                    *d = T::from_sample(0.5 * (f[0] + f[1]));
                }
            }
            n => {
                for (d, f) in data.chunks_exact_mut(n).zip(stereo.chunks_exact(2)) {
                    d[0] = T::from_sample(f[0]);
                    d[1] = T::from_sample(f[1]);
                    for x in &mut d[2..] {
                        *x = T::EQUILIBRIUM;
                    }
                }
            }
        }
        self.out_buf = stereo;
    }
}

// -------------------------------------------------------------------------------------------------
// Devices
// -------------------------------------------------------------------------------------------------

/// An output device the user can pick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputDeviceInfo {
    /// Stable identifier (`host:device`), usable with [`AudioDevice::open`].
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

/// Lists the output devices of the default audio host.
pub fn list_output_devices() -> Vec<OutputDeviceInfo> {
    let host = cpal::default_host();
    let default = host.default_output_device();
    let mut out = Vec::new();
    let Ok(devices) = host.output_devices() else { return out };
    for d in devices {
        let name = d.description().map(|x| x.name().to_owned()).unwrap_or_else(|_| "?".into());
        let id = d.id().map(|i| i.to_string()).unwrap_or_else(|_| name.clone());
        let is_default = default.as_ref().is_some_and(|x| *x == d);
        out.push(OutputDeviceInfo { id, name, is_default });
    }
    out
}

enum Driver {
    Cpal(#[allow(dead_code)] cpal::Stream),
    Virtual { stop: Arc<AtomicBool>, thread: Option<JoinHandle<()>> },
}

/// A running audio output (real or virtual) bound to a playback clock.
pub(crate) struct AudioDevice {
    pub shared: Arc<AudioShared>,
    pub link: Arc<Mutex<AudioLink>>,
    pub tap: Arc<Mutex<Consumer<f32>>>,
    pub rate: u32,
    pub name: String,
    pub is_virtual: bool,
    driver: Driver,
}

impl Drop for AudioDevice {
    fn drop(&mut self) {
        if let Driver::Virtual { stop, thread } = &mut self.driver {
            stop.store(true, Ordering::Release);
            if let Some(t) = thread.take() {
                let _ = t.join();
            }
        }
    }
}

const RING_SECONDS: f64 = 0.75;
const VIRTUAL_RATE: u32 = 48_000;
const VIRTUAL_LATENCY: Duration = Duration::from_millis(20);

fn rank(f: SampleFormat) -> u32 {
    match f {
        SampleFormat::F32 => 0,
        SampleFormat::I32 => 1,
        SampleFormat::I16 => 2,
        SampleFormat::F64 => 3,
        _ => 4,
    }
}

fn pick_config(dev: &cpal::Device) -> Option<cpal::SupportedStreamConfig> {
    let def = dev.default_output_config().ok();
    let rate = def.as_ref().map(|d| d.sample_rate());
    let mut best: Option<cpal::SupportedStreamConfig> = None;
    if let (Ok(ranges), Some(rate)) = (dev.supported_output_configs(), rate) {
        for r in ranges {
            if r.channels() != 2 {
                continue;
            }
            let Some(c) = r.try_with_sample_rate(rate) else { continue };
            if best.as_ref().is_none_or(|b| rank(c.sample_format()) < rank(b.sample_format())) {
                best = Some(c);
            }
        }
    }
    best.or(def)
}

fn find_device(host: &cpal::Host, wanted: Option<&str>) -> Option<cpal::Device> {
    if let Some(w) = wanted.filter(|w| !w.is_empty()) {
        if let Ok(id) = w.parse::<cpal::DeviceId>()
            && let Some(d) = host.device_by_id(&id)
        {
            return Some(d);
        }
        if let Ok(list) = host.output_devices() {
            for d in list {
                if d.description().is_ok_and(|x| x.name() == w) {
                    return Some(d);
                }
            }
        }
        tracing::warn!("audio device {w:?} not found, using the default one");
    }
    host.default_output_device()
}

macro_rules! build_for {
    ($fmt:expr, $dev:expr, $cfg:expr, $renderer:expr, $channels:expr, $latency:expr, $err:expr, [$($variant:ident => $ty:ty),*]) => {{
        let mut renderer = $renderer;
        let channels = $channels;
        let latency = $latency;
        match $fmt {
            $(SampleFormat::$variant => $dev.build_output_stream::<$ty, _, _>(
                $cfg,
                move |data: &mut [$ty], info: &cpal::OutputCallbackInfo| renderer.callback(data, channels, latency, info),
                $err,
                None,
            ),)*
            other => {
                tracing::warn!("unsupported sample format {other:?}");
                return Err(format!("unsupported sample format {other:?}"));
            }
        }
    }};
}

impl AudioDevice {
    /// Opens `preferred` (or the default device). Falls back to the virtual device when the real
    /// one cannot be opened, or when `force_virtual` is set. Never fails.
    pub fn open(preferred: Option<&str>, clock: Arc<PlaybackClock>, force_virtual: bool) -> AudioDevice {
        if !force_virtual {
            match Self::open_cpal(preferred, clock.clone()) {
                Ok(d) => return d,
                Err(e) => tracing::warn!("no usable audio device ({e}); using the virtual device"),
            }
        }
        Self::open_virtual(clock)
    }

    fn open_cpal(preferred: Option<&str>, clock: Arc<PlaybackClock>) -> Result<AudioDevice, String> {
        let host = cpal::default_host();
        let dev = find_device(&host, preferred).ok_or("no output device")?;
        let name = dev.description().map(|d| d.name().to_owned()).unwrap_or_else(|_| "default".into());
        let supported = pick_config(&dev).ok_or("device reports no output configuration")?;
        let format = supported.sample_format();
        let cfg = supported.config();
        let rate = cfg.sample_rate;
        let channels = usize::from(cfg.channels);
        if channels == 0 || rate < 8000 {
            return Err(format!("unusable configuration ({channels} ch, {rate} Hz)"));
        }
        let shared = Arc::new(AudioShared::new(rate, clock.clone()));
        let (tap_p, tap_c) = RingBuffer::new(16384);
        let ring = (f64::from(rate) * RING_SECONDS) as usize;
        let (link, renderer) = make_pair(shared.clone(), ring, Some(tap_p));
        let err_shared = shared.clone();
        let err = move |e: cpal::Error| {
            tracing::error!("audio stream error: {e}");
            err_shared.device_error.store(true, Ordering::Release);
        };
        let stream = build_for!(
            format, dev, cfg, renderer, channels, Duration::ZERO, err,
            [F32 => f32, F64 => f64, I16 => i16, I32 => i32, I8 => i8, U8 => u8, U16 => u16, U32 => u32, I64 => i64, U64 => u64]
        )
        .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        clock.set_audio_driven(true);
        tracing::info!("audio output: {name} — {rate} Hz, {channels} ch, {format:?}");
        Ok(AudioDevice { shared, link: Arc::new(Mutex::new(link)), tap: Arc::new(Mutex::new(tap_c)), rate, name, is_virtual: false, driver: Driver::Cpal(stream) })
    }

    fn open_virtual(clock: Arc<PlaybackClock>) -> AudioDevice {
        let rate = VIRTUAL_RATE;
        let shared = Arc::new(AudioShared::new(rate, clock.clone()));
        let (tap_p, tap_c) = RingBuffer::new(16384);
        let ring = (f64::from(rate) * RING_SECONDS) as usize;
        let (link, mut renderer) = make_pair(shared.clone(), ring, Some(tap_p));
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let thread = thread::Builder::new()
            .name("400_media_player-vaudio".into())
            .spawn(move || {
                let period = Duration::from_millis(10);
                let frames = (f64::from(rate) * period.as_secs_f64()) as usize;
                let mut buf = vec![0.0f32; frames * 2];
                let mut next = Instant::now();
                while !stop2.load(Ordering::Acquire) {
                    renderer.render_stereo(&mut buf, VIRTUAL_LATENCY.as_nanos() as u64);
                    next += period;
                    let now = Instant::now();
                    if next > now {
                        thread::sleep(next - now);
                    } else if now - next > Duration::from_millis(200) {
                        next = now;
                    }
                }
            })
            .expect("spawn virtual audio thread");
        clock.set_audio_driven(true);
        AudioDevice {
            shared,
            link: Arc::new(Mutex::new(link)),
            tap: Arc::new(Mutex::new(tap_c)),
            rate,
            name: "Virtual (no sound card)".into(),
            is_virtual: true,
            driver: Driver::Virtual { stop, thread: Some(thread) },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;

    fn setup() -> (Arc<AudioShared>, Arc<PlaybackClock>, AudioLink, Renderer) {
        let clock = Arc::new(PlaybackClock::new());
        clock.set_audio_driven(true);
        clock.release();
        let shared = Arc::new(AudioShared::new(SR, clock.clone()));
        let (link, renderer) = make_pair(shared.clone(), 48_000, None);
        (shared, clock, link, renderer)
    }

    /// A block of `frames` frames whose left channel is a ramp starting at `v0`.
    fn block(frames: usize, v0: f32) -> Vec<f32> {
        (0..frames).flat_map(|i| [v0 + i as f32 * 1e-6, -(v0 + i as f32 * 1e-6)]).collect()
    }

    fn mark(epoch: u32, frames: u32, pts: f64, eof: bool) -> Mark {
        Mark { epoch, frames, pts_start: pts, pts_end: pts + f64::from(frames) / f64::from(SR), eof }
    }

    /// Plays `segments` of (level in dBFS, seconds) as a 440 Hz tone through the renderer, in 10 ms
    /// callbacks, and returns everything that came out (interleaved stereo).
    fn play_tone(night: bool, segments: &[(f32, usize)]) -> Vec<f32> {
        let (sh, _clock, mut link, mut r) = setup();
        sh.set_night(night);
        sh.gate.store(true, Ordering::Release);
        let e = sh.epoch.load(Ordering::Relaxed);
        let (mut played, mut t, mut index) = (Vec::new(), 5_000_000_000u64, 0usize);
        for &(db, seconds) in segments {
            let amp = 10f32.powf(db / 20.0);
            for _ in 0..seconds * 100 {
                let blk: Vec<f32> = (0..480)
                    .flat_map(|n| {
                        let v = amp * (2.0 * std::f32::consts::PI * 440.0 * (index * 480 + n) as f32 / SR as f32).sin();
                        [v, v]
                    })
                    .collect();
                assert!(link.try_push(&blk, mark(e, 480, index as f64 * 0.01, false)));
                let mut out = vec![0.0f32; 960];
                r.render_stereo_at(&mut out, t, 30_000_000);
                played.extend(out);
                t += 10_000_000;
                index += 1;
            }
        }
        played
    }

    fn peak_db_between(samples: &[f32], from_s: f32, to_s: f32) -> f32 {
        let a = (from_s * SR as f32) as usize * 2;
        let b = (to_s * SR as f32) as usize * 2;
        20.0 * samples[a..b].iter().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-9).log10()
    }

    #[test]
    fn night_mode_squeezes_the_dynamic_range_and_off_leaves_the_sound_alone() {
        let segments = [(-40.0, 3), (0.0, 2)];
        let plain = play_tone(false, &segments);
        let night = play_tone(true, &segments);
        // Without it: untouched (the loud tone only meets the soft clipper's gentle shoulder).
        assert!((peak_db_between(&plain, 1.5, 2.5) - -40.0).abs() < 0.2);
        assert!(peak_db_between(&plain, 4.0, 5.0) > -1.0);
        // With it: whispers up by 6 dB, the bang down to about −12 dBFS.
        let quiet = peak_db_between(&night, 1.5, 2.5);
        let loud = peak_db_between(&night, 4.0, 5.0);
        assert!((quiet - -34.0).abs() < 1.0, "quiet {quiet}");
        assert!((loud - -12.0).abs() < 1.5, "loud {loud}");
    }

    #[test]
    fn night_mode_can_be_switched_while_playing() {
        let (sh, _clock, mut link, mut r) = setup();
        sh.gate.store(true, Ordering::Release);
        let e = sh.epoch.load(Ordering::Relaxed);
        let mut t = 5_000_000_000u64;
        let mut peaks = Vec::new();
        for i in 0..300 {
            if i == 100 {
                sh.set_night(true);
            }
            if i == 200 {
                sh.set_night(false);
            }
            let blk: Vec<f32> = (0..480)
                .flat_map(|n| {
                    let v = 0.5 * (2.0 * std::f32::consts::PI * 440.0 * (i * 480 + n) as f32 / SR as f32).sin();
                    [v, v]
                })
                .collect();
            assert!(link.try_push(&blk, mark(e, 480, i as f64 * 0.01, false)));
            let mut out = vec![0.0f32; 960];
            r.render_stereo_at(&mut out, t, 30_000_000);
            t += 10_000_000;
            peaks.push(out.iter().fold(0.0f32, |m, v| m.max(v.abs())));
        }
        // Off → on → off: the level glides, it never jumps by more than a few dB between callbacks.
        for w in peaks[20..].windows(2) {
            let step = (w[1].max(1e-6) / w[0].max(1e-6)).log10().abs() * 20.0;
            assert!(step < 3.0, "a jump of {step} dB between two callbacks");
        }
        assert!((peaks[299] - 0.5).abs() < 0.01, "back to the untouched sound: {}", peaks[299]);
    }

    #[test]
    fn plays_blocks_in_order_and_anchors_the_clock() {
        let (sh, clock, mut link, mut r) = setup();
        let e = sh.epoch.load(Ordering::Relaxed);
        for i in 0..4 {
            assert!(link.try_push(&block(960, 0.1 + i as f32 * 0.01), mark(e, 960, 10.0 + i as f64 * 0.02, false)));
        }
        sh.gate.store(true, Ordering::Release);
        // Simulated real time: a 480-frame callback every 10 ms.
        let t0 = 5_000_000_000u64;
        let lat = 30_000_000u64;
        // First callback: 480 frames. The play-gain fade-in affects only the first ~8 ms.
        let mut out = vec![0.0f32; 960];
        r.render_stereo_at(&mut out, t0, lat);
        let a = clock.anchor();
        assert!((a.pts - 10.0).abs() < 1e-9, "{a:?}");
        assert_eq!(a.at_ns, t0 + lat);
        assert!((a.rate - 1.0).abs() < 1e-9);
        // Second callback finishes block 0: unity gain → the ramp comes through bit-exact.
        let mut out2 = vec![0.0f32; 960];
        r.render_stereo_at(&mut out2, t0 + 10_000_000, lat);
        let expect = block(960, 0.1);
        for i in 0..480 {
            assert!((out2[2 * i] - expect[2 * (480 + i)]).abs() < 1e-6, "frame {i}");
            assert!((out2[2 * i + 1] - expect[2 * (480 + i) + 1]).abs() < 1e-6);
        }
        // Third callback starts block 1 → the clock follows the stream position.
        let mut out3 = vec![0.0f32; 960];
        r.render_stereo_at(&mut out3, t0 + 20_000_000, lat);
        let a = clock.anchor();
        assert!((a.pts - 10.02).abs() < 1e-6, "{a:?}");
        // …and it reads back as "now": 30 ms of latency still to go.
        assert!((a.at_ns as i64 - (t0 + 20_000_000 + lat) as i64).abs() < 10, "{a:?}");
        assert_eq!(sh.underruns.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn stale_epochs_are_dropped_silently() {
        let (sh, _clock, mut link, mut r) = setup();
        let old = sh.epoch.load(Ordering::Relaxed);
        for _ in 0..10 {
            assert!(link.try_push(&block(960, 0.5), mark(old, 960, 3.0, false)));
        }
        sh.gate.store(true, Ordering::Release);
        let mut out = vec![0.0f32; 960];
        r.render_stereo(&mut out, 0);
        assert!(out.iter().any(|v| *v != 0.0));
        // Seek: new epoch, old data must never be heard again.
        let new = sh.next_epoch();
        let mut out = vec![1.0f32; 960];
        r.render_stereo(&mut out, 0);
        assert!(out.iter().all(|v| *v == 0.0), "stale audio leaked");
        assert_eq!(link.queued_frames(), 0, "stale blocks were not drained");
        // Fresh data of the new epoch plays.
        assert!(link.try_push(&block(960, 0.25), mark(new, 960, 42.0, false)));
        let mut out = vec![0.0f32; 960];
        r.render_stereo(&mut out, 0);
        assert!(out.iter().any(|v| *v != 0.0));
    }

    #[test]
    fn stale_blocks_are_drained_even_while_paused() {
        // Regression: a seek while paused left the old read-ahead in the ring, which blocked the
        // decoder (it only refills below its fill target) and froze the player in "buffering".
        let (sh, _clock, mut link, mut r) = setup();
        let old = sh.epoch.load(Ordering::Relaxed);
        for _ in 0..12 {
            assert!(link.try_push(&block(960, 0.5), mark(old, 960, 3.0, false)));
        }
        // Gate closed and fully faded: the device only asks for silence.
        let mut out = vec![0.0f32; 960];
        r.render_stereo(&mut out, 0);
        assert!(link.queued_frames() > 0, "paused output must keep the current epoch's data");
        let new = sh.next_epoch();
        r.render_stereo(&mut out, 0);
        assert_eq!(link.queued_frames(), 0, "stale blocks stayed in the ring");
        // New-epoch data queued meanwhile is kept for the pre-roll…
        assert!(link.try_push(&block(960, 0.25), mark(new, 960, 42.0, false)));
        r.render_stereo(&mut out, 0);
        assert_eq!(link.queued_frames(), 960);
        // …and is what plays once the gate opens.
        sh.gate.store(true, Ordering::Release);
        r.render_stereo(&mut out, 0);
        assert!(out.iter().any(|v| *v != 0.0));
    }

    #[test]
    fn closed_gate_fades_out_and_keeps_the_data() {
        let (sh, _clock, mut link, mut r) = setup();
        let e = sh.epoch.load(Ordering::Relaxed);
        for _ in 0..8 {
            assert!(link.try_push(&block(960, 0.5), mark(e, 960, 0.0, false)));
        }
        sh.gate.store(true, Ordering::Release);
        let mut out = vec![0.0f32; 4800];
        r.render_stereo(&mut out, 0);
        let q_playing = link.queued_frames();
        sh.gate.store(false, Ordering::Release);
        let mut out = vec![0.0f32; 4800];
        r.render_stereo(&mut out, 0);
        // fade-out reaches silence (8 ms ≈ 384 frames) and stops consuming afterwards
        assert!(out[out.len() - 100..].iter().all(|v| *v == 0.0));
        let q_after_fade = link.queued_frames();
        let mut out = vec![1.0f32; 4800];
        r.render_stereo(&mut out, 0);
        assert!(out.iter().all(|v| *v == 0.0));
        assert_eq!(link.queued_frames(), q_after_fade, "paused renderer must not consume");
        assert!(q_after_fade < q_playing);
        // resume: audio comes back
        sh.gate.store(true, Ordering::Release);
        let mut out = vec![0.0f32; 4800];
        r.render_stereo(&mut out, 0);
        assert!(out.iter().any(|v| v.abs() > 0.1));
    }

    #[test]
    fn eof_block_is_reported_once_consumed() {
        let (sh, _clock, mut link, mut r) = setup();
        let e = sh.epoch.load(Ordering::Relaxed);
        assert!(link.try_push(&block(480, 0.2), mark(e, 480, 0.0, true)));
        sh.gate.store(true, Ordering::Release);
        assert!(!sh.finished(e));
        let mut out = vec![0.0f32; 1920];
        r.render_stereo(&mut out, 0);
        assert_eq!(sh.ended_epoch.load(Ordering::Relaxed), e);
        // `finished` becomes true when the last sample has left the device (here: immediately).
        thread::sleep(Duration::from_millis(15));
        assert!(sh.finished(e));
        assert_eq!(sh.underruns.load(Ordering::Relaxed), 0, "silence after the end of the stream is not an underrun");
    }

    #[test]
    fn empty_eof_marker_ends_the_stream_after_the_last_samples() {
        let (sh, _clock, mut link, mut r) = setup();
        let e = sh.epoch.load(Ordering::Relaxed);
        assert!(link.try_push(&block(480, 0.2), mark(e, 480, 0.0, false)));
        assert!(link.try_push(&[], Mark { epoch: e, frames: 0, pts_start: 0.01, pts_end: 0.01, eof: true }));
        sh.gate.store(true, Ordering::Release);
        let t0 = 1_000_000_000u64;
        let mut out = vec![0.0f32; 1920];
        r.render_stereo_at(&mut out, t0, 10_000_000);
        assert_eq!(sh.ended_epoch.load(Ordering::Relaxed), e);
        // 480 frames were played first → the end is heard 10 ms (latency) + 10 ms (480 frames) later.
        let end = sh.ended_at_ns.load(Ordering::Relaxed);
        assert!((end as i64 - (t0 + 20_000_000) as i64).abs() < 1000, "{end}");
        assert_eq!(sh.underruns.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn volume_mute_and_limiter() {
        let (sh, _clock, mut link, mut r) = setup();
        let e = sh.epoch.load(Ordering::Relaxed);
        sh.gate.store(true, Ordering::Release);
        // 0.5 volume → gain 0.25 (perceptual curve)
        sh.set_volume(0.5);
        let mut fed = 0;
        let mut out = vec![0.0f32; 960];
        for _ in 0..6 {
            while link.try_push(&vec![0.4f32; 960], mark(e, 480, fed as f64 * 0.01, false)) {
                fed += 1;
                if fed > 20 {
                    break;
                }
            }
            r.render_stereo(&mut out, 0);
        }
        let last = out[out.len() - 1];
        assert!((last - 0.1).abs() < 1e-4, "{last}");
        sh.set_muted(true);
        for _ in 0..3 {
            r.render_stereo(&mut out, 0);
        }
        assert!(out.iter().all(|v| v.abs() < 1e-6), "muted output must be silent");
        // 2× volume on a hot signal never exceeds full scale
        sh.set_muted(false);
        sh.set_volume(2.0);
        for _ in 0..30 {
            while link.try_push(&vec![0.9f32; 960], mark(e, 480, 1.0, false)) {}
            r.render_stereo(&mut out, 0);
        }
        assert!(out.iter().all(|v| v.abs() <= 1.0));
        assert!(out.iter().any(|v| v.abs() > 0.9));
    }

    #[test]
    fn rates_follow_the_block_spans() {
        let (sh, clock, mut link, mut r) = setup();
        let e = sh.epoch.load(Ordering::Relaxed);
        // A block of 960 output frames covering 40 ms of media = 2× speed.
        let m = Mark { epoch: e, frames: 960, pts_start: 5.0, pts_end: 5.04, eof: false };
        assert!(link.try_push(&block(960, 0.1), m));
        sh.gate.store(true, Ordering::Release);
        let mut out = vec![0.0f32; 960];
        r.render_stereo(&mut out, 0);
        assert!((clock.anchor().rate - 2.0).abs() < 1e-9);
    }

    #[test]
    fn ring_reports_free_space_and_refuses_overflow() {
        let (_sh, _clock, mut link, _r) = setup();
        let cap = link.capacity_frames();
        assert_eq!(link.free_frames(), cap);
        let mut n = 0;
        while link.try_push(&block(960, 0.0), mark(1, 960, 0.0, false)) {
            n += 1;
        }
        assert_eq!(n, cap / 960);
        assert!(link.free_frames() < 960);
    }

    #[test]
    fn virtual_device_plays_in_real_time() {
        let clock = Arc::new(PlaybackClock::new());
        let dev = AudioDevice::open(None, clock.clone(), true);
        assert!(dev.is_virtual);
        clock.release();
        let sh = dev.shared.clone();
        let e = sh.epoch.load(Ordering::Relaxed);
        // 0.7 s of audio in 20 ms blocks (the ring holds 0.75 s).
        {
            let mut link = dev.link.lock();
            for i in 0..35 {
                assert!(link.try_push(&block(960, 0.1), mark(e, 960, i as f64 * 0.02, false)));
            }
        }
        sh.gate.store(true, Ordering::Release);
        thread::sleep(Duration::from_millis(500));
        let pos = clock.now();
        assert!((0.3..0.6).contains(&pos), "clock = {pos}");
        assert_eq!(sh.underruns.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn buffered_ms_tracks_produced_minus_consumed() {
        let (sh, _clock, mut link, mut r) = setup();
        let e = sh.epoch.load(Ordering::Relaxed);
        sh.produced_frames.store(960, Ordering::Release);
        assert!((sh.buffered_ms() - 20.0).abs() < 0.05);
        assert!(link.try_push(&block(960, 0.2), mark(e, 960, 0.0, false)));
        sh.gate.store(true, Ordering::Release);
        let mut out = vec![0.0f32; 960]; // 480 stereo frames (10 ms)
        r.render_stereo(&mut out, 0);
        assert!((sh.buffered_ms() - 10.0).abs() < 0.05, "buffered_ms = {}", sh.buffered_ms());
        r.render_stereo(&mut out, 0);
        assert!(sh.buffered_ms().abs() < 0.05);
    }
}

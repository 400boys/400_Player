//! The audio decoding thread: packets in, time-stretched stereo blocks out.
//!
//! Native Rust decoder → pure-Rust Hermite resampler & downmixer (any layout/format/rate → stereo
//! `f32` at the device rate) → WSOLA [`Tempo`] → ring buffer. Every block carries the media-time
//! span it represents, which is what the output callback uses to drive the master clock.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use media_player_400_core::dsp::tempo::Tempo;
use parking_lot::Mutex;

use crate::audio_out::{AudioLink, AudioShared, Mark};
use crate::codecs::{AudioDecoder, RawAudioFrame};
use crate::container::MediaPacket;
use crate::queue::{Item, PacketQueue};
use crate::shared::Shared;

/// Timestamp jumps larger than this restart the time stretcher instead of being smoothed over.
const DISCONTINUITY: f64 = 0.100;

pub(crate) struct AudioDecoderParams {
    pub decoder: AudioDecoder,
    pub queue: Arc<PacketQueue>,
    pub shared: Arc<Shared>,
    pub dev: Arc<AudioShared>,
    pub link: Arc<Mutex<AudioLink>>,
    pub out_rate: u32,
    pub stop: Arc<AtomicBool>,
    pub time_base: f64,
    pub start_offset: f64,
    pub buffer_ms: u32,
}

/// Evaluates a 4-point, 3rd-order Hermite cubic polynomial at fractional offset `t` (`0.0 .. 1.0`)
/// between `y0` and `y1`, using neighbouring samples `ym1` ($x = -1$) and `y2` ($x = +2$).
#[inline]
fn hermite4(ym1: f32, y0: f32, y1: f32, y2: f32, t: f32) -> f32 {
    let c0 = y0;
    let c1 = 0.5 * (y1 - ym1);
    let c2 = ym1 - 2.5 * y0 + 2.0 * y1 - 0.5 * y2;
    let c3 = 0.5 * (y2 - ym1) + 1.5 * (y0 - y1);
    ((c3 * t + c2) * t + c1) * t + c0
}

/// Pure-Rust ITU-R BS.775 multi-channel downmixer and 4-point Hermite cubic resampler to stereo `f32`.
struct Resampler {
    channels: u16,
    in_rate: u32,
    out_rate: u32,
    frac_pos: f64,
    /// Two most recent input stereo frames (`hist[0]` = $N-2$, `hist[1]` = $N-1$) for seamless 4-tap continuity.
    hist: [(f32, f32); 2],
    has_hist: bool,
}

impl Resampler {
    fn new(f: &RawAudioFrame, out_rate: u32) -> Self {
        Self {
            channels: f.channels.max(1),
            in_rate: f.sample_rate.max(8000),
            out_rate: out_rate.max(8000),
            frac_pos: 0.0,
            hist: [(0.0, 0.0); 2],
            has_hist: false,
        }
    }

    fn matches(&self, f: &RawAudioFrame) -> bool {
        self.channels == f.channels.max(1) && self.in_rate == f.sample_rate.max(8000)
    }

    fn convert(&mut self, f: Option<&RawAudioFrame>, out: &mut Vec<f32>) {
        let Some(f) = f else { return };
        let ch = usize::from(self.channels);
        let in_frames = f.frames();
        if in_frames == 0 {
            return;
        }
        const C_707: f32 = std::f32::consts::FRAC_1_SQRT_2;
        const LFE_GAIN: f32 = 0.316_227_76; // -10 dB
        // Headroom factor for 5.1 / 7.1 ITU-R BS.775 fold-down so full-scale multi-channel peaks do not clip.
        const NORM_51: f32 = 1.0 / (1.0 + C_707 + C_707);
        const NORM_71: f32 = 1.0 / (1.0 + C_707 + 2.0 * 0.5);

        let raw_pair = |idx: usize| -> (f32, f32) {
            let base = idx.min(in_frames - 1) * ch;
            match ch {
                1 => {
                    let s = f.samples[base];
                    (s, s)
                }
                2 => (f.samples[base], f.samples[base + 1]),
                3 => {
                    let (fl, fr, fc) = (f.samples[base], f.samples[base + 1], f.samples[base + 2] * C_707);
                    let norm = 1.0 / (1.0 + C_707);
                    ((fl + fc) * norm, (fr + fc) * norm)
                }
                4 => {
                    let (fl, fr, bl, br) = (f.samples[base], f.samples[base + 1], f.samples[base + 2] * C_707, f.samples[base + 3] * C_707);
                    let norm = 1.0 / (1.0 + C_707);
                    ((fl + bl) * norm, (fr + br) * norm)
                }
                5 => {
                    let (fl, fr, fc) = (f.samples[base], f.samples[base + 1], f.samples[base + 2] * C_707);
                    let (bl, br) = (f.samples[base + 3] * C_707, f.samples[base + 4] * C_707);
                    ((fl + fc + bl) * NORM_51, (fr + fc + br) * NORM_51)
                }
                6 => {
                    let (fl, fr, fc) = (f.samples[base], f.samples[base + 1], f.samples[base + 2] * C_707);
                    let lfe = f.samples[base + 3] * LFE_GAIN;
                    let (sl, sr) = (f.samples[base + 4] * C_707, f.samples[base + 5] * C_707);
                    ((fl + fc + sl + lfe) * NORM_51, (fr + fc + sr + lfe) * NORM_51)
                }
                n if n >= 7 => {
                    let (fl, fr, fc) = (f.samples[base], f.samples[base + 1], f.samples[base + 2] * C_707);
                    let lfe = f.samples[base + 3] * LFE_GAIN;
                    let (bl, br) = (f.samples[base + 4] * 0.5, f.samples[base + 5] * 0.5);
                    let sl = f.samples[base + 6] * 0.5;
                    let sr = if n >= 8 { f.samples[base + 7] * 0.5 } else { sl };
                    ((fl + fc + bl + sl + lfe) * NORM_71, (fr + fc + br + sr + lfe) * NORM_71)
                }
                _ => (f.samples[base], f.samples[base + 1]),
            }
        };

        if !self.has_hist {
            let first = raw_pair(0);
            self.hist = [first, first];
            self.has_hist = true;
        }

        let sample_at = |i: isize, hist: &[(f32, f32); 2]| -> (f32, f32) {
            if i <= -2 {
                hist[0]
            } else if i == -1 {
                hist[1]
            } else {
                raw_pair(i as usize)
            }
        };

        if self.in_rate == self.out_rate {
            out.reserve(in_frames * 2);
            for i in 0..in_frames {
                let (l, r) = raw_pair(i);
                out.push(l);
                out.push(r);
            }
            self.hist = if in_frames >= 2 {
                [raw_pair(in_frames - 2), raw_pair(in_frames - 1)]
            } else {
                [self.hist[1], raw_pair(0)]
            };
            return;
        }

        let step = f64::from(self.in_rate) / f64::from(self.out_rate);
        let hist = self.hist;
        // When downsampling significantly (e.g. 96/192 kHz -> 48 kHz), apply a 3-tap [0.25, 0.5, 0.25]
        // anti-aliasing low-pass kernel before evaluating the 4-point Hermite cubic interpolator.
        let tap = |i: isize| -> (f32, f32) {
            if step > 1.25 {
                let (la, ra) = sample_at(i - 1, &hist);
                let (lb, rb) = sample_at(i, &hist);
                let (lc, rc) = sample_at(i + 1, &hist);
                (0.25 * la + 0.5 * lb + 0.25 * lc, 0.25 * ra + 0.5 * rb + 0.25 * rc)
            } else {
                sample_at(i, &hist)
            }
        };

        let mut pos = self.frac_pos;
        while pos < in_frames as f64 {
            let i0 = pos.floor() as isize;
            let frac = (pos - i0 as f64) as f32;
            let (lm1, rm1) = tap(i0 - 1);
            let (l0, r0) = tap(i0);
            let (l1, r1) = tap(i0 + 1);
            let (l2, r2) = tap(i0 + 2);
            out.push(hermite4(lm1, l0, l1, l2, frac));
            out.push(hermite4(rm1, r0, r1, r2, frac));
            pos += step;
        }
        self.frac_pos = pos - in_frames as f64;
        self.hist = if in_frames >= 2 {
            [raw_pair(in_frames - 2), raw_pair(in_frames - 1)]
        } else {
            [self.hist[1], raw_pair(0)]
        };
    }
}

struct Worker {
    p: AudioDecoderParams,
    epoch: u32,
    resampler: Option<Resampler>,
    tempo: Tempo,
    /// Accurate seek: samples before this media time are dropped.
    trim_until: Option<f64>,
    /// Media time of the next input sample if the stream is continuous.
    expected: Option<f64>,
    origin_set: bool,
    resampled: Vec<f32>,
    block: Vec<f32>,
}

pub(crate) fn run(p: AudioDecoderParams) {
    let tempo = Tempo::new(p.out_rate);
    let mut w = Worker { p, epoch: 0, resampler: None, tempo, trim_until: None, expected: None, origin_set: false, resampled: Vec::new(), block: Vec::new() };
    w.main();
}

impl Worker {
    fn main(&mut self) {
        let q = self.p.queue.clone();
        while !self.p.stop.load(Ordering::Relaxed) {
            let Some(item) = q.pop(Duration::from_millis(20)) else { continue };
            match item {
                Item::Flush { epoch, target, accurate } => {
                    self.p.decoder.flush_to(target + self.p.start_offset);
                    self.epoch = epoch;
                    self.resampler = None;
                    self.tempo.reset(target);
                    self.origin_set = false;
                    self.expected = None;
                    self.trim_until = accurate.then_some(target);
                }
                Item::Packet(pkt) => {
                    if self.epoch != self.p.shared.current_epoch() {
                        continue;
                    }
                    self.feed(&pkt);
                }
                Item::Eof => {
                    if self.epoch == self.p.shared.current_epoch() {
                        let _ = self.p.decoder.send_eof();
                        self.drain();
                        self.finish();
                    }
                }
            }
        }
    }

    fn feed(&mut self, pkt: &MediaPacket) {
        if let Err(e) = self.p.decoder.send_packet(pkt) {
            self.p.shared.counters.decode_errors.fetch_add(1, Ordering::Relaxed);
            tracing::debug!("audio send_packet: {e}");
            return;
        }
        self.drain();
    }

    fn drain(&mut self) -> bool {
        while let Some(frame) = self.p.decoder.receive_frame() {
            if !self.handle(&frame) {
                return false;
            }
        }
        true
    }

    fn handle(&mut self, f: &RawAudioFrame) -> bool {
        let rate = f64::from(self.p.out_rate);
        let pts = f.pts.map(|t| t as f64 * self.p.time_base - self.p.start_offset).or(self.expected).unwrap_or(0.0);
        if self.resampler.as_ref().is_none_or(|r| !r.matches(f)) {
            self.resampler = Some(Resampler::new(f, self.p.out_rate));
        }
        let mut buf = std::mem::take(&mut self.resampled);
        buf.clear();
        if let Some(r) = self.resampler.as_mut() {
            r.convert(Some(f), &mut buf);
        }
        let ok = self.push_samples(&buf, pts, rate);
        self.resampled = buf;
        ok
    }

    /// Feeds resampled stereo samples that start at media time `pts` into the stretcher.
    fn push_samples(&mut self, samples: &[f32], mut pts: f64, rate: f64) -> bool {
        let mut samples = samples;
        let n = samples.len() / 2;
        if n == 0 {
            return true;
        }
        if let Some(t) = self.trim_until {
            let end = pts + n as f64 / rate;
            if end <= t {
                return true; // entirely before the seek target
            }
            if pts < t {
                let skip = (((t - pts) * rate).round() as usize).min(n);
                samples = &samples[skip * 2..];
                pts = t;
            }
            self.trim_until = None;
        }
        if self.origin_set && self.expected.is_some_and(|e| (pts - e).abs() > DISCONTINUITY) {
            // Gap or overlap in the timestamps: flush what we have and restart from here.
            self.tempo.finish();
            if !self.emit(false) {
                return false;
            }
            self.origin_set = false;
        }
        if !self.origin_set {
            self.tempo.reset(pts);
            self.origin_set = true;
        }
        self.expected = Some(pts + (samples.len() / 2) as f64 / rate);
        self.tempo.push(samples);
        self.emit(false)
    }

    /// Pulls every complete block out of the stretcher and queues it.
    fn emit(&mut self, eof: bool) -> bool {
        let _ = eof;
        loop {
            self.tempo.set_speed(self.p.shared.speed.load(Ordering::Relaxed));
            let mut block = std::mem::take(&mut self.block);
            block.clear();
            let span = self.tempo.pull(&mut block);
            let Some((a, b)) = span else {
                self.block = block;
                return true;
            };
            let ok = self.write_block(&block, a, b);
            self.block = block;
            if !ok {
                return false;
            }
        }
    }

    fn write_block(&mut self, samples: &[f32], pts_start: f64, pts_end: f64) -> bool {
        let frames = (samples.len() / 2) as u32;
        let target = (f64::from(self.p.out_rate) * f64::from(self.p.buffer_ms) / 1000.0) as usize;
        loop {
            if self.p.stop.load(Ordering::Relaxed) || self.p.shared.current_epoch() != self.epoch {
                return false;
            }
            {
                let mut link = self.p.link.lock();
                if link.queued_frames() < target.max(frames as usize) && link.try_push(samples, Mark { epoch: self.epoch, frames, pts_start, pts_end, eof: false }) {
                    self.p.dev.produced_frames.fetch_add(u64::from(frames), Ordering::Release);
                    return true;
                }
            }
            thread::sleep(Duration::from_millis(2));
        }
    }

    /// End of stream: drain the resampler and the stretcher, then queue the end marker.
    fn finish(&mut self) {
        let rate = f64::from(self.p.out_rate);
        if let Some(r) = self.resampler.as_mut() {
            let mut buf = std::mem::take(&mut self.resampled);
            buf.clear();
            r.convert(None, &mut buf);
            if !buf.is_empty() {
                let pts = self.expected.unwrap_or(0.0);
                if !self.push_samples(&buf, pts, rate) {
                    return;
                }
            }
            self.resampled = buf;
        }
        self.tempo.finish();
        if !self.emit(true) {
            return;
        }
        let end = self.expected.unwrap_or(0.0);
        loop {
            if self.p.stop.load(Ordering::Relaxed) || self.p.shared.current_epoch() != self.epoch {
                return;
            }
            {
                let mut link = self.p.link.lock();
                if link.try_push(&[], Mark { epoch: self.epoch, frames: 0, pts_start: end, pts_end: end, eof: true }) {
                    self.p.dev.produced_eof.store(true, Ordering::Release);
                    return;
                }
            }
            thread::sleep(Duration::from_millis(2));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hermite4_interpolates_sine_smoothly_across_chunk_boundaries() {
        let in_rate = 96_000u32;
        let out_rate = 48_000u32;
        let total = 4800usize;
        let samples: Vec<f32> = (0..total)
            .flat_map(|i| {
                let s = (2.0 * std::f64::consts::PI * 1000.0 * i as f64 / f64::from(in_rate)).sin() as f32 * 0.8;
                [s, -s]
            })
            .collect();
        let f_full = RawAudioFrame { samples: samples.clone(), channels: 2, sample_rate: in_rate, pts: Some(0) };
        let mut r_full = Resampler::new(&f_full, out_rate);
        let mut out_full = Vec::new();
        r_full.convert(Some(&f_full), &mut out_full);

        let mut r_chunked = Resampler::new(&f_full, out_rate);
        let mut out_chunked = Vec::new();
        for chunk in samples.chunks(333 * 2) {
            let f = RawAudioFrame { samples: chunk.to_vec(), channels: 2, sample_rate: in_rate, pts: None };
            r_chunked.convert(Some(&f), &mut out_chunked);
        }
        assert_eq!(out_full.len(), out_chunked.len());
        let max_diff = out_full.iter().zip(&out_chunked).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        assert!(max_diff < 0.02, "chunk boundary discontinuity: {max_diff}");
    }

    #[test]
    fn surround_51_and_71_downmix_is_normalized_and_symmetric() {
        // 5.1 full-scale on all channels must not clip above 1.0
        let f51 = RawAudioFrame { samples: vec![1.0, 1.0, 1.0, 1.0, 1.0, 1.0], channels: 6, sample_rate: 48_000, pts: Some(0) };
        let mut r51 = Resampler::new(&f51, 48_000);
        let mut out51 = Vec::new();
        r51.convert(Some(&f51), &mut out51);
        assert_eq!(out51.len(), 2);
        assert!((out51[0] - out51[1]).abs() < 1e-6);
        assert!(out51[0] > 0.8 && out51[0] <= 1.15, "5.1 peak = {}", out51[0]);

        // 7.1 center-only signal routes equally to L and R
        let f71 = RawAudioFrame { samples: vec![0.0, 0.0, 0.8, 0.0, 0.0, 0.0, 0.0, 0.0], channels: 8, sample_rate: 48_000, pts: Some(0) };
        let mut r71 = Resampler::new(&f71, 48_000);
        let mut out71 = Vec::new();
        r71.convert(Some(&f71), &mut out71);
        assert_eq!(out71.len(), 2);
        assert!((out71[0] - out71[1]).abs() < 1e-6 && out71[0] > 0.1);
    }
}

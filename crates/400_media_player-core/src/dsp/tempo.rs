//! Pitch-preserving time-stretch (WSOLA — waveform-similarity overlap-add).
//!
//! The stretcher works on interleaved stereo `f32`. Output is produced in fixed blocks of one
//! *hop* (≈ 20 ms); each block comes with the media time span it represents, which the audio
//! clock uses to stay exact whatever the playback speed is.
//!
//! * `speed == 1.0` is bit-transparent: blocks are copied straight from the input.
//! * Otherwise each block is the cross-fade between the natural continuation of the previous
//!   window and the best-matching segment (normalised cross-correlation, searched ±10 ms around
//!   the nominal analysis position, coarse-to-fine on a 4× decimated mono signal).
//!
//! Because the algorithm only depends on the *content* of the input, its output is independent
//! from how the input is chunked, which the tests verify.

use std::f32::consts::PI;

/// Frames of decimation used by the coarse search.
const DECIM: usize = 4;

/// Streaming WSOLA time stretcher for interleaved stereo audio.
pub struct Tempo {
    rate: f64,
    hop: usize,
    tol: usize,
    speed: f64,
    /// Interleaved stereo input FIFO covering frames `[buf_start, buf_start + len)`.
    buf: Vec<f32>,
    buf_start: u64,
    /// Nominal analysis position of the next block, in input frames.
    nominal: f64,
    /// Natural continuation of the previous window (`hop` frames, interleaved); empty at start.
    tail: Vec<f32>,
    tail_start: u64,
    origin_pts: f64,
    real_end: u64,
    eof: bool,
    fade: Vec<f32>,
    // scratch buffers (kept to avoid per-block allocation)
    tail_mono: Vec<f32>,
    td: Vec<f32>,
    xd: Vec<f32>,
}

impl Tempo {
    pub fn new(sample_rate: u32) -> Self {
        let rate = f64::from(sample_rate.max(8000));
        // hop ≈ 20 ms rounded to a multiple of 8, search tolerance ≈ 10 ms.
        let hop = (((rate * 0.020) as usize + 4) / 8 * 8).max(64);
        let tol = ((rate * 0.010) as usize).max(16);
        let fade = (0..hop).map(|i| 0.5 - 0.5 * (PI * (i as f32 + 0.5) / hop as f32).cos()).collect();
        Self {
            rate,
            hop,
            tol,
            speed: 1.0,
            buf: Vec::new(),
            buf_start: 0,
            nominal: 0.0,
            tail: Vec::new(),
            tail_start: 0,
            origin_pts: 0.0,
            real_end: 0,
            eof: false,
            fade,
            tail_mono: Vec::new(),
            td: Vec::new(),
            xd: Vec::new(),
        }
    }

    /// Frames per output block.
    pub fn hop(&self) -> usize {
        self.hop
    }

    pub fn speed(&self) -> f64 {
        self.speed
    }

    /// Sets the playback speed (clamped to `0.25 ..= 4.0`); effective from the next block.
    pub fn set_speed(&mut self, speed: f64) {
        let s = if speed.is_finite() { speed.clamp(0.25, 4.0) } else { 1.0 };
        self.speed = if (s - 1.0).abs() < 1e-9 { 1.0 } else { s };
    }

    /// Drops all state; the next input frame is assumed to be at media time `origin_pts`.
    pub fn reset(&mut self, origin_pts: f64) {
        self.buf.clear();
        self.buf_start = 0;
        self.nominal = 0.0;
        self.tail.clear();
        self.tail_start = 0;
        self.origin_pts = origin_pts;
        self.real_end = 0;
        self.eof = false;
    }

    /// Appends interleaved stereo samples (`len` must be even).
    pub fn push(&mut self, interleaved: &[f32]) {
        debug_assert!(interleaved.len().is_multiple_of(2));
        self.buf.extend_from_slice(interleaved);
    }

    /// Declares the end of the input: remaining blocks are flushed with zero padding.
    pub fn finish(&mut self) {
        self.eof = true;
        self.real_end = self.avail_end();
    }

    /// Input frames buffered but not yet consumed.
    pub fn pending_frames(&self) -> usize {
        self.buf.len() / 2
    }

    #[inline]
    fn avail_end(&self) -> u64 {
        self.buf_start + (self.buf.len() / 2) as u64
    }

    #[inline]
    fn mono_at(&self, abs: u64) -> f32 {
        let i = ((abs - self.buf_start) * 2) as usize;
        0.5 * (self.buf[i] + self.buf[i + 1])
    }

    /// Produces the next block, appending `hop` stereo frames to `out`.
    ///
    /// Returns the media-time span `(start, end)` in seconds that the block covers, or `None`
    /// when more input is needed (or, after [`finish`](Self::finish), when everything has been
    /// delivered).
    pub fn pull(&mut self, out: &mut Vec<f32>) -> Option<(f64, f64)> {
        let h = self.hop as u64;
        let tol = self.tol as u64;
        let c = self.nominal.max(0.0).round() as u64;
        let has_tail = !self.tail.is_empty();
        let exact = has_tail && self.speed == 1.0 && c == self.tail_start;
        let need_end = if exact || !has_tail { c + 2 * h } else { c + tol + 2 * h };

        if self.eof {
            if c >= self.real_end {
                return None;
            }
            let missing = need_end.saturating_sub(self.avail_end());
            if missing > 0 {
                self.buf.resize(self.buf.len() + missing as usize * 2, 0.0);
            }
        } else if self.avail_end() < need_end {
            return None;
        }

        let s = if !has_tail || exact { c.max(self.buf_start) } else { self.search(c) };

        let off = ((s - self.buf_start) * 2) as usize;
        let hop2 = self.hop * 2;
        {
            let x = &self.buf[off..off + hop2];
            if !has_tail || exact {
                out.extend_from_slice(x);
            } else {
                out.reserve(hop2);
                for i in 0..self.hop {
                    let w = self.fade[i];
                    let iw = 1.0 - w;
                    out.push(self.tail[2 * i] * iw + x[2 * i] * w);
                    out.push(self.tail[2 * i + 1] * iw + x[2 * i + 1] * w);
                }
            }
        }
        self.tail.clear();
        self.tail.extend_from_slice(&self.buf[off + hop2..off + 2 * hop2]);
        self.tail_start = s + h;

        let pts_start = self.origin_pts + self.nominal / self.rate;
        self.nominal = if self.speed == 1.0 { (s + h) as f64 } else { self.nominal + self.hop as f64 * self.speed };
        let pts_end = self.origin_pts + self.nominal / self.rate;

        // Drop input that no later search can reach.
        let keep_from = (self.nominal.max(0.0).round() as u64).saturating_sub(tol).max(self.buf_start);
        let drop_frames = (keep_from - self.buf_start) as usize;
        if drop_frames > 0 {
            let n = (drop_frames * 2).min(self.buf.len());
            self.buf.drain(..n);
            self.buf_start += (n / 2) as u64;
        }
        Some((pts_start, pts_end))
    }

    /// Finds the start (absolute input frame) of the segment that best continues `self.tail`.
    fn search(&mut self, c: u64) -> u64 {
        let h = self.hop;
        let tol = self.tol as u64;
        let lo = c.saturating_sub(tol).max(self.buf_start);
        let hi = c + tol;
        let span = (hi - lo) as usize;

        // Mono + decimated versions of the target (the tail).
        self.tail_mono.clear();
        for i in 0..h {
            self.tail_mono.push(0.5 * (self.tail[2 * i] + self.tail[2 * i + 1]));
        }
        let nd = h / DECIM;
        self.td.clear();
        for k in 0..nd {
            let s: f32 = self.tail_mono[k * DECIM..(k + 1) * DECIM].iter().sum();
            self.td.push(s / DECIM as f32);
        }
        // Decimated mono of the candidate region [lo, hi + h).
        let nx = span / DECIM + nd + 1;
        self.xd.clear();
        for j in 0..nx {
            let base = lo + (j * DECIM) as u64;
            let mut s = 0.0f32;
            for m in 0..DECIM as u64 {
                s += self.mono_at(base + m);
            }
            self.xd.push(s / DECIM as f32);
        }

        // Coarse search.
        let ncand = span / DECIM + 1;
        let center_i = (((c - lo) as usize) / DECIM).min(ncand - 1);
        let mut energy: f64 = self.xd[..nd].iter().map(|v| f64::from(*v) * f64::from(*v)).sum();
        let mut best_i = center_i;
        let mut best_score = f64::NEG_INFINITY;
        for i in 0..ncand {
            let mut dot = 0.0f32;
            for (a, b) in self.xd[i..i + nd].iter().zip(&self.td) {
                dot += a * b;
            }
            let score = f64::from(dot) / (energy + 1e-12).sqrt() - 1e-9 * (i as f64 - center_i as f64).abs();
            if score > best_score {
                best_score = score;
                best_i = i;
            }
            if i + 1 < ncand {
                let out_v = f64::from(self.xd[i]);
                let in_v = f64::from(self.xd[i + nd]);
                energy = (energy - out_v * out_v + in_v * in_v).max(0.0);
            }
        }

        // Refinement at full resolution around the coarse winner.
        let s0 = lo + (best_i * DECIM) as u64;
        let from = s0.saturating_sub(DECIM as u64 - 1).max(lo);
        let to = (s0 + DECIM as u64 - 1).min(hi);
        let mut best_s = s0.clamp(lo, hi);
        let mut best_score = f64::NEG_INFINITY;
        for s in from..=to {
            let mut dot = 0.0f64;
            let mut e = 0.0f64;
            for k in 0..h {
                let x = f64::from(self.mono_at(s + k as u64));
                dot += x * f64::from(self.tail_mono[k]);
                e += x * x;
            }
            let score = dot / (e + 1e-12).sqrt() - 1e-9 * (s as f64 - c as f64).abs();
            if score > best_score {
                best_score = score;
                best_s = s;
            }
        }
        best_s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI as PI64;

    const FS: u32 = 48_000;

    fn sine(freq: f64, secs: f64, amp: f32) -> Vec<f32> {
        let n = (secs * f64::from(FS)) as usize;
        (0..n)
            .flat_map(|i| {
                let v = (2.0 * PI64 * freq * i as f64 / f64::from(FS)).sin() as f32 * amp;
                [v, v]
            })
            .collect()
    }

    /// Runs the whole input through a Tempo in chunks of `chunk` frames.
    fn run(input: &[f32], speed: f64, chunk: usize) -> (Vec<f32>, Vec<(f64, f64)>) {
        let mut t = Tempo::new(FS);
        t.set_speed(speed);
        let mut out = Vec::new();
        let mut spans = Vec::new();
        for c in input.chunks(chunk * 2) {
            t.push(c);
            while let Some(s) = t.pull(&mut out) {
                spans.push(s);
            }
        }
        t.finish();
        while let Some(s) = t.pull(&mut out) {
            spans.push(s);
        }
        (out, spans)
    }

    fn crossings_per_second(samples: &[f32]) -> f64 {
        let mono: Vec<f32> = samples.chunks_exact(2).map(|f| f[0]).collect();
        let mut n = 0;
        for w in mono.windows(2) {
            if w[0] <= 0.0 && w[1] > 0.0 {
                n += 1;
            }
        }
        n as f64 / (mono.len() as f64 / f64::from(FS))
    }

    fn rms(samples: &[f32]) -> f64 {
        (samples.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>() / samples.len() as f64).sqrt()
    }

    #[test]
    fn unity_speed_is_transparent() {
        let input = sine(440.0, 1.0, 0.5);
        let (out, spans) = run(&input, 1.0, 1000);
        assert!(out.len() >= input.len() - 2 * 2 * 960, "covers the input: {} vs {}", out.len(), input.len());
        let n = out.len().min(input.len());
        let max_err = out[..n].iter().zip(&input[..n]).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(max_err < 1e-6, "max error {max_err}");
        // spans are contiguous and run at real time
        for w in spans.windows(2) {
            assert!((w[0].1 - w[1].0).abs() < 1e-9);
        }
        assert!((spans[0].0 - 0.0).abs() < 1e-12);
    }

    #[test]
    fn speed_up_keeps_pitch_and_scales_duration() {
        let input = sine(440.0, 3.0, 0.5);
        let (out, spans) = run(&input, 1.5, 1024);
        let out_secs = out.len() as f64 / 2.0 / f64::from(FS);
        assert!((out_secs - 2.0).abs() < 0.12, "duration {out_secs}");
        let mid = &out[out.len() / 4..out.len() * 3 / 4];
        let f = crossings_per_second(mid);
        assert!((f - 440.0).abs() < 440.0 * 0.03, "pitch drifted: {f} Hz");
        let r = rms(mid);
        assert!((r - 0.5 / 2f64.sqrt()).abs() < 0.04, "amplitude {r}");
        // media time advances 1.5x faster than output time
        let media: f64 = spans.iter().map(|s| s.1 - s.0).sum();
        assert!((media / out_secs - 1.5).abs() < 0.02, "ratio {}", media / out_secs);
    }

    #[test]
    fn slow_down_keeps_pitch() {
        let input = sine(220.0, 2.0, 0.4);
        let (out, _) = run(&input, 0.5, 777);
        let out_secs = out.len() as f64 / 2.0 / f64::from(FS);
        assert!((out_secs - 4.0).abs() < 0.2, "duration {out_secs}");
        let mid = &out[out.len() / 4..out.len() * 3 / 4];
        let f = crossings_per_second(mid);
        assert!((f - 220.0).abs() < 220.0 * 0.03, "pitch {f}");
    }

    #[test]
    fn output_is_independent_from_input_chunking() {
        let mut input = sine(330.0, 1.0, 0.3);
        // add a second partial so the signal is not trivially periodic
        let extra = sine(1234.0, 1.0, 0.2);
        for (a, b) in input.iter_mut().zip(&extra) {
            *a += b;
        }
        let (a, _) = run(&input, 1.25, 1);
        let (b, _) = run(&input, 1.25, 4096);
        let (c, _) = run(&input, 1.25, 333);
        assert_eq!(a.len(), b.len());
        assert_eq!(a, b);
        assert_eq!(a, c);
    }

    #[test]
    fn speed_changes_are_click_free() {
        let input = sine(200.0, 4.0, 0.5);
        let mut t = Tempo::new(FS);
        let mut out = Vec::new();
        let schedule = [(1.0, 0.0), (1.5, 1.0), (0.75, 2.0), (1.0, 3.0)];
        let mut pos = 0usize;
        for (speed, at) in schedule {
            t.set_speed(speed);
            let end = ((at + 1.0) * f64::from(FS)) as usize * 2;
            while pos < end.min(input.len()) {
                let n = 960.min(end.min(input.len()) - pos);
                t.push(&input[pos..pos + n]);
                pos += n;
                while t.pull(&mut out).is_some() {}
            }
        }
        assert!(out.iter().all(|v| v.is_finite()));
        // 200 Hz at 0.5 amplitude: the steepest legitimate slope is 2π·200/48000·0.5 ≈ 0.013 per frame.
        let max_step = out.chunks_exact(2).map(|f| f[0]).collect::<Vec<_>>().windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
        assert!(max_step < 0.06, "discontinuity of {max_step}");
    }

    #[test]
    fn reset_restarts_the_timeline() {
        let input = sine(440.0, 0.5, 0.5);
        let mut t = Tempo::new(FS);
        let mut out = Vec::new();
        t.push(&input);
        while t.pull(&mut out).is_some() {}
        t.reset(100.0);
        out.clear();
        t.push(&input);
        let first = t.pull(&mut out).unwrap();
        assert!((first.0 - 100.0).abs() < 1e-9);
        assert_eq!(out.len(), t.hop() * 2);
    }

    #[test]
    fn silence_and_noise_do_not_break_the_search() {
        let silence = vec![0.0f32; FS as usize * 2];
        let (out, _) = run(&silence, 1.7, 2048);
        assert!(out.iter().all(|v| *v == 0.0));
        let mut x = 12345u32;
        let noise: Vec<f32> = (0..FS as usize * 2)
            .map(|_| {
                x = x.wrapping_mul(1664525).wrapping_add(1013904223);
                (x >> 8) as f32 / (1 << 24) as f32 - 0.5
            })
            .collect();
        let (out, _) = run(&noise, 0.8, 2048);
        assert!(out.iter().all(|v| v.is_finite() && v.abs() <= 1.0));
    }
}

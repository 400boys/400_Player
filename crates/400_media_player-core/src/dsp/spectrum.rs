//! Real-time spectrum analyser for the visualiser (log-spaced bands, smoothing, peak hold).

use std::f32::consts::PI;
use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{RealFftPlanner, RealToComplex};

const FFT_SIZE: usize = 2048;

/// Turns a stream of mono samples into smoothed, display-ready band levels in `0.0 ..= 1.0`.
pub struct SpectrumAnalyzer {
    sample_rate: f32,
    fft: Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,
    ring: Vec<f32>,
    write: usize,
    filled: usize,
    fresh: usize,
    input: Vec<f32>,
    output: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    /// `(lo_hz, hi_hz)` of each band.
    edges: Vec<(f32, f32)>,
    bands: Vec<f32>,
    peaks: Vec<f32>,
    peak_age: Vec<f32>,
    level: f32,
}

impl SpectrumAnalyzer {
    pub fn new(sample_rate: u32, bands: usize) -> Self {
        let sample_rate = sample_rate.max(8000) as f32;
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(FFT_SIZE);
        let window = (0..FFT_SIZE).map(|i| 0.5 - 0.5 * (2.0 * PI * i as f32 / FFT_SIZE as f32).cos()).collect();
        let bands = bands.max(4);
        let f_min = 40.0f32;
        let f_max = 16_000.0f32.min(sample_rate * 0.45);
        let ratio = (f_max / f_min).powf(1.0 / bands as f32);
        let edges = (0..bands).map(|i| (f_min * ratio.powi(i as i32), f_min * ratio.powi(i as i32 + 1))).collect();
        Self {
            sample_rate,
            input: fft.make_input_vec(),
            output: fft.make_output_vec(),
            scratch: fft.make_scratch_vec(),
            fft,
            window,
            ring: vec![0.0; FFT_SIZE],
            write: 0,
            filled: 0,
            fresh: 0,
            edges,
            bands: vec![0.0; bands],
            peaks: vec![0.0; bands],
            peak_age: vec![0.0; bands],
            level: 0.0,
        }
    }

    /// Feeds mono samples (the most recent `2048` are kept).
    pub fn push(&mut self, mono: &[f32]) {
        for &s in mono {
            self.ring[self.write] = s;
            self.write = (self.write + 1) % FFT_SIZE;
        }
        self.filled = (self.filled + mono.len()).min(FFT_SIZE);
        self.fresh += mono.len();
    }

    pub fn reset(&mut self) {
        self.ring.fill(0.0);
        self.filled = 0;
        self.fresh = 0;
        self.bands.fill(0.0);
        self.peaks.fill(0.0);
        self.level = 0.0;
    }

    /// Smoothed band levels, `0.0 ..= 1.0`.
    pub fn bands(&self) -> &[f32] {
        &self.bands
    }
    /// Peak-hold markers, `0.0 ..= 1.0`.
    pub fn peaks(&self) -> &[f32] {
        &self.peaks
    }
    /// Overall low-frequency energy (useful to drive background effects).
    pub fn level(&self) -> f32 {
        self.level
    }

    /// Recomputes the spectrum; `dt` is the time since the previous call in seconds.
    pub fn update(&mut self, dt: f32) {
        let dt = dt.clamp(0.0, 0.25);
        let mut target = vec![0.0f32; self.bands.len()];
        if self.fresh > 0 && self.filled >= FFT_SIZE / 4 {
            for i in 0..FFT_SIZE {
                self.input[i] = self.ring[(self.write + i) % FFT_SIZE] * self.window[i];
            }
            if self.fft.process_with_scratch(&mut self.input, &mut self.output, &mut self.scratch).is_ok() {
                let bin_hz = self.sample_rate / FFT_SIZE as f32;
                let mag = |k: usize| 4.0 * self.output[k.min(self.output.len() - 1)].norm() / FFT_SIZE as f32;
                for (i, &(lo, hi)) in self.edges.iter().enumerate() {
                    let (lo_b, hi_b) = (lo / bin_hz, hi / bin_hz);
                    let m = if hi_b - lo_b < 1.0 {
                        let pos = 0.5 * (lo_b + hi_b);
                        let k = pos.floor() as usize;
                        let f = pos - k as f32;
                        mag(k) * (1.0 - f) + mag(k + 1) * f
                    } else {
                        (lo_b.ceil() as usize..=hi_b.floor() as usize).map(mag).fold(0.0, f32::max)
                    };
                    let center = (lo * hi).sqrt();
                    let db = 20.0 * (m + 1e-7).log10() + 3.0 * (center / 1000.0).log2();
                    target[i] = ((db + 72.0) / 72.0).clamp(0.0, 1.0);
                }
            }
        }
        self.fresh = 0;

        let attack = 1.0 - (-dt / 0.03).exp();
        let release = 1.0 - (-dt / 0.20).exp();
        let bars = self.bands.iter_mut().zip(self.peaks.iter_mut()).zip(self.peak_age.iter_mut());
        for (((band, peak), age), &goal) in bars.zip(target.iter()) {
            let k = if goal > *band { attack } else { release };
            *band += (goal - *band) * k;
            if *band >= *peak {
                *peak = *band;
                *age = 0.0;
            } else {
                *age += dt;
                if *age > 0.45 {
                    *peak = (*peak - dt * 0.6).max(*band);
                }
            }
        }
        let n = (self.bands.len() / 4).max(1);
        let low = self.bands[..n].iter().sum::<f32>() / n as f32;
        self.level += (low - self.level) * (1.0 - (-dt / 0.08).exp());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed_sine(a: &mut SpectrumAnalyzer, fs: f32, f: f32, amp: f32, frames: usize) {
        let s: Vec<f32> = (0..frames).map(|i| (2.0 * PI * f * i as f32 / fs).sin() * amp).collect();
        a.push(&s);
    }

    #[test]
    fn sine_lights_up_the_right_band() {
        let fs = 48_000.0;
        let mut a = SpectrumAnalyzer::new(48_000, 32);
        for _ in 0..30 {
            feed_sine(&mut a, fs, 1000.0, 0.8, 1024);
            a.update(1.0 / 60.0);
        }
        let (best, &v) = a.bands().iter().enumerate().max_by(|x, y| x.1.total_cmp(y.1)).unwrap();
        let (lo, hi) = a.edges[best];
        assert!(lo <= 1100.0 && hi >= 900.0, "band {best} covers {lo}-{hi}");
        assert!(v > 0.6, "level {v}");
        assert!(a.bands().iter().all(|b| (0.0..=1.0).contains(b)));
        assert!(a.peaks().iter().zip(a.bands()).all(|(p, b)| p >= b));
    }

    #[test]
    fn silence_decays_to_zero() {
        let mut a = SpectrumAnalyzer::new(44_100, 24);
        for _ in 0..20 {
            feed_sine(&mut a, 44_100.0, 440.0, 0.9, 1024);
            a.update(1.0 / 60.0);
        }
        assert!(a.bands().iter().any(|b| *b > 0.3));
        for _ in 0..120 {
            a.update(1.0 / 60.0); // no fresh samples (paused)
        }
        assert!(a.bands().iter().all(|b| *b < 0.01), "{:?}", a.bands());
        assert!(a.level() < 0.01);
    }
}

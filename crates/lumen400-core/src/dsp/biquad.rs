//! RBJ-cookbook biquad filters (transposed direct form II, `f64` state for low-frequency
//! stability).

use std::f64::consts::PI;

/// Normalised biquad coefficients (`a0 == 1`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BiquadCoeffs {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
}

impl Default for BiquadCoeffs {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl BiquadCoeffs {
    pub const IDENTITY: Self = Self { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0 };

    /// Peaking (bell) filter.
    pub fn peaking(sample_rate: f64, freq: f64, q: f64, gain_db: f64) -> Self {
        let a = 10f64.powf(gain_db / 40.0);
        let w0 = 2.0 * PI * freq / sample_rate;
        let alpha = w0.sin() / (2.0 * q);
        let cosw = w0.cos();
        let a0 = 1.0 + alpha / a;
        Self { b0: (1.0 + alpha * a) / a0, b1: (-2.0 * cosw) / a0, b2: (1.0 - alpha * a) / a0, a1: (-2.0 * cosw) / a0, a2: (1.0 - alpha / a) / a0 }
    }

    /// Magnitude response in dB at `freq`.
    pub fn response_db(&self, sample_rate: f64, freq: f64) -> f64 {
        let w = 2.0 * PI * freq / sample_rate;
        let (c1, s1) = (w.cos(), -w.sin());
        let (c2, s2) = ((2.0 * w).cos(), -(2.0 * w).sin());
        let nr = self.b0 + self.b1 * c1 + self.b2 * c2;
        let ni = self.b1 * s1 + self.b2 * s2;
        let dr = 1.0 + self.a1 * c1 + self.a2 * c2;
        let di = self.a1 * s1 + self.a2 * s2;
        let mag2 = (nr * nr + ni * ni) / (dr * dr + di * di).max(1e-300);
        10.0 * mag2.max(1e-300).log10()
    }
}

/// One filter section with its own state.
#[derive(Clone, Copy, Debug, Default)]
pub struct Biquad {
    c: BiquadCoeffs,
    z1: f64,
    z2: f64,
}

impl Biquad {
    pub fn set_coeffs(&mut self, c: BiquadCoeffs) {
        self.c = c;
    }

    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }

    #[inline]
    pub fn process(&mut self, x: f64) -> f64 {
        let c = &self.c;
        let y = c.b0 * x + self.z1;
        self.z1 = c.b1 * x - c.a1 * y + self.z2;
        self.z2 = c.b2 * x - c.a2 * y;
        y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peaking_hits_requested_gain_at_center() {
        for &(f, g) in &[(100.0, 6.0), (1000.0, -9.0), (8000.0, 12.0), (31.25, 3.0)] {
            let c = BiquadCoeffs::peaking(48_000.0, f, 1.41, g);
            let r = c.response_db(48_000.0, f);
            assert!((r - g).abs() < 0.01, "f={f} g={g} got {r}");
            // far away the filter is transparent
            let far = c.response_db(48_000.0, if f < 1000.0 { 12_000.0 } else { 40.0 });
            assert!(far.abs() < 1.5, "far response {far}");
        }
    }

    #[test]
    fn identity_is_transparent() {
        let mut b = Biquad::default();
        for i in 0..100 {
            let x = (i as f64 * 0.37).sin();
            assert!((b.process(x) - x).abs() < 1e-12);
        }
    }

    #[test]
    fn time_domain_matches_frequency_response() {
        let fs = 48_000.0;
        let f = 1000.0;
        let c = BiquadCoeffs::peaking(fs, f, 1.41, 9.0);
        let mut b = Biquad::default();
        b.set_coeffs(c);
        let mut peak = 0.0f64;
        for n in 0..48_000 {
            let x = (2.0 * PI * f * n as f64 / fs).sin();
            let y = b.process(x);
            if n > 24_000 {
                peak = peak.max(y.abs());
            }
        }
        let db = 20.0 * peak.log10();
        assert!((db - 9.0).abs() < 0.05, "measured {db}");
    }
}

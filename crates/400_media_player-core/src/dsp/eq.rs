//! 10-band graphic equaliser (one octave apart, peaking filters) with presets.

use serde::{Deserialize, Serialize};

use super::biquad::{Biquad, BiquadCoeffs};

pub const EQ_BANDS: usize = 10;
/// Centre frequencies in Hz (ISO octave bands).
pub const EQ_FREQS: [f32; EQ_BANDS] = [31.25, 62.5, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];
/// Band gain limit in dB.
pub const EQ_MAX_DB: f32 = 12.0;
/// Bandwidth of every band (≈ one octave).
pub const EQ_Q: f64 = 1.41;

/// User-facing equaliser parameters (persisted in the settings).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EqSettings {
    pub enabled: bool,
    pub preamp_db: f32,
    pub gains_db: [f32; EQ_BANDS],
    /// Name of the preset the gains came from (`"Custom"` once edited).
    pub preset: String,
}

impl Default for EqSettings {
    fn default() -> Self {
        Self { enabled: false, preamp_db: 0.0, gains_db: [0.0; EQ_BANDS], preset: "Flat".into() }
    }
}

impl EqSettings {
    /// Settings for a named preset (case-insensitive); `None` if unknown.
    pub fn from_preset(name: &str) -> Option<Self> {
        let p = PRESETS.iter().find(|p| p.name.eq_ignore_ascii_case(name))?;
        let mut s = Self { enabled: true, preamp_db: 0.0, gains_db: p.gains, preset: p.name.to_owned() };
        s.gains_db.iter_mut().for_each(|g| *g = g.clamp(-EQ_MAX_DB, EQ_MAX_DB));
        s.preamp_db = s.auto_preamp();
        Some(s)
    }

    /// Preamp that exactly compensates the largest boost, preventing clipping.
    pub fn auto_preamp(&self) -> f32 {
        -self.gains_db.iter().copied().fold(0.0f32, f32::max)
    }

    pub fn is_flat(&self) -> bool {
        self.preamp_db.abs() < 0.05 && self.gains_db.iter().all(|g| g.abs() < 0.05)
    }

    /// Linear preamp gain.
    pub fn preamp_gain(&self) -> f32 {
        10f32.powf(self.preamp_db.clamp(-24.0, 24.0) / 20.0)
    }
}

/// A factory preset.
pub struct EqPreset {
    pub name: &'static str,
    pub gains: [f32; EQ_BANDS],
}

macro_rules! preset {
    ($name:expr, [$($g:expr),*]) => { EqPreset { name: $name, gains: [$($g),*] } };
}

/// Factory presets (the classic graphic-equaliser set plus a few modern ones).
pub const PRESETS: &[EqPreset] = &[
    preset!("Flat", [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
    preset!("Bass Boost", [6.0, 5.0, 3.5, 1.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
    preset!("Treble Boost", [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.5, 3.5, 5.0, 6.0]),
    preset!("Vocal", [-2.0, -2.0, -1.0, 1.0, 3.0, 4.0, 3.0, 1.0, 0.0, -1.0]),
    preset!("Loudness", [5.0, 3.5, 0.0, 0.0, -1.0, 0.0, -0.5, 0.0, 3.0, 4.0]),
    preset!("Classical", [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -7.2, -7.2, -7.2, -9.6]),
    preset!("Club", [0.0, 0.0, 8.0, 5.6, 5.6, 5.6, 3.2, 0.0, 0.0, 0.0]),
    preset!("Dance", [9.6, 7.2, 2.4, 0.0, 0.0, -5.6, -7.2, -7.2, 0.0, 0.0]),
    preset!("Full Bass", [-8.0, 9.6, 9.6, 5.6, 1.6, -4.0, -8.0, -10.4, -11.2, -11.2]),
    preset!("Full Treble", [-9.6, -9.6, -9.6, -4.0, 2.4, 11.2, 12.0, 12.0, 12.0, 12.0]),
    preset!("Headphones", [4.8, 11.2, 5.6, -3.2, -2.4, 1.6, 4.8, 9.6, 12.0, 12.0]),
    preset!("Live", [-4.8, 0.0, 4.0, 5.6, 5.6, 5.6, 4.0, 2.4, 2.4, 2.4]),
    preset!("Party", [7.2, 7.2, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 7.2, 7.2]),
    preset!("Pop", [-1.6, 4.8, 7.2, 8.0, 5.6, 0.0, -2.4, -2.4, -1.6, -1.6]),
    preset!("Reggae", [0.0, 0.0, 0.0, -5.6, 0.0, 6.4, 6.4, 0.0, 0.0, 0.0]),
    preset!("Rock", [8.0, 4.8, -5.6, -8.0, -3.2, 4.0, 8.8, 11.2, 11.2, 11.2]),
    preset!("Ska", [-2.4, -4.8, -4.0, 0.0, 4.0, 5.6, 8.8, 9.6, 11.2, 9.6]),
    preset!("Soft", [4.8, 1.6, 0.0, -2.4, 0.0, 4.0, 8.0, 9.6, 11.2, 12.0]),
    preset!("Techno", [8.0, 5.6, 0.0, -5.6, -4.8, 0.0, 8.0, 9.6, 9.6, 8.8]),
];

/// Combined response of the band filters (preamp excluded) in dB, for drawing the curve.
pub fn eq_response_db(settings: &EqSettings, sample_rate: f64, freq: f64) -> f64 {
    let mut total = 0.0;
    for (i, &g) in settings.gains_db.iter().enumerate() {
        if g.abs() < 0.05 || f64::from(EQ_FREQS[i]) > sample_rate * 0.45 {
            continue;
        }
        total += BiquadCoeffs::peaking(sample_rate, f64::from(EQ_FREQS[i]), EQ_Q, f64::from(g)).response_db(sample_rate, freq);
    }
    total
}

#[derive(Clone, Copy, Default)]
struct Band {
    active: bool,
    l: Biquad,
    r: Biquad,
}

/// Real-time stereo equaliser. Cheap to retune: the filter state is kept so parameter changes
/// do not click.
pub struct Equalizer {
    sample_rate: f64,
    bands: [Band; EQ_BANDS],
    preamp: f32,
    enabled: bool,
}

impl Equalizer {
    pub fn new(sample_rate: u32) -> Self {
        Self { sample_rate: f64::from(sample_rate), bands: [Band::default(); EQ_BANDS], preamp: 1.0, enabled: false }
    }

    /// Applies new parameters.
    pub fn apply(&mut self, s: &EqSettings) {
        self.enabled = s.enabled;
        self.preamp = s.preamp_gain();
        for (i, band) in self.bands.iter_mut().enumerate() {
            let g = f64::from(s.gains_db[i].clamp(-EQ_MAX_DB, EQ_MAX_DB));
            let f = f64::from(EQ_FREQS[i]);
            band.active = g.abs() >= 0.05 && f <= self.sample_rate * 0.45;
            let c = if band.active { BiquadCoeffs::peaking(self.sample_rate, f, EQ_Q, g) } else { BiquadCoeffs::IDENTITY };
            band.l.set_coeffs(c);
            band.r.set_coeffs(c);
        }
    }

    /// `true` when processing would be a no-op.
    pub fn is_bypassed(&self) -> bool {
        !self.enabled || (self.preamp == 1.0 && self.bands.iter().all(|b| !b.active))
    }

    pub fn reset(&mut self) {
        for b in &mut self.bands {
            b.l.reset();
            b.r.reset();
        }
    }

    /// Processes interleaved stereo samples in place.
    pub fn process_stereo(&mut self, buf: &mut [f32]) {
        if self.is_bypassed() {
            return;
        }
        let pre = f64::from(self.preamp);
        for frame in buf.chunks_exact_mut(2) {
            let mut l = f64::from(frame[0]) * pre;
            let mut r = f64::from(frame[1]) * pre;
            for b in self.bands.iter_mut().filter(|b| b.active) {
                l = b.l.process(l);
                r = b.r.process(r);
            }
            frame[0] = l as f32;
            frame[1] = r as f32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    fn tone_rms_gain(eq: &mut Equalizer, f: f64, fs: f64) -> f64 {
        let n = 48_000;
        let mut buf: Vec<f32> = (0..n)
            .flat_map(|i| {
                let v = (2.0 * PI * f * i as f64 / fs).sin() as f32 * 0.25;
                [v, v]
            })
            .collect();
        let rms = |b: &[f32]| (b.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>() / b.len() as f64).sqrt();
        let before = rms(&buf[24_000..]);
        eq.process_stereo(&mut buf);
        20.0 * (rms(&buf[24_000..]) / before).log10()
    }

    #[test]
    fn flat_is_bypassed_and_exact() {
        let mut eq = Equalizer::new(48_000);
        let s = EqSettings { enabled: true, ..EqSettings::default() };
        eq.apply(&s);
        assert!(eq.is_bypassed());
        let mut buf = vec![0.1, -0.2, 0.3, 0.4];
        eq.process_stereo(&mut buf);
        assert_eq!(buf, vec![0.1, -0.2, 0.3, 0.4]);
    }

    #[test]
    fn band_gain_is_applied_to_the_tone_in_that_band() {
        let mut eq = Equalizer::new(48_000);
        let mut s = EqSettings { enabled: true, ..Default::default() };
        s.gains_db[5] = 8.0; // 1 kHz
        eq.apply(&s);
        let g = tone_rms_gain(&mut eq, 1000.0, 48_000.0);
        assert!((g - 8.0).abs() < 0.15, "gain at 1 kHz: {g}");
        eq.reset();
        let far = tone_rms_gain(&mut eq, 60.0, 48_000.0);
        assert!(far.abs() < 1.0, "gain far from the band: {far}");
    }

    #[test]
    fn disabled_does_nothing_and_preamp_works() {
        let mut eq = Equalizer::new(44_100);
        let mut s = EqSettings { enabled: false, preamp_db: -6.0, ..Default::default() };
        s.gains_db[0] = 12.0;
        eq.apply(&s);
        let mut buf = vec![0.5, 0.5];
        eq.process_stereo(&mut buf);
        assert_eq!(buf, vec![0.5, 0.5]);
        s.enabled = true;
        s.gains_db = [0.0; EQ_BANDS];
        eq.apply(&s);
        let mut buf = vec![0.5, 0.5];
        eq.process_stereo(&mut buf);
        assert!((buf[0] - 0.25).abs() < 0.01, "-6 dB ≈ x0.5: {}", buf[0]);
    }

    #[test]
    fn presets_are_valid() {
        for p in PRESETS {
            let s = EqSettings::from_preset(p.name).expect(p.name);
            assert!(s.gains_db.iter().all(|g| g.abs() <= EQ_MAX_DB), "{}", p.name);
            // auto preamp prevents boost clipping
            assert!(s.preamp_db <= 0.0);
        }
        assert!(EqSettings::from_preset("flat").unwrap().is_flat());
        assert!(EqSettings::from_preset("nope").is_none());
    }

    #[test]
    fn curve_matches_gain_at_center() {
        let mut s = EqSettings::default();
        s.gains_db[3] = 6.0;
        let r = eq_response_db(&s, 48_000.0, 250.0);
        assert!((r - 6.0).abs() < 0.05);
    }

    #[test]
    fn settings_roundtrip_through_json() {
        let s = EqSettings::from_preset("Rock").unwrap();
        let j = serde_json::to_string(&s).unwrap();
        let back: EqSettings = serde_json::from_str(&j).unwrap();
        assert_eq!(s, back);
        // Unknown / missing fields fall back to defaults.
        let partial: EqSettings = serde_json::from_str(r#"{"enabled":true}"#).unwrap();
        assert!(partial.enabled && partial.is_flat());
    }
}

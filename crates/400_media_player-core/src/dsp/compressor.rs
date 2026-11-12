//! A look-ahead dynamic range compressor: the engine of the night mode.
//!
//! Watching a film late in the evening, the explosions are too loud and the whispers too quiet. The
//! compressor turns everything above a threshold down (the louder, the more) and gives the result a
//! fixed boost, so loud passages come down a lot, quiet ones come up a little and speech stays about
//! where it was. The detector looks a millisecond or two ahead of what is played, so even a sudden
//! bang is already tamed when it reaches the output.
//!
//! The processing is stereo-linked (both channels get the same gain, the picture does not shift),
//! allocation-free once built and switched on and off with a short cross-fade, never with a click.

/// How the compressor behaves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CompressorSettings {
    /// Level (dBFS) above which the signal is turned down.
    pub threshold_db: f32,
    /// `4.0` means 4 dB more input above the threshold give 1 dB more output.
    pub ratio: f32,
    /// Width (dB) of the transition around the threshold: wider is gentler.
    pub knee_db: f32,
    /// How fast the gain comes down when a loud passage starts.
    pub attack_ms: f32,
    /// How slowly it comes back up afterwards.
    pub release_ms: f32,
    /// Fixed boost (dB) applied after the compression.
    pub makeup_db: f32,
    /// How far ahead of the output the detector listens.
    pub lookahead_ms: f32,
}

impl CompressorSettings {
    /// The night mode: peaks at 0 dBFS leave at about −12 dBFS, passages below −29 dBFS are raised
    /// by 6 dB, normal speech (around −20 dBFS) ends up about 3 dB louder than it was.
    pub const NIGHT: Self = Self { threshold_db: -24.0, ratio: 4.0, knee_db: 10.0, attack_ms: 1.0, release_ms: 220.0, makeup_db: 6.0, lookahead_ms: 1.5 };

    /// Gain change (dB, never positive) for an input of `level_db`, with the soft knee.
    pub fn gain_reduction_db(&self, level_db: f32) -> f32 {
        let slope = 1.0 - 1.0 / self.ratio.max(1.0);
        let over = level_db - self.threshold_db;
        let w = self.knee_db.max(0.0);
        if 2.0 * over <= -w {
            0.0
        } else if 2.0 * over.abs() < w {
            // Inside the knee the reduction grows smoothly from zero.
            let x = over + w * 0.5;
            -slope * x * x / (2.0 * w)
        } else {
            -slope * over
        }
    }

    /// Output level (dBFS) for a steady input of `level_db` (what the settings are meant to do).
    pub fn output_db(&self, level_db: f32) -> f32 {
        level_db + self.gain_reduction_db(level_db) + self.makeup_db
    }
}

/// How long switching the effect on or off takes to fade.
const FADE_SECONDS: f32 = 0.08;

/// The processor. One per audio output; feed it the interleaved stereo stream.
pub struct Compressor {
    settings: CompressorSettings,
    attack: f32,
    release: f32,
    makeup: f32,
    /// Look-ahead delay line (interleaved stereo) and its write position.
    delay: Vec<f32>,
    pos: usize,
    /// Smoothed gain reduction, in dB (zero or below).
    reduction_db: f32,
    enabled: bool,
    /// How much of the processed signal is heard: `0.0` = untouched, `1.0` = fully processed.
    wet: f32,
    wet_step: f32,
}

impl Compressor {
    pub fn new(sample_rate: u32, settings: CompressorSettings) -> Self {
        let rate = sample_rate.max(8_000) as f32;
        let lookahead = ((rate * settings.lookahead_ms / 1000.0).round() as usize).max(1);
        let coefficient = |ms: f32| (-1.0 / (rate * ms.max(0.01) / 1000.0)).exp();
        Self {
            settings,
            attack: coefficient(settings.attack_ms),
            release: coefficient(settings.release_ms),
            makeup: db_to_gain(settings.makeup_db),
            delay: vec![0.0; lookahead * 2],
            pos: 0,
            reduction_db: 0.0,
            enabled: false,
            wet: 0.0,
            wet_step: 1.0 / (rate * FADE_SECONDS),
        }
    }

    /// The night-mode compressor.
    pub fn night(sample_rate: u32) -> Self {
        Self::new(sample_rate, CompressorSettings::NIGHT)
    }

    pub fn settings(&self) -> &CompressorSettings {
        &self.settings
    }

    /// Switches the effect; the change is faded over a few milliseconds.
    pub fn set_enabled(&mut self, on: bool) {
        if on && !self.enabled && self.wet == 0.0 {
            // Start from a clean state: nothing stale from the last time it ran.
            self.delay.fill(0.0);
            self.reduction_db = 0.0;
        }
        self.enabled = on;
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Delay (in frames) the effect adds to the sound while it is on.
    pub fn latency_frames(&self) -> usize {
        self.delay.len() / 2
    }

    /// Processes interleaved stereo samples in place.
    pub fn process_stereo(&mut self, buf: &mut [f32]) {
        let target_wet = if self.enabled { 1.0 } else { 0.0 };
        if !self.enabled && self.wet == 0.0 {
            return;
        }
        let lookahead = self.delay.len() / 2;
        for frame in buf.chunks_exact_mut(2) {
            // A broken decoder can hand over NaN or infinity: neither may reach the gain smoother,
            // which would carry it forever.
            let dry_l = if frame[0].is_finite() { frame[0] } else { 0.0 };
            let dry_r = if frame[1].is_finite() { frame[1] } else { 0.0 };
            // Detector: the louder channel, on the sample about to enter the delay.
            let peak = dry_l.abs().max(dry_r.abs());
            let level_db = if peak > 1e-6 { 20.0 * peak.log10() } else { -120.0 };
            let target = self.settings.gain_reduction_db(level_db);
            let coefficient = if target < self.reduction_db { self.attack } else { self.release };
            self.reduction_db = target + coefficient * (self.reduction_db - target);
            let gain = db_to_gain(self.reduction_db) * self.makeup;

            // The sample played now is the one that entered `lookahead` frames ago.
            let i = self.pos * 2;
            let (late_l, late_r) = (self.delay[i], self.delay[i + 1]);
            self.delay[i] = dry_l;
            self.delay[i + 1] = dry_r;
            self.pos += 1;
            if self.pos == lookahead {
                self.pos = 0;
            }

            if self.wet < target_wet {
                self.wet = (self.wet + self.wet_step).min(target_wet);
            } else if self.wet > target_wet {
                self.wet = (self.wet - self.wet_step).max(target_wet);
            }
            let wet = self.wet;
            frame[0] = dry_l + (late_l * gain - dry_l) * wet;
            frame[1] = dry_r + (late_r * gain - dry_r) * wet;
        }
    }
}

#[inline]
fn db_to_gain(db: f32) -> f32 {
    (db * (std::f32::consts::LN_10 / 20.0)).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;

    fn sine(db: f32, seconds: f32, freq: f32, start: usize) -> Vec<f32> {
        let amp = db_to_gain(db);
        (0..(seconds * SR as f32) as usize)
            .flat_map(|i| {
                let v = amp * (2.0 * std::f32::consts::PI * freq * (start + i) as f32 / SR as f32).sin();
                [v, v]
            })
            .collect()
    }

    fn peak_db(buf: &[f32]) -> f32 {
        20.0 * buf.iter().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-9).log10()
    }

    fn rms_db(buf: &[f32]) -> f32 {
        let mean = buf.iter().map(|v| v * v).sum::<f32>() / buf.len() as f32;
        10.0 * mean.max(1e-18).log10()
    }

    /// The last half of a steady segment, once the gain has settled.
    fn settled(buf: &[f32]) -> &[f32] {
        &buf[buf.len() / 2..]
    }

    fn night() -> Compressor {
        let mut c = Compressor::night(SR);
        c.set_enabled(true);
        c
    }

    /// Lets the fade-in finish so the next measurement is of the fully processed sound.
    fn warm_up(c: &mut Compressor) {
        let mut quiet = sine(-60.0, 0.1, 100.0, 0);
        c.process_stereo(&mut quiet);
    }

    #[test]
    fn the_curve_matches_the_design() {
        let s = CompressorSettings::NIGHT;
        // Far below the threshold nothing is turned down: only the boost remains.
        assert_eq!(s.gain_reduction_db(-60.0), 0.0);
        assert!((s.output_db(-40.0) - (-34.0)).abs() < 1e-4);
        // Far above it the slope is the ratio: 0 dBFS ends up 18 dB lower, then +6 dB.
        assert!((s.gain_reduction_db(0.0) + 18.0).abs() < 1e-4);
        assert!((s.output_db(0.0) - (-12.0)).abs() < 1e-4);
        // Continuous and monotonic through the knee: louder in is never quieter out.
        let mut previous = f32::MIN;
        for i in -1200..=0 {
            let level = i as f32 * 0.1;
            let out = s.output_db(level);
            assert!(out >= previous - 1e-4, "{level}: {out} < {previous}");
            assert!(s.gain_reduction_db(level) <= 0.0);
            previous = out;
        }
        // The dynamic range is squeezed: 40 dB in, much less out.
        assert!(s.output_db(0.0) - s.output_db(-40.0) < 25.0);
    }

    #[test]
    fn loud_comes_down_quiet_comes_up_speech_stays() {
        let mut c = night();
        warm_up(&mut c);
        let mut quiet = sine(-40.0, 1.0, 440.0, 0);
        let mut speech = sine(-20.0, 1.0, 440.0, 0);
        let mut loud = sine(0.0, 1.0, 440.0, 0);
        c.process_stereo(&mut quiet);
        c.process_stereo(&mut speech);
        c.process_stereo(&mut loud);
        let (q, s, l) = (peak_db(settled(&quiet)), peak_db(settled(&speech)), peak_db(settled(&loud)));
        assert!((q - -34.0).abs() < 1.0, "quiet: {q}");
        assert!((s - -17.0).abs() < 1.5, "speech: {s}");
        assert!((l - -12.0).abs() < 1.0, "loud: {l}");
        // Between the quietest and the loudest passages: 40 dB before, 22 dB after.
        assert!((l - q) < 25.0, "{l} - {q}");
        // The peak never goes near full scale, so the soft clipper downstream has nothing to do.
        assert!(l < -6.0);
    }

    #[test]
    fn a_sudden_bang_is_tamed_before_it_is_heard() {
        let mut c = night();
        warm_up(&mut c);
        let mut silence = vec![0.0f32; SR as usize / 5 * 2];
        c.process_stereo(&mut silence);
        let mut bang = sine(0.0, 0.1, 300.0, 0);
        c.process_stereo(&mut bang);
        // The first five milliseconds after the onset already stay well under full scale (without the
        // look-ahead they would sit at +6 dBFS before the gain caught up).
        let first = &bang[..(SR as usize / 200) * 2];
        let peak = first.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak < 0.5, "peak {peak}");
        assert!(bang.iter().all(|v| v.abs() < 0.5), "nothing in the bang reaches −6 dBFS");
    }

    #[test]
    fn the_gain_comes_back_after_a_loud_passage() {
        let mut c = night();
        warm_up(&mut c);
        let mut loud = sine(0.0, 0.5, 440.0, 0);
        c.process_stereo(&mut loud);
        let mut tail = sine(-40.0, 2.0, 440.0, 0);
        c.process_stereo(&mut tail);
        // Right after the bang the whisper is still held down; two seconds later it is at +6 dB again.
        // (The first few milliseconds are the end of the bang itself, still in the look-ahead delay.)
        let early = peak_db(&tail[SR as usize / 100 * 2..SR as usize / 10 * 2]);
        let late = peak_db(&tail[tail.len() - SR as usize / 10 * 2..]);
        assert!(early < late - 3.0, "release: {early} → {late}");
        assert!((late - -34.0).abs() < 1.0, "recovered: {late}");
    }

    #[test]
    fn silence_stays_silent_and_nothing_overflows() {
        let mut c = night();
        let mut zeros = vec![0.0f32; 4800];
        c.process_stereo(&mut zeros);
        assert!(zeros.iter().all(|v| *v == 0.0));
        let mut wild: Vec<f32> = (0..9600).map(|i| if i % 7 < 3 { 1.0 } else { -1.0 }).collect();
        c.process_stereo(&mut wild);
        assert!(wild.iter().all(|v| v.is_finite() && v.abs() < 1.0), "full-scale square wave stays inside");
        let mut nonsense = vec![f32::NAN, 0.5, 0.2, f32::INFINITY, f32::NEG_INFINITY, f32::NAN];
        c.process_stereo(&mut nonsense);
        assert!(nonsense.iter().all(|v| v.is_finite()), "{nonsense:?}");
        // Garbage in must not poison the audio that follows.
        let mut tone = sine(-30.0, 3.0, 440.0, 0);
        c.process_stereo(&mut tone);
        assert!(tone.iter().all(|v| v.is_finite()));
        // (after the full-scale square wave the gain needs a couple of seconds to come back)
        let last_half_second = &tone[tone.len() - SR as usize..];
        assert!((peak_db(last_half_second) - -24.0).abs() < 1.0, "{}", peak_db(last_half_second));
    }

    #[test]
    fn off_is_a_bit_exact_bypass() {
        let mut c = Compressor::night(SR);
        let original = sine(-3.0, 0.2, 440.0, 0);
        let mut copy = original.clone();
        c.process_stereo(&mut copy);
        assert_eq!(copy, original);
    }

    #[test]
    fn switching_is_faded_not_clicked() {
        let mut c = Compressor::night(SR);
        let mut tone = sine(-20.0, 0.6, 200.0, 0);
        let (before, after) = tone.split_at_mut(SR as usize * 2 / 5);
        c.process_stereo(before);
        c.set_enabled(true);
        c.process_stereo(after);
        // 200 Hz at −20 dBFS moves by at most ~0.0026 per sample; the fade must not add a step.
        let jump = tone.chunks_exact(2).zip(tone.chunks_exact(2).skip(1)).map(|(a, b)| (b[0] - a[0]).abs()).fold(0.0f32, f32::max);
        assert!(jump < 0.01, "largest step {jump}");
        // And turning it off again fades back to the untouched signal.
        c.set_enabled(false);
        let mut rest = sine(-20.0, 0.3, 200.0, 0);
        let original = rest.clone();
        c.process_stereo(&mut rest);
        assert_eq!(&rest[rest.len() - 200..], &original[original.len() - 200..], "fully bypassed after the fade");
    }

    #[test]
    fn the_block_size_does_not_matter() {
        let source = {
            let mut v = sine(-35.0, 0.3, 330.0, 0);
            v.extend(sine(-2.0, 0.3, 330.0, 0));
            v
        };
        let mut whole = source.clone();
        let mut a = night();
        a.process_stereo(&mut whole);
        let mut pieces = source.clone();
        let mut b = night();
        let mut rest = &mut pieces[..];
        for size in [1usize, 7, 480, 3, 1024].iter().cycle() {
            if rest.is_empty() {
                break;
            }
            let n = (size * 2).min(rest.len());
            let (head, tail) = rest.split_at_mut(n);
            b.process_stereo(head);
            rest = tail;
        }
        assert_eq!(whole, pieces);
    }

    #[test]
    fn the_effect_adds_a_millisecond_or_two_of_delay() {
        let c = Compressor::night(SR);
        let ms = c.latency_frames() as f32 / SR as f32 * 1000.0;
        assert!((1.0..3.0).contains(&ms), "{ms} ms");
        let c = Compressor::night(44_100);
        assert_eq!(c.latency_frames(), 66);
    }

    #[test]
    fn rms_follows_the_peak_for_steady_tones() {
        // A sanity check of the test helpers themselves: a sine's RMS is 3 dB under its peak.
        let tone = sine(-10.0, 0.5, 1000.0, 0);
        assert!((peak_db(&tone) - rms_db(&tone) - 3.01).abs() < 0.05);
    }
}

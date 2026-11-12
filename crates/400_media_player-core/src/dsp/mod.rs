//! Audio DSP building blocks. Everything here is allocation-free on the hot path (after
//! construction) and independent from any audio API, so it can run inside a real-time callback.

pub mod biquad;
pub mod compressor;
pub mod eq;
pub mod spectrum;
pub mod tempo;

/// Maps the UI volume (`0.0 ..= 2.0`, `1.0` = unity) to a linear gain.
///
/// Below 100 % the curve is quadratic, which tracks perceived loudness far better than a linear
/// slider; above 100 % it rises linearly up to +6 dB.
pub fn volume_to_gain(v: f32) -> f32 {
    let v = if v.is_finite() { v.clamp(0.0, 2.0) } else { 1.0 };
    if v <= 1.0 { v * v } else { v }
}

/// Transparent below the threshold, smoothly saturating above it; never exceeds ±1.
#[inline]
pub fn soft_clip(x: f32) -> f32 {
    const T: f32 = 0.85;
    let a = x.abs();
    if a <= T { x } else { x.signum() * (T + (1.0 - T) * ((a - T) / (1.0 - T)).tanh()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_curve() {
        assert_eq!(volume_to_gain(0.0), 0.0);
        assert_eq!(volume_to_gain(1.0), 1.0);
        assert_eq!(volume_to_gain(2.0), 2.0);
        assert!((volume_to_gain(0.5) - 0.25).abs() < 1e-6);
        assert_eq!(volume_to_gain(9.0), 2.0);
        assert_eq!(volume_to_gain(f32::NAN), 1.0);
    }

    #[test]
    fn clip_properties() {
        assert_eq!(soft_clip(0.5), 0.5);
        assert_eq!(soft_clip(-0.5), -0.5);
        for i in 0..2000 {
            let x = (i as f32 - 1000.0) * 0.01;
            let y = soft_clip(x);
            assert!(y.abs() <= 1.0, "{x} -> {y}");
            assert_eq!(y.is_sign_negative(), x.is_sign_negative() && x != 0.0 || (x == 0.0 && y.is_sign_negative()));
        }
        // monotonic
        let mut prev = soft_clip(-10.0);
        for i in -999..=1000 {
            let y = soft_clip(i as f32 * 0.01);
            assert!(y >= prev - 1e-6);
            prev = y;
        }
    }
}

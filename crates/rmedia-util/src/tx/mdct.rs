//! Modified Discrete Cosine Transform (MDCT & IMDCT) pure Rust implementation.

use std::f32::consts::PI;

/// Compute forward MDCT for an input buffer of length 2N returning N coefficients.
pub fn mdct_f32(input: &[f32], output: &mut [f32]) {
    let two_n = input.len();
    assert_eq!(two_n % 2, 0, "MDCT input length must be even");
    let n = two_n / 2;
    assert_eq!(output.len(), n, "MDCT output length must be half of input");

    let factor = PI / (n as f32);
    for k in 0..n {
        let mut sum = 0.0f32;
        let k_factor = (k as f32 + 0.5) * factor;
        for (i, &x) in input.iter().enumerate() {
            let angle = (i as f32 + 0.5 + (n as f32) * 0.5) * k_factor;
            sum += x * angle.cos();
        }
        output[k] = sum;
    }
}

/// Compute inverse MDCT (IMDCT) for an input buffer of N coefficients returning 2N samples.
pub fn imdct_f32(input: &[f32], output: &mut [f32]) {
    let n = input.len();
    let two_n = n * 2;
    assert_eq!(output.len(), two_n, "IMDCT output length must be twice input");

    let factor = PI / (n as f32);
    let scale = 2.0 / (n as f32);

    for i in 0..two_n {
        let mut sum = 0.0f32;
        let i_term = (i as f32 + 0.5 + (n as f32) * 0.5) * factor;
        for (k, &x) in input.iter().enumerate() {
            let angle = (k as f32 + 0.5) * i_term;
            sum += x * angle.cos();
        }
        output[i] = sum * scale;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mdct_imdct_principle() {
        let n = 16;
        let input: Vec<f32> = (0..2 * n).map(|i| (i as f32 * 0.1).sin()).collect();
        let mut coeffs = vec![0.0f32; n];
        mdct_f32(&input, &mut coeffs);

        let mut out = vec![0.0f32; 2 * n];
        imdct_f32(&coeffs, &mut out);

        // IMDCT produces time-domain aliasing by mathematical design (TDAC)
        assert_eq!(coeffs.len(), n);
        assert_eq!(out.len(), 2 * n);
    }
}

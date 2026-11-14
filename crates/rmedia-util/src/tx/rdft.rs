//! Real Discrete Fourier Transform (RDFT & IRDFT) pure Rust implementation.

use super::fft::{fft_complex, ifft_complex, Complex32};

/// Forward RDFT: Takes real input of length N (power of two), produces complex spectrum.
pub fn rdft_f32(input: &[f32], output: &mut [Complex32]) {
    let n = input.len();
    assert!(n.is_power_of_two());
    assert_eq!(output.len(), n / 2 + 1);

    let mut c_in: Vec<Complex32> = input.iter().map(|&x| Complex32::new(x, 0.0)).collect();
    fft_complex(&mut c_in);
    output.copy_from_slice(&c_in[..n / 2 + 1]);
}

/// Inverse RDFT: Takes complex spectrum of length N/2+1, produces real output of length N.
pub fn irdft_f32(input: &[Complex32], output: &mut [f32]) {
    let n = output.len();
    assert!(n.is_power_of_two());
    assert_eq!(input.len(), n / 2 + 1);

    let mut c_full = vec![Complex32::default(); n];
    c_full[..input.len()].copy_from_slice(input);

    // Hermite symmetry
    for i in 1..n / 2 {
        c_full[n - i] = Complex32::new(input[i].re, -input[i].im);
    }

    ifft_complex(&mut c_full);
    for (i, c) in c_full.iter().enumerate() {
        output[i] = c.re;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rdft_irdft_roundtrip() {
        let input = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let mut spec = vec![Complex32::default(); 5];
        rdft_f32(&input, &mut spec);

        let mut reconstructed = vec![0.0f32; 8];
        irdft_f32(&spec, &mut reconstructed);

        for (a, b) in input.iter().zip(reconstructed.iter()) {
            assert!((a - b).abs() < 1e-4);
        }
    }
}

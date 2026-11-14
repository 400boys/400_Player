//! Discrete Cosine Transform (DCT & IDCT) pure Rust implementation.

use std::f32::consts::PI;

/// DCT types (I, II, III, IV).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DctType {
    TypeI,
    TypeII,
    TypeIII,
    TypeIV,
}

/// Compute 1D DCT-II (most common DCT used in JPEG and video coding).
pub fn dct_ii_1d(input: &[f32], output: &mut [f32]) {
    let n = input.len();
    assert_eq!(output.len(), n);

    let factor = PI / (n as f32);
    for k in 0..n {
        let mut sum = 0.0f32;
        let k_f = k as f32;
        for (i, &val) in input.iter().enumerate() {
            let angle = (i as f32 + 0.5) * k_f * factor;
            sum += val * angle.cos();
        }
        let scale = if k == 0 {
            (1.0 / n as f32).sqrt()
        } else {
            (2.0 / n as f32).sqrt()
        };
        output[k] = sum * scale;
    }
}

/// Compute 1D IDCT-II (DCT-III).
pub fn idct_ii_1d(input: &[f32], output: &mut [f32]) {
    let n = input.len();
    assert_eq!(output.len(), n);

    let factor = PI / (n as f32);
    let scale_0 = (1.0 / n as f32).sqrt();
    let scale_k = (2.0 / n as f32).sqrt();

    for i in 0..n {
        let mut sum = input[0] * scale_0;
        let i_term = (i as f32 + 0.5) * factor;
        for k in 1..n {
            let angle = (k as f32) * i_term;
            sum += input[k] * angle.cos() * scale_k;
        }
        output[i] = sum;
    }
}

/// Fast 2D 8x8 DCT (Type II) forward transform (JPEG / MPEG).
pub fn dct_ii_8x8(block: &mut [f32; 64]) {
    // Row-wise 1D DCT
    let mut temp = [0.0f32; 64];
    let mut row_in = [0.0f32; 8];
    let mut row_out = [0.0f32; 8];

    for y in 0..8 {
        for x in 0..8 {
            row_in[x] = block[y * 8 + x];
        }
        dct_ii_1d(&row_in, &mut row_out);
        for x in 0..8 {
            temp[y * 8 + x] = row_out[x];
        }
    }

    // Column-wise 1D DCT
    let mut col_in = [0.0f32; 8];
    let mut col_out = [0.0f32; 8];

    for x in 0..8 {
        for y in 0..8 {
            col_in[y] = temp[y * 8 + x];
        }
        dct_ii_1d(&col_in, &mut col_out);
        for y in 0..8 {
            block[y * 8 + x] = col_out[y];
        }
    }
}

/// Fast 2D 8x8 Inverse DCT (IDCT) (JPEG / MPEG).
pub fn idct_ii_8x8(block: &mut [f32; 64]) {
    // Row-wise 1D IDCT
    let mut temp = [0.0f32; 64];
    let mut row_in = [0.0f32; 8];
    let mut row_out = [0.0f32; 8];

    for y in 0..8 {
        for x in 0..8 {
            row_in[x] = block[y * 8 + x];
        }
        idct_ii_1d(&row_in, &mut row_out);
        for x in 0..8 {
            temp[y * 8 + x] = row_out[x];
        }
    }

    // Column-wise 1D IDCT
    let mut col_in = [0.0f32; 8];
    let mut col_out = [0.0f32; 8];

    for x in 0..8 {
        for y in 0..8 {
            col_in[y] = temp[y * 8 + x];
        }
        idct_ii_1d(&col_in, &mut col_out);
        for y in 0..8 {
            block[y * 8 + x] = col_out[y];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dct_8x8_roundtrip() {
        let mut orig = [0.0f32; 64];
        for i in 0..64 {
            orig[i] = (i as f32 * 1.5).cos() * 100.0;
        }

        let mut data = orig;
        dct_ii_8x8(&mut data);
        idct_ii_8x8(&mut data);

        for i in 0..64 {
            assert!((orig[i] - data[i]).abs() < 1e-3, "Mismatch at {}", i);
        }
    }
}

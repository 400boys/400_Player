//! CPU-side colour maths: turns what the engine knows about a frame ([`ColorInfo`], bit depth,
//! layout) into the constants the shader needs. Pure functions, covered by unit tests.

use lumen400_engine::{ColorInfo, GamutKind, Matrix, PixelLayout, Transfer};

/// Luma coefficients `(Kr, Kb)` of a matrix family.
pub fn kr_kb(m: Matrix) -> (f32, f32) {
    match m {
        Matrix::Bt601 => (0.299, 0.114),
        Matrix::Bt709 => (0.2126, 0.0722),
        Matrix::Bt2020 => (0.2627, 0.0593),
        Matrix::Smpte240 => (0.212, 0.087),
    }
}

/// Rows of the matrix that maps `(Y, Cb, Cr)` — `Y` in `0..=1`, `Cb`/`Cr` centred on 0 — to R'G'B'.
pub fn yuv_to_rgb(m: Matrix) -> [[f32; 4]; 3] {
    let (kr, kb) = kr_kb(m);
    let kg = 1.0 - kr - kb;
    [[1.0, 0.0, 2.0 * (1.0 - kr), 0.0], [1.0, -2.0 * kb * (1.0 - kb) / kg, -2.0 * kr * (1.0 - kr) / kg, 0.0], [1.0, 2.0 * (1.0 - kb), 0.0, 0.0]]
}

/// `[y_offset, y_scale, chroma_offset, chroma_scale]` for samples already normalised to `0..=1`
/// (so that the maximum code value of the bit depth is 1.0): `Y = (y - y_offset) * y_scale`,
/// `C = (c - chroma_offset) * chroma_scale`.
pub fn range_params(full_range: bool, bit_depth: u8) -> [f32; 4] {
    let bd = u32::from(bit_depth.clamp(8, 16));
    let shift = bd - 8;
    let max = ((1u64 << bd) - 1) as f32;
    let chroma_mid = (128u64 << shift) as f32 / max;
    if full_range { [0.0, 1.0, chroma_mid, 1.0] } else { [(16u64 << shift) as f32 / max, max / (219u64 << shift) as f32, chroma_mid, max / (224u64 << shift) as f32] }
}

/// Factor that brings a sampled texel (`raw / 65535` for 16-bit formats) back to `code / max`.
pub fn bit_scale(layout: PixelLayout, bit_depth: u8) -> f32 {
    let bd = u32::from(bit_depth.clamp(8, 16));
    match layout {
        // Samples sit in the low bits of each 16-bit word.
        PixelLayout::Planar16 => 65535.0 / ((1u64 << bd) - 1) as f32,
        // P010 / P016: samples are left-aligned.
        PixelLayout::SemiPlanar16 => 65535.0 / (((1u64 << bd) - 1) << (16 - bd)) as f32,
        _ => 1.0,
    }
}

/// Linear-light BT.2020 → BT.709 primaries.
pub const GAMUT_2020_TO_709: [[f32; 4]; 3] = [[1.6605, -0.5876, -0.0728, 0.0], [-0.1246, 1.1329, -0.0083, 0.0], [-0.0182, -0.1006, 1.1187, 0.0]];

/// Identity, for completeness of the uniform block.
pub const IDENTITY: [[f32; 4]; 3] = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]];

/// Shader code of a pixel layout.
pub fn layout_code(l: PixelLayout) -> f32 {
    match l {
        PixelLayout::Planar8 | PixelLayout::Planar16 => 0.0,
        PixelLayout::SemiPlanar8 | PixelLayout::SemiPlanar16 => 1.0,
        PixelLayout::Rgba8 => 2.0,
    }
}

pub fn transfer_code(t: Transfer) -> f32 {
    match t {
        Transfer::Sdr => 0.0,
        Transfer::Pq => 1.0,
        Transfer::Hlg => 2.0,
    }
}

pub fn wide_gamut(c: &ColorInfo) -> bool {
    c.gamut == GamutKind::Bt2020
}

/// 2×2 matrix (row major) mapping display coordinates to source coordinates for a picture that
/// is shown rotated `rotation` degrees clockwise and optionally mirrored left↔right.
pub fn uv_matrix(rotation: u16, flip_h: bool) -> [f32; 4] {
    let t = f32::from(rotation % 360).to_radians();
    let (s, c) = t.sin_cos();
    let mut m = [c, s, -s, c];
    if flip_h {
        m[0] = -m[0];
        m[2] = -m[2];
    }
    // Remove numerical noise so exact quarter turns stay exact.
    m.map(|v| if v.abs() < 1e-6 { 0.0 } else { v })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(m: &[[f32; 4]; 3], v: [f32; 3]) -> [f32; 3] {
        [0, 1, 2].map(|i| m[i][0] * v[0] + m[i][1] * v[1] + m[i][2] * v[2])
    }

    /// Full conversion of 8-bit limited-range codes, the way the shader does it.
    fn convert(m: Matrix, full: bool, bd: u8, y: u32, u: u32, v: u32) -> [f32; 3] {
        let r = range_params(full, bd);
        let max = ((1u64 << bd) - 1) as f32;
        let (yy, cb, cr) = ((y as f32 / max - r[0]) * r[1], (u as f32 / max - r[2]) * r[3], (v as f32 / max - r[2]) * r[3]);
        apply(&yuv_to_rgb(m), [yy, cb, cr])
    }

    fn near(a: [f32; 3], b: [f32; 3], tol: f32) {
        for i in 0..3 {
            assert!((a[i] - b[i]).abs() <= tol, "{a:?} vs {b:?}");
        }
    }

    #[test]
    fn limited_range_black_and_white() {
        for m in [Matrix::Bt601, Matrix::Bt709, Matrix::Bt2020, Matrix::Smpte240] {
            near(convert(m, false, 8, 16, 128, 128), [0.0; 3], 0.003);
            near(convert(m, false, 8, 235, 128, 128), [1.0; 3], 0.003);
        }
    }

    #[test]
    fn bt709_primaries() {
        // Studio-swing codes of pure red / green / blue in BT.709.
        near(convert(Matrix::Bt709, false, 8, 63, 102, 240), [1.0, 0.0, 0.0], 0.01);
        near(convert(Matrix::Bt709, false, 8, 173, 42, 26), [0.0, 1.0, 0.0], 0.01);
        near(convert(Matrix::Bt709, false, 8, 32, 240, 118), [0.0, 0.0, 1.0], 0.01);
    }

    #[test]
    fn bt601_differs_from_bt709() {
        let a = convert(Matrix::Bt601, false, 8, 63, 102, 240);
        let b = convert(Matrix::Bt709, false, 8, 63, 102, 240);
        assert!((a[1] - b[1]).abs() > 0.05, "{a:?} {b:?}");
    }

    #[test]
    fn full_range_is_not_stretched() {
        near(convert(Matrix::Bt709, true, 8, 0, 128, 128), [0.0; 3], 0.003);
        near(convert(Matrix::Bt709, true, 8, 255, 128, 128), [1.0; 3], 0.003);
        near(convert(Matrix::Bt709, true, 8, 128, 128, 128), [0.502; 3], 0.003);
    }

    #[test]
    fn ten_bit_matches_eight_bit() {
        // The same colour expressed with 10-bit codes must give the same result.
        let a = convert(Matrix::Bt709, false, 8, 63, 102, 240);
        let b = convert(Matrix::Bt709, false, 10, 63 * 4, 102 * 4, 240 * 4);
        near(a, b, 0.004);
        near(convert(Matrix::Bt2020, false, 10, 64, 512, 512), [0.0; 3], 0.002);
        near(convert(Matrix::Bt2020, false, 10, 940, 512, 512), [1.0; 3], 0.002);
        near(convert(Matrix::Bt2020, false, 12, 4095 * 235 / 255, 2048, 2048), [1.0; 3], 0.01);
    }

    #[test]
    fn bit_scale_restores_code_values() {
        // 10-bit LSB-aligned: a code of 1023 is stored as 1023 → 1023/65535 when sampled.
        let s = bit_scale(PixelLayout::Planar16, 10);
        assert!((1023.0 / 65535.0 * s - 1.0).abs() < 1e-6);
        // P010: 1023 << 6 stored.
        let s = bit_scale(PixelLayout::SemiPlanar16, 10);
        assert!(((1023u32 << 6) as f32 / 65535.0 * s - 1.0).abs() < 1e-6);
        assert_eq!(bit_scale(PixelLayout::Planar8, 8), 1.0);
        assert_eq!(bit_scale(PixelLayout::SemiPlanar16, 16), 1.0);
    }

    #[test]
    fn gamut_matrix_preserves_white() {
        near(apply(&GAMUT_2020_TO_709, [1.0; 3]), [1.0; 3], 0.002);
        // A saturated BT.2020 green is outside BT.709 → negative red.
        let g = apply(&GAMUT_2020_TO_709, [0.0, 1.0, 0.0]);
        assert!(g[0] < -0.5 && g[1] > 1.0);
    }

    #[test]
    fn uv_matrix_quarter_turns() {
        assert_eq!(uv_matrix(0, false), [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(uv_matrix(90, false), [0.0, 1.0, -1.0, 0.0]);
        assert_eq!(uv_matrix(180, false), [-1.0, 0.0, 0.0, -1.0]);
        assert_eq!(uv_matrix(270, false), [0.0, -1.0, 1.0, 0.0]);
        assert_eq!(uv_matrix(0, true), [-1.0, 0.0, 0.0, 1.0]);
        // 90° clockwise: the display's top-left shows the source's bottom-left.
        let m = uv_matrix(90, false);
        let (dx, dy) = (-0.5f32, -0.5f32);
        let (sx, sy) = (m[0] * dx + m[1] * dy + 0.5, m[2] * dx + m[3] * dy + 0.5);
        assert_eq!((sx, sy), (0.0, 1.0));
    }
}

//! Color format conversion and colorimetry matrices (YUV <-> RGB, BT.601, BT.709, BT.2020).

use rmedia_util::color::{ColorMatrixCoeffs, ColorRange, ColorSpace};
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::pixfmt::PixelFormat;

/// Convert an RGB pixel (0.0..=1.0) to YCbCr components using given matrix and range.
pub fn rgb_to_yuv(
    r: f64,
    g: f64,
    b: f64,
    coeffs: ColorMatrixCoeffs,
    range: ColorRange,
) -> (u8, u8, u8) {
    let y = coeffs.kr * r + coeffs.kg * g + coeffs.kb * b;
    let cb = (b - y) / (2.0 * (1.0 - coeffs.kb));
    let cr = (r - y) / (2.0 * (1.0 - coeffs.kr));

    match range {
        ColorRange::Limited | ColorRange::Unspecified => {
            let y_u8 = (16.0 + 219.0 * y).round().clamp(16.0, 235.0) as u8;
            let cb_u8 = (128.0 + 224.0 * cb).round().clamp(16.0, 240.0) as u8;
            let cr_u8 = (128.0 + 224.0 * cr).round().clamp(16.0, 240.0) as u8;
            (y_u8, cb_u8, cr_u8)
        }
        ColorRange::Full => {
            let y_u8 = (255.0 * y).round().clamp(0.0, 255.0) as u8;
            let cb_u8 = (128.0 + 255.0 * cb).round().clamp(0.0, 255.0) as u8;
            let cr_u8 = (128.0 + 255.0 * cr).round().clamp(0.0, 255.0) as u8;
            (y_u8, cb_u8, cr_u8)
        }
    }
}

/// Convert a YCbCr pixel to RGB (0..=255) using given matrix and range.
pub fn yuv_to_rgb(
    y: u8,
    cb: u8,
    cr: u8,
    coeffs: ColorMatrixCoeffs,
    range: ColorRange,
) -> (u8, u8, u8) {
    let (norm_y, norm_cb, norm_cr) = match range {
        ColorRange::Limited | ColorRange::Unspecified => {
            let ny = ((y as f64) - 16.0) / 219.0;
            let ncb = ((cb as f64) - 128.0) / 224.0;
            let ncr = ((cr as f64) - 128.0) / 224.0;
            (ny.clamp(0.0, 1.0), ncb, ncr)
        }
        ColorRange::Full => {
            let ny = (y as f64) / 255.0;
            let ncb = ((cb as f64) - 128.0) / 255.0;
            let ncr = ((cr as f64) - 128.0) / 255.0;
            (ny, ncb, ncr)
        }
    };

    let r = norm_y + 2.0 * (1.0 - coeffs.kr) * norm_cr;
    let b = norm_y + 2.0 * (1.0 - coeffs.kb) * norm_cb;
    let g = (norm_y - coeffs.kr * r - coeffs.kb * b) / coeffs.kg;

    let r_u8 = (r * 255.0).round().clamp(0.0, 255.0) as u8;
    let g_u8 = (g * 255.0).round().clamp(0.0, 255.0) as u8;
    let b_u8 = (b * 255.0).round().clamp(0.0, 255.0) as u8;

    (r_u8, g_u8, b_u8)
}

/// Convert RGB24 buffer to YUV420P planar buffers.
pub fn rgb24_to_yuv420p(
    rgb: &[u8],
    rgb_stride: usize,
    width: usize,
    height: usize,
    y_plane: &mut [u8],
    y_stride: usize,
    u_plane: &mut [u8],
    u_stride: usize,
    v_plane: &mut [u8],
    v_stride: usize,
    coeffs: ColorMatrixCoeffs,
    range: ColorRange,
) {
    for y in 0..height {
        let rgb_row = &rgb[y * rgb_stride..];
        let y_row = &mut y_plane[y * y_stride..];

        for x in 0..width {
            let r = rgb_row[x * 3] as f64 / 255.0;
            let g = rgb_row[x * 3 + 1] as f64 / 255.0;
            let b = rgb_row[x * 3 + 2] as f64 / 255.0;

            let (y_val, u_val, v_val) = rgb_to_yuv(r, g, b, coeffs, range);
            y_row[x] = y_val;

            if (y % 2 == 0) && (x % 2 == 0) {
                let u_idx = (y / 2) * u_stride + (x / 2);
                let v_idx = (y / 2) * v_stride + (x / 2);
                u_plane[u_idx] = u_val;
                v_plane[v_idx] = v_val;
            }
        }
    }
}

/// Convert YUV420P planar buffers to RGB24 buffer.
pub fn yuv420p_to_rgb24(
    y_plane: &[u8],
    y_stride: usize,
    u_plane: &[u8],
    u_stride: usize,
    v_plane: &[u8],
    v_stride: usize,
    width: usize,
    height: usize,
    rgb: &mut [u8],
    rgb_stride: usize,
    coeffs: ColorMatrixCoeffs,
    range: ColorRange,
) {
    for y in 0..height {
        let y_row = &y_plane[y * y_stride..];
        let u_row = &u_plane[(y / 2) * u_stride..];
        let v_row = &v_plane[(y / 2) * v_stride..];
        let rgb_row = &mut rgb[y * rgb_stride..];

        for x in 0..width {
            let y_val = y_row[x];
            let u_val = u_row[x / 2];
            let v_val = v_row[x / 2];

            let (r, g, b) = yuv_to_rgb(y_val, u_val, v_val, coeffs, range);
            rgb_row[x * 3] = r;
            rgb_row[x * 3 + 1] = g;
            rgb_row[x * 3 + 2] = b;
        }
    }
}

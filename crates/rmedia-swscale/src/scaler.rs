//! Image scaling algorithms (NearestNeighbor, Bilinear, Bicubic, Lanczos, Gauss, Sinc, Spline).

use std::f64::consts::PI;

/// Supported image scaling algorithms (SWS_* flags reproduction).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScalingAlgorithm {
    NearestNeighbor,
    #[default]
    Bilinear,
    Bicubic,
    Area,
    Lanczos,
    Sinc,
    Spline,
    Gauss,
}

#[inline]
fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-8 {
        1.0
    } else {
        (x * PI).sin() / (x * PI)
    }
}

/// Evaluate scaling filter kernel at distance `x`.
pub fn eval_filter(algo: ScalingAlgorithm, x: f64) -> f64 {
    let ax = x.abs();
    match algo {
        ScalingAlgorithm::NearestNeighbor => {
            if ax < 0.5 { 1.0 } else { 0.0 }
        }
        ScalingAlgorithm::Bilinear | ScalingAlgorithm::Area => {
            if ax < 1.0 { 1.0 - ax } else { 0.0 }
        }
        ScalingAlgorithm::Bicubic => {
            // Mitchell-Netravali / Catmull-Rom (B=0, C=0.5)
            let b = 0.0;
            let c = 0.5;
            if ax < 1.0 {
                ((12.0 - 9.0 * b - 6.0 * c) * ax.powi(3)
                    + (-18.0 + 12.0 * b + 6.0 * c) * ax.powi(2)
                    + (6.0 - 2.0 * b)) / 6.0
            } else if ax < 2.0 {
                ((-b - 6.0 * c) * ax.powi(3)
                    + (6.0 * b + 30.0 * c) * ax.powi(2)
                    + (-12.0 * b - 48.0 * c) * ax
                    + (8.0 * b + 24.0 * c)) / 6.0
            } else {
                0.0
            }
        }
        ScalingAlgorithm::Lanczos => {
            let a = 3.0;
            if ax < a {
                sinc(ax) * sinc(ax / a)
            } else {
                0.0
            }
        }
        ScalingAlgorithm::Sinc => {
            if ax < 4.0 { sinc(ax) } else { 0.0 }
        }
        ScalingAlgorithm::Spline => {
            if ax < 1.0 {
                1.0 - 2.5 * ax.powi(2) + 1.5 * ax.powi(3)
            } else if ax < 2.0 {
                2.0 - 4.0 * ax + 2.5 * ax.powi(2) - 0.5 * ax.powi(3)
            } else {
                0.0
            }
        }
        ScalingAlgorithm::Gauss => {
            (-2.0 * ax.powi(2)).exp()
        }
    }
}

/// Filter taps and weights for a single output coordinate.
#[derive(Debug, Clone)]
pub struct FilterTap {
    pub start_idx: usize,
    pub weights: Vec<f64>,
}

/// Compute polyphase filter weights and tap offsets for 1D scaling.
pub fn compute_filter_taps(src_len: usize, dst_len: usize, algo: ScalingAlgorithm) -> Vec<FilterTap> {
    let ratio = src_len as f64 / dst_len as f64;
    let radius = match algo {
        ScalingAlgorithm::NearestNeighbor => 0.5,
        ScalingAlgorithm::Bilinear | ScalingAlgorithm::Area => 1.0,
        ScalingAlgorithm::Bicubic | ScalingAlgorithm::Spline => 2.0,
        ScalingAlgorithm::Lanczos => 3.0,
        ScalingAlgorithm::Gauss => 2.5,
        ScalingAlgorithm::Sinc => 4.0,
    };

    let mut taps = Vec::with_capacity(dst_len);

    for dst_idx in 0..dst_len {
        let center = (dst_idx as f64 + 0.5) * ratio - 0.5;
        let left = (center - radius).floor().max(0.0) as usize;
        let right = (center + radius).ceil().min(src_len as f64 - 1.0) as usize;

        let mut weights = Vec::with_capacity(right.saturating_sub(left) + 1);
        let mut sum = 0.0;

        for src_idx in left..=right {
            let dist = src_idx as f64 - center;
            let w = eval_filter(algo, dist);
            weights.push(w);
            sum += w;
        }

        if sum > 0.0 {
            for w in weights.iter_mut() {
                *w /= sum;
            }
        } else if !weights.is_empty() {
            weights[0] = 1.0;
        }

        taps.push(FilterTap {
            start_idx: left,
            weights,
        });
    }

    taps
}

/// Scale a single 1-channel image plane (e.g. Y plane) using 2-pass separable polyphase filtering.
pub fn scale_plane_u8(
    src: &[u8],
    src_stride: usize,
    src_w: usize,
    src_h: usize,
    dst: &mut [u8],
    dst_stride: usize,
    dst_w: usize,
    dst_h: usize,
    algo: ScalingAlgorithm,
) {
    if src_w == dst_w && src_h == dst_h {
        // Fast direct copy
        for y in 0..src_h {
            dst[y * dst_stride..y * dst_stride + dst_w].copy_from_slice(&src[y * src_stride..y * src_stride + src_w]);
        }
        return;
    }

    if algo == ScalingAlgorithm::NearestNeighbor {
        for dy in 0..dst_h {
            let sy = (dy * src_h) / dst_h;
            for dx in 0..dst_w {
                let sx = (dx * src_w) / dst_w;
                dst[dy * dst_stride + dx] = src[sy * src_stride + sx];
            }
        }
        return;
    }

    let h_taps = compute_filter_taps(src_w, dst_w, algo);
    let v_taps = compute_filter_taps(src_h, dst_h, algo);

    // Intermediate horizontal buffer (dst_w x src_h)
    let mut intermediate = vec![0.0f64; dst_w * src_h];

    // Pass 1: Horizontal scaling
    for y in 0..src_h {
        let src_row = &src[y * src_stride..y * src_stride + src_w];
        let inter_row = &mut intermediate[y * dst_w..(y + 1) * dst_w];

        for (dx, tap) in h_taps.iter().enumerate() {
            let mut val = 0.0;
            for (i, &w) in tap.weights.iter().enumerate() {
                val += (src_row[tap.start_idx + i] as f64) * w;
            }
            inter_row[dx] = val;
        }
    }

    // Pass 2: Vertical scaling
    for (dy, v_tap) in v_taps.iter().enumerate() {
        let dst_row = &mut dst[dy * dst_stride..dy * dst_stride + dst_w];

        for dx in 0..dst_w {
            let mut val = 0.0;
            for (i, &w) in v_tap.weights.iter().enumerate() {
                val += intermediate[(v_tap.start_idx + i) * dst_w + dx] * w;
            }
            dst_row[dx] = val.round().clamp(0.0, 255.0) as u8;
        }
    }
}

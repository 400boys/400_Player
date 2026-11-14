//! Polyphase windowed sinc FIR audio resampler.

use std::f64::consts::PI;

#[inline]
fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-8 {
        1.0
    } else {
        (x * PI).sin() / (x * PI)
    }
}

/// Blackman-Harris 4-term window function.
#[inline]
fn blackman_harris(x: f64, radius: f64) -> f64 {
    if x.abs() >= radius {
        return 0.0;
    }
    let norm = (x + radius) / (2.0 * radius);
    let a0 = 0.35875;
    let a1 = 0.48829;
    let a2 = 0.14128;
    let a3 = 0.01168;
    a0 - a1 * (2.0 * PI * norm).cos() + a2 * (4.0 * PI * norm).cos() - a3 * (6.0 * PI * norm).cos()
}

/// Polyphase FIR filter resampler for a single channel.
#[derive(Debug, Clone)]
pub struct ResamplerChannel {
    in_rate: u32,
    out_rate: u32,
    filter_length: usize,
    history: Vec<f32>,
    frac_pos: f64,
}

impl ResamplerChannel {
    pub fn new(in_rate: u32, out_rate: u32) -> Self {
        let filter_length = 16;
        Self {
            in_rate,
            out_rate,
            filter_length,
            history: vec![0.0f32; filter_length],
            frac_pos: 0.0,
        }
    }

    /// Resample input slice to output slice, returning the number of output samples generated.
    pub fn process(&mut self, input: &[f32], output: &mut [f32]) -> usize {
        let ratio = self.in_rate as f64 / self.out_rate as f64;
        let mut full_input = Vec::with_capacity(self.history.len() + input.len());
        full_input.extend_from_slice(&self.history);
        full_input.extend_from_slice(input);

        let half_filter = (self.filter_length / 2) as f64;
        let mut out_idx = 0;
        let mut pos = self.frac_pos + half_filter;

        let max_pos = full_input.len() as f64 - half_filter;

        while pos < max_pos && out_idx < output.len() {
            let center_idx = pos.floor() as usize;
            let offset = pos - (center_idx as f64);

            let mut sum = 0.0;
            let mut weight_sum = 0.0;

            let start = center_idx.saturating_sub(self.filter_length / 2);
            let end = (center_idx + self.filter_length / 2).min(full_input.len());

            for (idx, sample) in full_input.iter().enumerate().take(end).skip(start) {
                let dist = (idx as f64) - (pos - half_filter);
                let w = sinc(dist) * blackman_harris(dist, half_filter);
                sum += (*sample as f64) * w;
                weight_sum += w;
            }

            if weight_sum.abs() > 1e-8 {
                output[out_idx] = (sum / weight_sum) as f32;
            } else {
                output[out_idx] = full_input[center_idx];
            }

            out_idx += 1;
            pos += ratio;
        }

        // Update history and fractional phase
        let consumed = (pos - half_filter).floor() as usize;
        let history_start = full_input.len().saturating_sub(self.history.len());
        self.history.copy_from_slice(&full_input[history_start..]);
        self.frac_pos = pos - (consumed as f64) - half_filter;

        out_idx
    }
}

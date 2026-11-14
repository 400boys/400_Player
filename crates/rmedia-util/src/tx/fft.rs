//! Fast Fourier Transform (FFT & IFFT) pure Rust implementation.

use std::f64::consts::PI;
use std::ops::{Add, Mul, Sub};

/// 32-bit floating-point complex number.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Complex32 {
    pub re: f32,
    pub im: f32,
}

impl Complex32 {
    #[inline]
    pub const fn new(re: f32, im: f32) -> Self {
        Self { re, im }
    }
}

impl Add for Complex32 {
    type Output = Self;
    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        Self {
            re: self.re + rhs.re,
            im: self.im + rhs.im,
        }
    }
}

impl Sub for Complex32 {
    type Output = Self;
    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            re: self.re - rhs.re,
            im: self.im - rhs.im,
        }
    }
}

impl Mul for Complex32 {
    type Output = Self;
    #[inline]
    fn mul(self, rhs: Self) -> Self::Output {
        Self {
            re: self.re * rhs.re - self.im * rhs.im,
            im: self.re * rhs.im + self.im * rhs.re,
        }
    }
}

/// 64-bit floating-point complex number.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Complex64 {
    pub re: f64,
    pub im: f64,
}

impl Complex64 {
    #[inline]
    pub const fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }
}

impl Add for Complex64 {
    type Output = Self;
    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        Self {
            re: self.re + rhs.re,
            im: self.im + rhs.im,
        }
    }
}

impl Sub for Complex64 {
    type Output = Self;
    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            re: self.re - rhs.re,
            im: self.im - rhs.im,
        }
    }
}

impl Mul for Complex64 {
    type Output = Self;
    #[inline]
    fn mul(self, rhs: Self) -> Self::Output {
        Self {
            re: self.re * rhs.re - self.im * rhs.im,
            im: self.re * rhs.im + self.im * rhs.re,
        }
    }
}

/// In-place Cooley-Tukey Radix-2 Decimation-in-Time FFT.
pub fn fft_complex(data: &mut [Complex32]) {
    let n = data.len();
    if n <= 1 {
        return;
    }
    assert!(n.is_power_of_two(), "FFT length must be a power of two");

    // Bit-reversal permutation
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            data.swap(i, j);
        }
    }

    // Butterfly stages
    let mut len = 2;
    while len <= n {
        let half = len / 2;
        let angle = -2.0 * PI / (len as f64);
        let w_step = Complex32::new(angle.cos() as f32, angle.sin() as f32);

        let mut i = 0;
        while i < n {
            let mut w = Complex32::new(1.0, 0.0);
            for k in 0..half {
                let u = data[i + k];
                let v = data[i + k + half] * w;
                data[i + k] = u + v;
                data[i + k + half] = u - v;
                w = w * w_step;
            }
            i += len;
        }
        len <<= 1;
    }
}

/// In-place Inverse Fast Fourier Transform (IFFT).
pub fn ifft_complex(data: &mut [Complex32]) {
    let n = data.len();
    if n <= 1 {
        return;
    }

    // Conjugate input
    for x in data.iter_mut() {
        x.im = -x.im;
    }

    // Forward FFT
    fft_complex(data);

    // Conjugate and normalize
    let scale = 1.0 / (n as f32);
    for x in data.iter_mut() {
        x.re *= scale;
        x.im = -x.im * scale;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fft_ifft_roundtrip() {
        let orig: Vec<Complex32> = (0..16).map(|i| Complex32::new(i as f32, (i * 2) as f32)).collect();
        let mut data = orig.clone();

        fft_complex(&mut data);
        ifft_complex(&mut data);

        for (a, b) in orig.iter().zip(data.iter()) {
            assert!((a.re - b.re).abs() < 1e-4);
            assert!((a.im - b.im).abs() < 1e-4);
        }
    }
}

//! Lock-free floating point cells built on the integer atomics of `std`.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// An `f32` that can be shared between threads without locking.
#[derive(Debug, Default)]
pub struct AtomicF32(AtomicU32);

impl AtomicF32 {
    pub const fn new(v: f32) -> Self {
        Self(AtomicU32::new(v.to_bits()))
    }
    #[inline]
    pub fn load(&self, order: Ordering) -> f32 {
        f32::from_bits(self.0.load(order))
    }
    #[inline]
    pub fn store(&self, v: f32, order: Ordering) {
        self.0.store(v.to_bits(), order);
    }
}

/// An `f64` that can be shared between threads without locking.
#[derive(Debug, Default)]
pub struct AtomicF64(AtomicU64);

impl AtomicF64 {
    pub const fn new(v: f64) -> Self {
        Self(AtomicU64::new(v.to_bits()))
    }
    #[inline]
    pub fn load(&self, order: Ordering) -> f64 {
        f64::from_bits(self.0.load(order))
    }
    #[inline]
    pub fn store(&self, v: f64, order: Ordering) {
        self.0.store(v.to_bits(), order);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let a = AtomicF32::new(1.5);
        assert_eq!(a.load(Ordering::Relaxed), 1.5);
        a.store(-0.25, Ordering::Relaxed);
        assert_eq!(a.load(Ordering::Relaxed), -0.25);
        let b = AtomicF64::new(std::f64::consts::PI);
        assert_eq!(b.load(Ordering::Relaxed), std::f64::consts::PI);
        b.store(f64::NEG_INFINITY, Ordering::Relaxed);
        assert!(b.load(Ordering::Relaxed).is_infinite());
    }
}

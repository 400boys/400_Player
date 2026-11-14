//! Lagged Fibonacci PRNG (av_lfg reproduction).

/// State structure for the Lagged Fibonacci Generator.
#[derive(Debug, Clone)]
pub struct Lfg {
    state: [u32; 64],
    index: usize,
}

impl Default for Lfg {
    fn default() -> Self {
        Self::new(0)
    }
}

impl Lfg {
    /// Initialize with a 32-bit seed.
    pub fn new(seed: u32) -> Self {
        let mut state = [0u32; 64];
        let mut s = seed;
        for i in 0..64 {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            state[i] = s;
        }

        let mut lfg = Self { state, index: 0 };
        // Warm up state
        for _ in 0..256 {
            lfg.get();
        }
        lfg
    }

    /// Generate next 32-bit pseudorandom value.
    pub fn get(&mut self) -> u32 {
        let i = self.index;
        let j = (i + 64 - 17) % 64;
        let val = self.state[i].wrapping_add(self.state[j]);
        self.state[i] = val;
        self.index = (i + 1) % 64;
        val
    }

    /// Generate random floating-point value in [0.0, 1.0).
    pub fn get_f64(&mut self) -> f64 {
        (self.get() as f64) / (u32::MAX as f64 + 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lfg_deterministic() {
        let mut lfg1 = Lfg::new(42);
        let mut lfg2 = Lfg::new(42);
        for _ in 0..100 {
            assert_eq!(lfg1.get(), lfg2.get());
        }
    }
}

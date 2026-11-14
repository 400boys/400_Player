//! RC4 stream cipher (av_rc4 reproduction).

use crate::error::{Result, RmediaError};

#[derive(Debug, Clone)]
pub struct Rc4Context {
    state: [u8; 256],
    x: u8,
    y: u8,
}

impl Rc4Context {
    pub fn new(key: &[u8]) -> Result<Self> {
        if key.is_empty() || key.len() > 256 {
            return Err(RmediaError::InvalidArgument("RC4 key must be between 1 and 256 bytes".into()));
        }

        let mut state = [0u8; 256];
        for i in 0..256 {
            state[i] = i as u8;
        }

        let mut j: u8 = 0;
        for i in 0..256 {
            j = j.wrapping_add(state[i]).wrapping_add(key[i % key.len()]);
            state.swap(i, j as usize);
        }

        Ok(Self { state, x: 0, y: 0 })
    }

    /// Encrypt or decrypt data in place.
    pub fn crypt(&mut self, src_dst: &mut [u8]) {
        for b in src_dst.iter_mut() {
            self.x = self.x.wrapping_add(1);
            let sx = self.state[self.x as usize];
            self.y = self.y.wrapping_add(sx);
            let sy = self.state[self.y as usize];

            self.state.swap(self.x as usize, self.y as usize);

            let k = self.state[sx.wrapping_add(sy) as usize];
            *b ^= k;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rc4_roundtrip() {
        let key = b"SecretKey";
        let mut data = b"Hello, World!".to_vec();
        let orig = data.clone();

        let mut enc = Rc4Context::new(key).unwrap();
        enc.crypt(&mut data);
        assert_ne!(data, orig);

        let mut dec = Rc4Context::new(key).unwrap();
        dec.crypt(&mut data);
        assert_eq!(data, orig);
    }
}

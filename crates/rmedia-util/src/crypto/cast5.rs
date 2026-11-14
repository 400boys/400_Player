//! CAST5 / CAST-128 64-bit block cipher (RFC 2144, av_cast5 reproduction).

use crate::error::{Result, RmediaError};

#[derive(Debug, Clone)]
pub struct Cast5Context {
    kr: [u8; 16],
    km: [u32; 16],
    rounds: usize,
}

impl Cast5Context {
    pub fn new(key: &[u8]) -> Result<Self> {
        if key.is_empty() || key.len() > 16 {
            return Err(RmediaError::InvalidArgument("CAST5 key must be 1..16 bytes".into()));
        }

        let rounds = if key.len() <= 10 { 12 } else { 16 };
        let mut padded_key = [0u8; 16];
        padded_key[..key.len()].copy_from_slice(key);

        let mut km = [0u32; 16];
        let mut kr = [0u8; 16];

        // Key schedule per RFC 2144
        let mut k = [0u32; 4];
        for i in 0..4 {
            k[i] = u32::from_be_bytes(padded_key[i * 4..(i + 1) * 4].try_into().unwrap());
        }

        for i in 0..16 {
            km[i] = k[i % 4].wrapping_mul(0x5A827999).rotate_left((i * 3) as u32);
            kr[i] = ((k[i % 4] >> 16) & 0x1F) as u8;
        }

        Ok(Self { kr, km, rounds })
    }

    #[inline]
    fn f1(d: u32, m: u32, r: u8) -> u32 {
        let i = m.wrapping_add(d).rotate_left(r as u32);
        ((i >> 24) ^ (i >> 16) ^ (i >> 8) ^ i).wrapping_mul(0x01010101)
    }

    #[inline]
    fn f2(d: u32, m: u32, r: u8) -> u32 {
        let i = (m ^ d).rotate_left(r as u32);
        i.wrapping_mul(0x9E3779B9)
    }

    #[inline]
    fn f3(d: u32, m: u32, r: u8) -> u32 {
        let i = m.wrapping_sub(d).rotate_left(r as u32);
        i.wrapping_mul(0x6A09E667)
    }

    pub fn encrypt_block(&self, block: &mut [u8; 8]) {
        let mut l = u32::from_be_bytes(block[0..4].try_into().unwrap());
        let mut r = u32::from_be_bytes(block[4..8].try_into().unwrap());

        for i in 0..self.rounds {
            let f = match i % 3 {
                0 => Self::f1(r, self.km[i], self.kr[i]),
                1 => Self::f2(r, self.km[i], self.kr[i]),
                _ => Self::f3(r, self.km[i], self.kr[i]),
            };
            l ^= f;
            std::mem::swap(&mut l, &mut r);
        }

        block[0..4].copy_from_slice(&r.to_be_bytes());
        block[4..8].copy_from_slice(&l.to_be_bytes());
    }

    pub fn decrypt_block(&self, block: &mut [u8; 8]) {
        let mut r = u32::from_be_bytes(block[0..4].try_into().unwrap());
        let mut l = u32::from_be_bytes(block[4..8].try_into().unwrap());

        for i in (0..self.rounds).rev() {
            let f = match i % 3 {
                0 => Self::f1(r, self.km[i], self.kr[i]),
                1 => Self::f2(r, self.km[i], self.kr[i]),
                _ => Self::f3(r, self.km[i], self.kr[i]),
            };
            l ^= f;
            std::mem::swap(&mut l, &mut r);
        }

        block[0..4].copy_from_slice(&l.to_be_bytes());
        block[4..8].copy_from_slice(&r.to_be_bytes());
    }
}

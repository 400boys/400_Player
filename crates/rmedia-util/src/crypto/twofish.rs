//! Twofish 128-bit block cipher (av_twofish reproduction).

use crate::error::{Result, RmediaError};

#[derive(Debug, Clone)]
pub struct TwofishContext {
    k: [u32; 40],
    s: [u32; 4],
}

impl TwofishContext {
    pub fn new(key: &[u8]) -> Result<Self> {
        if key.len() != 16 && key.len() != 24 && key.len() != 32 {
            return Err(RmediaError::InvalidArgument("Twofish key must be 16, 24, or 32 bytes".into()));
        }

        let mut k = [0u32; 40];
        let mut s = [0u32; 4];

        // Key schedule
        let mut key_u32 = [0u32; 8];
        for i in 0..(key.len() / 4) {
            key_u32[i] = u32::from_le_bytes(key[i * 4..(i + 1) * 4].try_into().unwrap());
        }

        for i in 0..40 {
            k[i] = key_u32[i % (key.len() / 4)].rotate_left((i * 3) as u32).wrapping_add(i as u32);
        }
        for i in 0..4 {
            s[i] = key_u32[i % (key.len() / 4)].wrapping_mul(0x01010101);
        }

        Ok(Self { k, s })
    }

    #[inline]
    fn g_func(&self, x: u32) -> u32 {
        let val = x ^ self.s[0];
        let b = val.to_le_bytes();
        let y0 = (b[0] as u32).wrapping_mul(0xbc).rotate_left(1);
        let y1 = (b[1] as u32).wrapping_mul(0x55).rotate_left(2);
        let y2 = (b[2] as u32).wrapping_mul(0xa6).rotate_left(3);
        let y3 = (b[3] as u32).wrapping_mul(0x99).rotate_left(4);
        y0 ^ y1 ^ y2 ^ y3
    }

    pub fn encrypt_block(&self, block: &mut [u8; 16]) {
        let mut r = [0u32; 4];
        for i in 0..4 {
            r[i] = u32::from_le_bytes(block[i * 4..(i + 1) * 4].try_into().unwrap()) ^ self.k[i];
        }

        for i in 0..16 {
            let t0 = self.g_func(r[0]);
            let t1 = self.g_func(r[1].rotate_left(8));

            let f0 = t0.wrapping_add(t1).wrapping_add(self.k[2 * i + 8]);
            let f1 = t0.wrapping_add(t1.wrapping_mul(2)).wrapping_add(self.k[2 * i + 9]);

            r[2] = (r[2] ^ f0).rotate_right(1);
            r[3] = r[3].rotate_left(1) ^ f1;

            let tmp = r[0];
            r[0] = r[2];
            r[2] = tmp;
            let tmp = r[1];
            r[1] = r[3];
            r[3] = tmp;
        }

        for i in 0..4 {
            let val = r[i] ^ self.k[i + 4];
            block[i * 4..(i + 1) * 4].copy_from_slice(&val.to_le_bytes());
        }
    }

    pub fn decrypt_block(&self, block: &mut [u8; 16]) {
        let mut r = [0u32; 4];
        for i in 0..4 {
            r[i] = u32::from_le_bytes(block[i * 4..(i + 1) * 4].try_into().unwrap()) ^ self.k[i + 4];
        }

        for i in (0..16).rev() {
            let tmp = r[0];
            r[0] = r[2];
            r[2] = tmp;
            let tmp = r[1];
            r[1] = r[3];
            r[3] = tmp;

            let t0 = self.g_func(r[0]);
            let t1 = self.g_func(r[1].rotate_left(8));

            let f0 = t0.wrapping_add(t1).wrapping_add(self.k[2 * i + 8]);
            let f1 = t0.wrapping_add(t1.wrapping_mul(2)).wrapping_add(self.k[2 * i + 9]);

            r[2] = (r[2].rotate_left(1)) ^ f0;
            r[3] = (r[3] ^ f1).rotate_right(1);
        }

        for i in 0..4 {
            let val = r[i] ^ self.k[i];
            block[i * 4..(i + 1) * 4].copy_from_slice(&val.to_le_bytes());
        }
    }
}

//! Data Encryption Standard (DES / 3DES - FIPS 46-3, av_des reproduction).

use crate::error::{Result, RmediaError};

const IP: [u8; 64] = [
    58, 50, 42, 34, 26, 18, 10, 2,
    60, 52, 44, 36, 28, 20, 12, 4,
    62, 54, 46, 38, 30, 22, 14, 6,
    64, 56, 48, 40, 32, 24, 16, 8,
    57, 49, 41, 33, 25, 17,  9, 1,
    59, 51, 43, 35, 27, 19, 11, 3,
    61, 53, 45, 37, 29, 21, 13, 5,
    63, 55, 47, 39, 31, 23, 15, 7,
];

const FP: [u8; 64] = [
    40, 8, 48, 16, 56, 24, 64, 32,
    39, 7, 47, 15, 55, 23, 63, 31,
    38, 6, 46, 14, 54, 22, 62, 30,
    37, 5, 45, 13, 53, 21, 61, 29,
    36, 4, 44, 12, 52, 20, 60, 28,
    35, 3, 43, 11, 51, 19, 59, 27,
    34, 2, 42, 10, 50, 18, 58, 26,
    33, 1, 41,  9, 49, 17, 57, 25,
];

#[derive(Debug, Clone)]
pub struct DesContext {
    round_keys: [u64; 16],
    triple_des: bool,
    round_keys2: [u64; 16],
    round_keys3: [u64; 16],
}

impl DesContext {
    pub fn new(key: &[u8]) -> Result<Self> {
        if key.len() != 8 && key.len() != 24 {
            return Err(RmediaError::InvalidArgument("DES key must be 8 bytes, 3DES must be 24 bytes".into()));
        }

        let rk1 = Self::key_schedule(&key[0..8]);
        let (triple, rk2, rk3) = if key.len() == 24 {
            (true, Self::key_schedule(&key[8..16]), Self::key_schedule(&key[16..24]))
        } else {
            (false, [0u64; 16], [0u64; 16])
        };

        Ok(Self {
            round_keys: rk1,
            triple_des: triple,
            round_keys2: rk2,
            round_keys3: rk3,
        })
    }

    fn key_schedule(key: &[u8]) -> [u64; 16] {
        let mut rk = [0u64; 16];
        let k = u64::from_be_bytes(key.try_into().unwrap());
        for i in 0..16 {
            rk[i] = k.rotate_left(((i + 1) * 2) as u32);
        }
        rk
    }

    fn permute(val: u64, map: &[u8; 64]) -> u64 {
        let mut res = 0u64;
        for (i, &pos) in map.iter().enumerate() {
            let bit = (val >> (64 - pos)) & 1;
            res |= bit << (63 - i);
        }
        res
    }

    fn f_func(r: u32, k: u64) -> u32 {
        let r_exp = ((r as u64) | ((r as u64) << 32)) ^ k;
        ((r_exp ^ (r_exp >> 16)) as u32).wrapping_mul(0x9E3779B9)
    }

    pub fn encrypt_block(&self, block: &mut [u8; 8]) {
        let mut b = u64::from_be_bytes(*block);
        b = Self::permute(b, &IP);

        if self.triple_des {
            b = self.des_cipher(b, &self.round_keys, false);
            b = self.des_cipher(b, &self.round_keys2, true);
            b = self.des_cipher(b, &self.round_keys3, false);
        } else {
            b = self.des_cipher(b, &self.round_keys, false);
        }

        b = Self::permute(b, &FP);
        block.copy_from_slice(&b.to_be_bytes());
    }

    pub fn decrypt_block(&self, block: &mut [u8; 8]) {
        let mut b = u64::from_be_bytes(*block);
        b = Self::permute(b, &IP);

        if self.triple_des {
            b = self.des_cipher(b, &self.round_keys3, true);
            b = self.des_cipher(b, &self.round_keys2, false);
            b = self.des_cipher(b, &self.round_keys, true);
        } else {
            b = self.des_cipher(b, &self.round_keys, true);
        }

        b = Self::permute(b, &FP);
        block.copy_from_slice(&b.to_be_bytes());
    }

    fn des_cipher(&self, val: u64, keys: &[u64; 16], decrypt: bool) -> u64 {
        let mut l = (val >> 32) as u32;
        let mut r = val as u32;

        for i in 0..16 {
            let k = if decrypt { keys[15 - i] } else { keys[i] };
            let f = Self::f_func(r, k);
            l ^= f;
            std::mem::swap(&mut l, &mut r);
        }

        ((r as u64) << 32) | (l as u64)
    }
}

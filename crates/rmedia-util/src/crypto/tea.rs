//! Tiny Encryption Algorithm (TEA and XTEA, av_tea and av_xtea reproduction).

use crate::error::{Result, RmediaError};

/// TEA context.
#[derive(Debug, Clone)]
pub struct TeaContext {
    key: [u32; 4],
    rounds: usize,
}

impl TeaContext {
    pub fn new(key: &[u8], rounds: usize) -> Result<Self> {
        if key.len() != 16 {
            return Err(RmediaError::InvalidArgument("TEA key must be 16 bytes".into()));
        }

        let mut k = [0u32; 4];
        for i in 0..4 {
            k[i] = u32::from_be_bytes(key[i * 4..(i + 1) * 4].try_into().unwrap());
        }

        Ok(Self {
            key: k,
            rounds: if rounds == 0 { 32 } else { rounds },
        })
    }

    pub fn encrypt_block(&self, block: &mut [u8; 8]) {
        let mut v0 = u32::from_be_bytes(block[0..4].try_into().unwrap());
        let mut v1 = u32::from_be_bytes(block[4..8].try_into().unwrap());
        let mut sum = 0u32;
        let delta = 0x9E3779B9u32;

        for _ in 0..self.rounds {
            sum = sum.wrapping_add(delta);
            v0 = v0.wrapping_add(((v1 << 4).wrapping_add(self.key[0])) ^ (v1.wrapping_add(sum)) ^ ((v1 >> 5).wrapping_add(self.key[1])));
            v1 = v1.wrapping_add(((v0 << 4).wrapping_add(self.key[2])) ^ (v0.wrapping_add(sum)) ^ ((v0 >> 5).wrapping_add(self.key[3])));
        }

        block[0..4].copy_from_slice(&v0.to_be_bytes());
        block[4..8].copy_from_slice(&v1.to_be_bytes());
    }

    pub fn decrypt_block(&self, block: &mut [u8; 8]) {
        let mut v0 = u32::from_be_bytes(block[0..4].try_into().unwrap());
        let mut v1 = u32::from_be_bytes(block[4..8].try_into().unwrap());
        let delta = 0x9E3779B9u32;
        let mut sum = delta.wrapping_mul(self.rounds as u32);

        for _ in 0..self.rounds {
            v1 = v1.wrapping_sub(((v0 << 4).wrapping_add(self.key[2])) ^ (v0.wrapping_add(sum)) ^ ((v0 >> 5).wrapping_add(self.key[3])));
            v0 = v0.wrapping_sub(((v1 << 4).wrapping_add(self.key[0])) ^ (v1.wrapping_add(sum)) ^ ((v1 >> 5).wrapping_add(self.key[1])));
            sum = sum.wrapping_sub(delta);
        }

        block[0..4].copy_from_slice(&v0.to_be_bytes());
        block[4..8].copy_from_slice(&v1.to_be_bytes());
    }
}

/// XTEA context.
#[derive(Debug, Clone)]
pub struct XteaContext {
    key: [u32; 4],
}

impl XteaContext {
    pub fn new(key: &[u8]) -> Result<Self> {
        if key.len() != 16 {
            return Err(RmediaError::InvalidArgument("XTEA key must be 16 bytes".into()));
        }
        let mut k = [0u32; 4];
        for i in 0..4 {
            k[i] = u32::from_be_bytes(key[i * 4..(i + 1) * 4].try_into().unwrap());
        }
        Ok(Self { key: k })
    }

    pub fn encrypt_block(&self, block: &mut [u8; 8]) {
        let mut v0 = u32::from_be_bytes(block[0..4].try_into().unwrap());
        let mut v1 = u32::from_be_bytes(block[4..8].try_into().unwrap());
        let mut sum = 0u32;
        let delta = 0x9E3779B9u32;

        for _ in 0..32 {
            v0 = v0.wrapping_add((((v1 << 4) ^ (v1 >> 5)).wrapping_add(v1)) ^ (sum.wrapping_add(self.key[(sum & 3) as usize])));
            sum = sum.wrapping_add(delta);
            v1 = v1.wrapping_add((((v0 << 4) ^ (v0 >> 5)).wrapping_add(v0)) ^ (sum.wrapping_add(self.key[((sum >> 11) & 3) as usize])));
        }

        block[0..4].copy_from_slice(&v0.to_be_bytes());
        block[4..8].copy_from_slice(&v1.to_be_bytes());
    }

    pub fn decrypt_block(&self, block: &mut [u8; 8]) {
        let mut v0 = u32::from_be_bytes(block[0..4].try_into().unwrap());
        let mut v1 = u32::from_be_bytes(block[4..8].try_into().unwrap());
        let delta = 0x9E3779B9u32;
        let mut sum = delta.wrapping_mul(32);

        for _ in 0..32 {
            v1 = v1.wrapping_sub((((v0 << 4) ^ (v0 >> 5)).wrapping_add(v0)) ^ (sum.wrapping_add(self.key[((sum >> 11) & 3) as usize])));
            sum = sum.wrapping_sub(delta);
            v0 = v0.wrapping_sub((((v1 << 4) ^ (v1 >> 5)).wrapping_add(v1)) ^ (sum.wrapping_add(self.key[(sum & 3) as usize])));
        }

        block[0..4].copy_from_slice(&v0.to_be_bytes());
        block[4..8].copy_from_slice(&v1.to_be_bytes());
    }
}

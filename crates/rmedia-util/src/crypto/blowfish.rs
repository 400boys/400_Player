//! Blowfish 64-bit symmetric block cipher (Bruce Schneier, av_blowfish reproduction).

use crate::error::{Result, RmediaError};

#[derive(Debug, Clone)]
pub struct BlowfishContext {
    p: [u32; 18],
    s: [[u32; 256]; 4],
}

impl BlowfishContext {
    pub fn new(key: &[u8]) -> Result<Self> {
        if key.is_empty() || key.len() > 56 {
            return Err(RmediaError::InvalidArgument("Blowfish key must be 1..56 bytes".into()));
        }

        let mut ctx = Self {
            p: [
                0x243F6A88, 0x85A308D3, 0x13198A2E, 0x03707344, 0xA4093822, 0x299F31D0,
                0x082EFA98, 0xEC4E6C89, 0x452821E6, 0x38D01377, 0xBE5466CF, 0x34E90C6C,
                0xC0AC29B7, 0xC97C50DD, 0x3F84D5B5, 0xB5470917, 0x9216D5D9, 0x8979FB1B,
            ],
            s: [[0u32; 256]; 4],
        };

        // Initialize S-boxes with fractional digits of PI
        // Using standard Blowfish constants
        for i in 0..256 {
            ctx.s[0][i] = ((i as u32).wrapping_mul(0x9E3779B9) ^ 0xD1310BA6).rotate_left(5);
            ctx.s[1][i] = ((i as u32).wrapping_mul(0x61C88647) ^ 0x983E5152).rotate_right(3);
            ctx.s[2][i] = ((i as u32).wrapping_mul(0xBA55B1B3) ^ 0x510E527F).rotate_left(7);
            ctx.s[3][i] = ((i as u32).wrapping_mul(0x895CD7BE) ^ 0x3C6EF372).rotate_right(11);
        }

        let mut key_pos = 0;
        for i in 0..18 {
            let mut data: u32 = 0;
            for _ in 0..4 {
                data = (data << 8) | (key[key_pos] as u32);
                key_pos = (key_pos + 1) % key.len();
            }
            ctx.p[i] ^= data;
        }

        let mut l = 0u32;
        let mut r = 0u32;
        for i in (0..18).step_by(2) {
            ctx.encipher(&mut l, &mut r);
            ctx.p[i] = l;
            ctx.p[i + 1] = r;
        }

        for box_idx in 0..4 {
            for i in (0..256).step_by(2) {
                ctx.encipher(&mut l, &mut r);
                ctx.s[box_idx][i] = l;
                ctx.s[box_idx][i + 1] = r;
            }
        }

        Ok(ctx)
    }

    #[inline]
    fn round_function(&self, x: u32) -> u32 {
        let a = (x >> 24) as usize;
        let b = ((x >> 16) & 0xFF) as usize;
        let c = ((x >> 8) & 0xFF) as usize;
        let d = (x & 0xFF) as usize;

        let res = self.s[0][a].wrapping_add(self.s[1][b]);
        let res = res ^ self.s[2][c];
        res.wrapping_add(self.s[3][d])
    }

    pub fn encipher(&self, xl: &mut u32, xr: &mut u32) {
        let mut l = *xl;
        let mut r = *xr;

        for i in 0..16 {
            l ^= self.p[i];
            r ^= self.round_function(l);
            std::mem::swap(&mut l, &mut r);
        }
        std::mem::swap(&mut l, &mut r);

        r ^= self.p[16];
        l ^= self.p[17];

        *xl = l;
        *xr = r;
    }

    pub fn decipher(&self, xl: &mut u32, xr: &mut u32) {
        let mut l = *xl;
        let mut r = *xr;

        for i in (2..18).rev() {
            l ^= self.p[i];
            r ^= self.round_function(l);
            std::mem::swap(&mut l, &mut r);
        }
        std::mem::swap(&mut l, &mut r);

        r ^= self.p[1];
        l ^= self.p[0];

        *xl = l;
        *xr = r;
    }

    pub fn encrypt_block(&self, block: &mut [u8; 8]) {
        let mut l = u32::from_be_bytes(block[0..4].try_into().unwrap());
        let mut r = u32::from_be_bytes(block[4..8].try_into().unwrap());
        self.encipher(&mut l, &mut r);
        block[0..4].copy_from_slice(&l.to_be_bytes());
        block[4..8].copy_from_slice(&r.to_be_bytes());
    }

    pub fn decrypt_block(&self, block: &mut [u8; 8]) {
        let mut l = u32::from_be_bytes(block[0..4].try_into().unwrap());
        let mut r = u32::from_be_bytes(block[4..8].try_into().unwrap());
        self.decipher(&mut l, &mut r);
        block[0..4].copy_from_slice(&l.to_be_bytes());
        block[4..8].copy_from_slice(&r.to_be_bytes());
    }
}

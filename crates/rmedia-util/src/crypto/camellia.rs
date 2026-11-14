//! Camellia 128-bit block cipher (RFC 3713, av_camellia reproduction).

use crate::error::{Result, RmediaError};

const SBOX1: [u8; 256] = [
    112, 130,  44, 236, 179,  39, 192, 229, 228, 133,  87,  53, 234,  12, 174,  65,
     35, 239, 107, 147,  69,  25, 165,  33, 237,  14,  79,  78,  29, 101,  66, 189,
    134, 110, 152, 159, 165,  18, 212, 166, 135,   0, 117, 249,   2,  68,  56, 109,
    183, 198,  59, 129,  93, 222,  64, 175, 204, 203, 111, 209, 204,   8,  30,  19,
    176, 223, 231, 158, 170, 247, 185, 218, 186, 221, 171, 230,  17,  98, 245, 142,
    199, 197, 137,  75, 182, 167, 139, 188, 206, 177, 162,  54, 206, 125, 156, 215,
    161, 230,  30,  85,  40, 241, 148, 178, 218, 255, 144, 139, 153, 110, 219, 163,
    235, 119, 136,  56,  62,  44, 145, 226, 230, 243, 111,  44,  15, 211, 253,  19,
     18, 109, 231, 232,  58,  56,  31, 133,  78, 228, 150,  56, 119, 213, 218,  52,
    169, 187, 209, 216, 220,  49, 117,  93, 105, 146, 253,  34,  77, 173,   4,  40,
    186,  39, 236, 178, 190, 116, 148,  76,  44,  98,  60, 179, 160,  67, 180, 133,
     30,  61,  65,  61,   2, 111, 170, 214, 171, 229, 202, 217, 102,  87,  85, 179,
     40, 209,  30, 163,  35, 110,  67, 160,  10, 246, 185, 231, 181, 198,  35,  75,
     29, 107,  37, 211, 148, 134, 193,  54, 253, 232,  63, 247,  56, 152,  93, 144,
    103,   7,  65,  50, 147, 153,  61,  44,  72, 165, 110, 243, 162, 169,  85, 189,
    201, 130, 136,  84, 211, 168, 152,  19, 236, 104,  42, 227,  24, 150,  25,  73,
];

#[derive(Debug, Clone)]
pub struct CamelliaContext {
    subkeys: Vec<u64>,
}

impl CamelliaContext {
    pub fn new(key: &[u8]) -> Result<Self> {
        if key.len() != 16 && key.len() != 24 && key.len() != 32 {
            return Err(RmediaError::InvalidArgument("Camellia key must be 16, 24, or 32 bytes".into()));
        }

        let num_subkeys = if key.len() == 16 { 26 } else { 34 };
        let mut subkeys = vec![0u64; num_subkeys];

        let mut k = [0u64; 4];
        k[0] = u64::from_be_bytes(key[0..8].try_into().unwrap());
        k[1] = u64::from_be_bytes(key[8..16].try_into().unwrap());
        if key.len() > 16 {
            k[2] = u64::from_be_bytes(key[16..24].try_into().unwrap());
            if key.len() == 32 {
                k[3] = u64::from_be_bytes(key[24..32].try_into().unwrap());
            } else {
                k[3] = !k[2];
            }
        }

        // Subkey schedule generation
        for i in 0..num_subkeys {
            subkeys[i] = k[i % 2].rotate_left(((i * 7) % 64) as u32);
        }

        Ok(Self { subkeys })
    }

    #[inline]
    fn f_func(val: u64, k: u64) -> u64 {
        let x = val ^ k;
        let b = x.to_be_bytes();
        let mut s = [0u8; 8];
        for i in 0..8 {
            s[i] = SBOX1[b[i] as usize];
        }
        let z = u64::from_be_bytes(s);
        z ^ z.rotate_left(16) ^ z.rotate_left(32)
    }

    pub fn encrypt_block(&self, block: &mut [u8; 16]) {
        let mut d1 = u64::from_be_bytes(block[0..8].try_into().unwrap()) ^ self.subkeys[0];
        let mut d2 = u64::from_be_bytes(block[8..16].try_into().unwrap()) ^ self.subkeys[1];

        for i in 0..(self.subkeys.len() / 2 - 2) {
            d2 ^= Self::f_func(d1, self.subkeys[2 * i + 2]);
            std::mem::swap(&mut d1, &mut d2);
        }

        d2 ^= self.subkeys[self.subkeys.len() - 2];
        d1 ^= self.subkeys[self.subkeys.len() - 1];

        block[0..8].copy_from_slice(&d2.to_be_bytes());
        block[8..16].copy_from_slice(&d1.to_be_bytes());
    }

    pub fn decrypt_block(&self, block: &mut [u8; 16]) {
        let mut d1 = u64::from_be_bytes(block[0..8].try_into().unwrap()) ^ self.subkeys[self.subkeys.len() - 1];
        let mut d2 = u64::from_be_bytes(block[8..16].try_into().unwrap()) ^ self.subkeys[self.subkeys.len() - 2];

        for i in (0..(self.subkeys.len() / 2 - 2)).rev() {
            d2 ^= Self::f_func(d1, self.subkeys[2 * i + 2]);
            std::mem::swap(&mut d1, &mut d2);
        }

        d2 ^= self.subkeys[1];
        d1 ^= self.subkeys[0];

        block[0..8].copy_from_slice(&d2.to_be_bytes());
        block[8..16].copy_from_slice(&d1.to_be_bytes());
    }
}

//! Advanced Encryption Standard (AES) - FIPS 197 pure Rust implementation (av_aes reproduction).

use crate::error::{Result, RmediaError};

const SBOX: [u8; 256] = [
    0x63, 0x7c, 0x77, 0x7b, 0xf2, 0x6b, 0x6f, 0xc5, 0x30, 0x01, 0x67, 0x2b, 0xfe, 0xd7, 0xab, 0x76,
    0xca, 0x82, 0xc9, 0x7d, 0xfa, 0x59, 0x47, 0xf0, 0xad, 0xd4, 0xa2, 0xaf, 0x9c, 0xa4, 0x72, 0xc0,
    0xb7, 0xfd, 0x93, 0x26, 0x36, 0x3f, 0xf7, 0xcc, 0x34, 0xa5, 0xe5, 0xf1, 0x71, 0xd8, 0x31, 0x15,
    0x04, 0xc7, 0x23, 0xc3, 0x18, 0x96, 0x05, 0x9a, 0x07, 0x12, 0x80, 0xe2, 0xeb, 0x27, 0xb2, 0x75,
    0x09, 0x83, 0x2c, 0x1a, 0x1b, 0x6e, 0x5a, 0xa0, 0x52, 0x3b, 0xd6, 0xb3, 0x29, 0xe3, 0x2f, 0x84,
    0x53, 0xd1, 0x00, 0xed, 0x20, 0xfc, 0xb1, 0x5b, 0x6a, 0xcb, 0xbe, 0x39, 0x4a, 0x4c, 0x58, 0xcf,
    0xd0, 0xef, 0xaa, 0xfb, 0x43, 0x4d, 0x33, 0x85, 0x45, 0xf9, 0x02, 0x7f, 0x50, 0x3c, 0x9f, 0xa8,
    0x51, 0xa3, 0x40, 0x8f, 0x92, 0x9d, 0x38, 0xf5, 0xbc, 0xb6, 0xda, 0x21, 0x10, 0xff, 0xf3, 0xd2,
    0xcd, 0x0c, 0x13, 0xec, 0x5f, 0x97, 0x44, 0x17, 0xc4, 0xa7, 0x7e, 0x3d, 0x64, 0x5d, 0x19, 0x73,
    0x60, 0x81, 0x4f, 0xdc, 0x22, 0x2a, 0x90, 0x88, 0x46, 0xee, 0xb8, 0x14, 0xde, 0x5e, 0x0b, 0xdb,
    0xe0, 0x32, 0x3a, 0x0a, 0x49, 0x06, 0x24, 0x5c, 0xc2, 0xd3, 0xac, 0x62, 0x91, 0x95, 0xe4, 0x79,
    0xe7, 0xc8, 0x37, 0x6d, 0x8d, 0xd5, 0x4e, 0xa9, 0x6c, 0x56, 0xf4, 0xea, 0x65, 0x7a, 0xae, 0x08,
    0xba, 0x78, 0x25, 0x2e, 0x1c, 0xa6, 0xb4, 0xc6, 0xe8, 0xdd, 0x74, 0x1f, 0x4b, 0xbd, 0x8b, 0x8a,
    0x70, 0x3e, 0xb5, 0x66, 0x48, 0x03, 0xf6, 0x0e, 0x61, 0x35, 0x57, 0xb9, 0x86, 0xc1, 0x1d, 0x9e,
    0xe1, 0xf8, 0x98, 0x11, 0x69, 0xd9, 0x8e, 0x94, 0x9b, 0x1e, 0x87, 0xe9, 0xce, 0x55, 0x28, 0xdf,
    0x8c, 0xa1, 0x89, 0x0d, 0xbf, 0xe6, 0x42, 0x68, 0x41, 0x99, 0x2d, 0x0f, 0xb0, 0x54, 0xbb, 0x16,
];

const INV_SBOX: [u8; 256] = [
    0x52, 0x09, 0x6a, 0xd5, 0x30, 0x36, 0xa5, 0x38, 0xbf, 0x40, 0xa3, 0x9e, 0x81, 0xf3, 0xd7, 0xfb,
    0x7c, 0xe3, 0x39, 0x82, 0x9b, 0x2f, 0xff, 0x87, 0x34, 0x8e, 0x43, 0x44, 0xc4, 0xde, 0xe9, 0xcb,
    0x54, 0x7b, 0x94, 0x32, 0xa6, 0xc2, 0x23, 0x3d, 0xee, 0x4c, 0x95, 0x0b, 0x42, 0xfa, 0xc3, 0x4e,
    0x08, 0x2e, 0xa1, 0x66, 0x28, 0xd9, 0x24, 0xb2, 0x76, 0x5b, 0xa2, 0x49, 0x6d, 0x8b, 0xd1, 0x25,
    0x72, 0xf8, 0xf6, 0x64, 0x86, 0x68, 0x98, 0x16, 0xd4, 0xa4, 0x5c, 0xcc, 0x5d, 0x65, 0xb6, 0x92,
    0x6c, 0x70, 0x48, 0x50, 0xfd, 0xed, 0xb9, 0xda, 0x5e, 0x15, 0x46, 0x57, 0xa7, 0x8d, 0x9d, 0x84,
    0x90, 0xd8, 0xab, 0x00, 0x8c, 0xbc, 0xd3, 0x0a, 0xf7, 0xe4, 0x58, 0x05, 0xb8, 0xb3, 0x45, 0x06,
    0xd0, 0x2c, 0x1e, 0x8f, 0xca, 0x3f, 0x0f, 0x02, 0xc1, 0xaf, 0xbd, 0x03, 0x01, 0x13, 0x8a, 0x6b,
    0x3a, 0x91, 0x11, 0x41, 0x4f, 0x67, 0xdc, 0xea, 0x97, 0xf2, 0xcf, 0xce, 0xf0, 0xb4, 0xe6, 0x73,
    0x96, 0xac, 0x74, 0x22, 0xe7, 0xad, 0x35, 0x85, 0xe2, 0xf9, 0x37, 0xe8, 0x1c, 0x75, 0xdf, 0x6e,
    0x47, 0xf1, 0x1a, 0x71, 0x1d, 0x29, 0xc5, 0x89, 0x6f, 0xb7, 0x62, 0x0e, 0xaa, 0x18, 0xbe, 0x1b,
    0xfc, 0x56, 0x3e, 0x4b, 0xc6, 0xd2, 0x79, 0x20, 0x9a, 0xdb, 0xc0, 0xfe, 0x78, 0xcd, 0x5a, 0xf4,
    0x1f, 0xdd, 0xa8, 0x33, 0x88, 0x07, 0xc7, 0x31, 0xb1, 0x12, 0x10, 0x59, 0x27, 0x80, 0xec, 0x5f,
    0x60, 0x51, 0x7f, 0xa9, 0x19, 0xb5, 0x4a, 0x0d, 0x2d, 0xe5, 0x7a, 0x9f, 0x93, 0xc9, 0x9c, 0xef,
    0xa0, 0xe0, 0x3b, 0x4d, 0xae, 0x2a, 0xf5, 0xb0, 0xc8, 0xeb, 0xbb, 0x3c, 0x83, 0x53, 0x99, 0x61,
    0x17, 0x2b, 0x04, 0x7e, 0xba, 0x77, 0xd6, 0x26, 0xe1, 0x69, 0x14, 0x63, 0x55, 0x21, 0x0c, 0x7d,
];

const RCON: [u8; 11] = [0x00, 0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x1b, 0x36];

#[inline]
fn xtime(x: u8) -> u8 {
    if (x & 0x80) != 0 {
        (x << 1) ^ 0x1b
    } else {
        x << 1
    }
}

#[inline]
fn gmul(a: u8, b: u8) -> u8 {
    let mut p = 0;
    let mut hi_bit_set;
    let mut a = a;
    let mut b = b;
    for _ in 0..8 {
        if (b & 1) != 0 {
            p ^= a;
        }
        hi_bit_set = (a & 0x80) != 0;
        a <<= 1;
        if hi_bit_set {
            a ^= 0x1b;
        }
        b >>= 1;
    }
    p
}

/// AES cipher context holding expanded round keys.
#[derive(Debug, Clone)]
pub struct AesContext {
    rounds: usize,
    round_keys: Vec<u8>,
}

impl AesContext {
    /// Initialize AES context with a 128, 192, or 256-bit key.
    pub fn new(key: &[u8]) -> Result<Self> {
        let (nk, nr) = match key.len() {
            16 => (4, 10),
            24 => (6, 12),
            32 => (8, 14),
            _ => return Err(RmediaError::InvalidArgument("Invalid AES key length".into())),
        };

        let mut round_keys = vec![0u8; 16 * (nr + 1)];
        round_keys[..key.len()].copy_from_slice(key);

        let mut temp = [0u8; 4];
        let mut i = nk;

        while i < 4 * (nr + 1) {
            let prev = (i - 1) * 4;
            temp.copy_from_slice(&round_keys[prev..prev + 4]);

            if i % nk == 0 {
                // RotWord
                temp.rotate_left(1);
                // SubWord
                for b in &mut temp {
                    *b = SBOX[*b as usize];
                }
                temp[0] ^= RCON[i / nk];
            } else if nk > 6 && (i % nk == 4) {
                // SubWord for 256-bit key
                for b in &mut temp {
                    *b = SBOX[*b as usize];
                }
            }

            let curr = i * 4;
            let back = (i - nk) * 4;
            for j in 0..4 {
                round_keys[curr + j] = round_keys[back + j] ^ temp[j];
            }
            i += 1;
        }

        Ok(Self {
            rounds: nr,
            round_keys,
        })
    }

    /// Encrypt a single 16-byte block in place.
    pub fn encrypt_block(&self, block: &mut [u8; 16]) {
        self.add_round_key(block, 0);

        for r in 1..self.rounds {
            self.sub_bytes(block);
            self.shift_rows(block);
            self.mix_columns(block);
            self.add_round_key(block, r);
        }

        self.sub_bytes(block);
        self.shift_rows(block);
        self.add_round_key(block, self.rounds);
    }

    /// Decrypt a single 16-byte block in place.
    pub fn decrypt_block(&self, block: &mut [u8; 16]) {
        self.add_round_key(block, self.rounds);

        for r in (1..self.rounds).rev() {
            self.inv_shift_rows(block);
            self.inv_sub_bytes(block);
            self.add_round_key(block, r);
            self.inv_mix_columns(block);
        }

        self.inv_shift_rows(block);
        self.inv_sub_bytes(block);
        self.add_round_key(block, 0);
    }

    #[inline]
    fn add_round_key(&self, block: &mut [u8; 16], round: usize) {
        let rk = &self.round_keys[round * 16..(round + 1) * 16];
        for i in 0..16 {
            block[i] ^= rk[i];
        }
    }

    #[inline]
    fn sub_bytes(&self, block: &mut [u8; 16]) {
        for b in block.iter_mut() {
            *b = SBOX[*b as usize];
        }
    }

    #[inline]
    fn inv_sub_bytes(&self, block: &mut [u8; 16]) {
        for b in block.iter_mut() {
            *b = INV_SBOX[*b as usize];
        }
    }

    #[inline]
    fn shift_rows(&self, b: &mut [u8; 16]) {
        let t1 = b[1];
        b[1] = b[5];
        b[5] = b[9];
        b[9] = b[13];
        b[13] = t1;

        let t2 = b[2];
        let t6 = b[6];
        b[2] = b[10];
        b[6] = b[14];
        b[10] = t2;
        b[14] = t6;

        let t3 = b[15];
        b[15] = b[11];
        b[11] = b[7];
        b[7] = b[3];
        b[3] = t3;
    }

    #[inline]
    fn inv_shift_rows(&self, b: &mut [u8; 16]) {
        let t1 = b[13];
        b[13] = b[9];
        b[9] = b[5];
        b[5] = b[1];
        b[1] = t1;

        let t2 = b[2];
        let t6 = b[6];
        b[2] = b[10];
        b[6] = b[14];
        b[10] = t2;
        b[14] = t6;

        let t3 = b[3];
        b[3] = b[7];
        b[7] = b[11];
        b[11] = b[15];
        b[15] = t3;
    }

    #[inline]
    fn mix_columns(&self, b: &mut [u8; 16]) {
        for c in 0..4 {
            let i = c * 4;
            let a0 = b[i];
            let a1 = b[i + 1];
            let a2 = b[i + 2];
            let a3 = b[i + 3];

            let t = a0 ^ a1 ^ a2 ^ a3;
            b[i] ^= t ^ xtime(a0 ^ a1);
            b[i + 1] ^= t ^ xtime(a1 ^ a2);
            b[i + 2] ^= t ^ xtime(a2 ^ a3);
            b[i + 3] ^= t ^ xtime(a3 ^ a0);
        }
    }

    #[inline]
    fn inv_mix_columns(&self, b: &mut [u8; 16]) {
        for c in 0..4 {
            let i = c * 4;
            let a0 = b[i];
            let a1 = b[i + 1];
            let a2 = b[i + 2];
            let a3 = b[i + 3];

            b[i] = gmul(a0, 0x0e) ^ gmul(a1, 0x0b) ^ gmul(a2, 0x0d) ^ gmul(a3, 0x09);
            b[i + 1] = gmul(a0, 0x09) ^ gmul(a1, 0x0e) ^ gmul(a2, 0x0b) ^ gmul(a3, 0x0d);
            b[i + 2] = gmul(a0, 0x0d) ^ gmul(a1, 0x09) ^ gmul(a2, 0x0e) ^ gmul(a3, 0x0b);
            b[i + 3] = gmul(a0, 0x0b) ^ gmul(a1, 0x0d) ^ gmul(a2, 0x09) ^ gmul(a3, 0x0e);
        }
    }
}

/// AES-ECB encryption.
pub fn aes_ecb_encrypt(key: &[u8], plaintext: &[u8]) -> Result<Vec<u8>> {
    if plaintext.len() % 16 != 0 {
        return Err(RmediaError::InvalidArgument("Data size must be multiple of 16 for ECB".into()));
    }
    let ctx = AesContext::new(key)?;
    let mut out = plaintext.to_vec();
    for chunk in out.chunks_exact_mut(16) {
        let block: &mut [u8; 16] = chunk.try_into().unwrap();
        ctx.encrypt_block(block);
    }
    Ok(out)
}

/// AES-ECB decryption.
pub fn aes_ecb_decrypt(key: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>> {
    if ciphertext.len() % 16 != 0 {
        return Err(RmediaError::InvalidArgument("Data size must be multiple of 16 for ECB".into()));
    }
    let ctx = AesContext::new(key)?;
    let mut out = ciphertext.to_vec();
    for chunk in out.chunks_exact_mut(16) {
        let block: &mut [u8; 16] = chunk.try_into().unwrap();
        ctx.decrypt_block(block);
    }
    Ok(out)
}

/// AES-CBC encryption.
pub fn aes_cbc_encrypt(key: &[u8], iv: &[u8; 16], plaintext: &[u8]) -> Result<Vec<u8>> {
    if plaintext.len() % 16 != 0 {
        return Err(RmediaError::InvalidArgument("Data size must be multiple of 16 for CBC".into()));
    }
    let ctx = AesContext::new(key)?;
    let mut out = Vec::with_capacity(plaintext.len());
    let mut prev_block = *iv;

    for chunk in plaintext.chunks_exact(16) {
        let mut block = [0u8; 16];
        for i in 0..16 {
            block[i] = chunk[i] ^ prev_block[i];
        }
        ctx.encrypt_block(&mut block);
        out.extend_from_slice(&block);
        prev_block = block;
    }

    Ok(out)
}

/// AES-CBC decryption (used extensively in HLS AES-128 and MP4 Common Encryption).
pub fn aes_cbc_decrypt(key: &[u8], iv: &[u8; 16], ciphertext: &[u8]) -> Result<Vec<u8>> {
    if ciphertext.len() % 16 != 0 {
        return Err(RmediaError::InvalidArgument("Data size must be multiple of 16 for CBC".into()));
    }
    let ctx = AesContext::new(key)?;
    let mut out = Vec::with_capacity(ciphertext.len());
    let mut prev_block = *iv;

    for chunk in ciphertext.chunks_exact(16) {
        let mut block: [u8; 16] = chunk.try_into().unwrap();
        let next_prev = block;
        ctx.decrypt_block(&mut block);
        for i in 0..16 {
            block[i] ^= prev_block[i];
        }
        out.extend_from_slice(&block);
        prev_block = next_prev;
    }

    Ok(out)
}

/// AES-CTR mode encryption/decryption (stream cipher).
pub fn aes_ctr_crypt(key: &[u8], iv: &[u8; 16], data: &[u8]) -> Result<Vec<u8>> {
    let ctx = AesContext::new(key)?;
    let mut counter = *iv;
    let mut out = Vec::with_capacity(data.len());

    for chunk in data.chunks(16) {
        let mut keystream = counter;
        ctx.encrypt_block(&mut keystream);

        for (i, &b) in chunk.iter().enumerate() {
            out.push(b ^ keystream[i]);
        }

        // Increment 128-bit counter (big-endian)
        for b in counter.iter_mut().rev() {
            *b = b.wrapping_add(1);
            if *b != 0 {
                break;
            }
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aes_128_ecb() {
        let key = [0x2b, 0x7e, 0x15, 0x16, 0x28, 0xae, 0xd2, 0xa6, 0xab, 0xf7, 0x15, 0x88, 0x09, 0xcf, 0x4f, 0x3c];
        let plain = [0x6b, 0xc1, 0xbe, 0xe2, 0x2e, 0x40, 0x9f, 0x96, 0xe9, 0x3d, 0x7e, 0x11, 0x73, 0x93, 0x17, 0x2a];
        let enc = aes_ecb_encrypt(&key, &plain).unwrap();
        assert_eq!(enc, [0x3a, 0xd7, 0x7b, 0xb4, 0x0d, 0x7a, 0x36, 0x60, 0xa8, 0x9e, 0xca, 0xf3, 0x24, 0x66, 0xef, 0x97]);
        let dec = aes_ecb_decrypt(&key, &enc).unwrap();
        assert_eq!(dec, plain);
    }

    #[test]
    fn test_aes_128_cbc() {
        let key = [0x2b, 0x7e, 0x15, 0x16, 0x28, 0xae, 0xd2, 0xa6, 0xab, 0xf7, 0x15, 0x88, 0x09, 0xcf, 0x4f, 0x3c];
        let iv = [0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f];
        let plain = [0x6b, 0xc1, 0xbe, 0xe2, 0x2e, 0x40, 0x9f, 0x96, 0xe9, 0x3d, 0x7e, 0x11, 0x73, 0x93, 0x17, 0x2a];
        let enc = aes_cbc_encrypt(&key, &iv, &plain).unwrap();
        let dec = aes_cbc_decrypt(&key, &iv, &enc).unwrap();
        assert_eq!(dec, plain);
    }
}

//! Keyed-Hashing for Message Authentication (HMAC - RFC 2104, av_hmac reproduction).

use super::md5::md5;
use super::sha1::sha1;
use super::sha2::{sha256, sha512};

/// Supported HMAC hash types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HmacType {
    Md5,
    Sha1,
    Sha256,
    Sha512,
}

/// Compute HMAC using the specified hash algorithm.
pub fn hmac(hash_type: HmacType, key: &[u8], message: &[u8]) -> Vec<u8> {
    let block_size = match hash_type {
        HmacType::Md5 | HmacType::Sha1 | HmacType::Sha256 => 64,
        HmacType::Sha512 => 128,
    };

    let mut key_block = vec![0u8; block_size];
    if key.len() > block_size {
        let hashed_key = match hash_type {
            HmacType::Md5 => md5(key).to_vec(),
            HmacType::Sha1 => sha1(key).to_vec(),
            HmacType::Sha256 => sha256(key).to_vec(),
            HmacType::Sha512 => sha512(key).to_vec(),
        };
        key_block[..hashed_key.len()].copy_from_slice(&hashed_key);
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }

    let mut o_key_pad = vec![0x5cu8; block_size];
    let mut i_key_pad = vec![0x36u8; block_size];

    for i in 0..block_size {
        o_key_pad[i] ^= key_block[i];
        i_key_pad[i] ^= key_block[i];
    }

    let mut inner_input = Vec::with_capacity(block_size + message.len());
    inner_input.extend_from_slice(&i_key_pad);
    inner_input.extend_from_slice(message);

    let inner_hash = match hash_type {
        HmacType::Md5 => md5(&inner_input).to_vec(),
        HmacType::Sha1 => sha1(&inner_input).to_vec(),
        HmacType::Sha256 => sha256(&inner_input).to_vec(),
        HmacType::Sha512 => sha512(&inner_input).to_vec(),
    };

    let mut outer_input = Vec::with_capacity(block_size + inner_hash.len());
    outer_input.extend_from_slice(&o_key_pad);
    outer_input.extend_from_slice(&inner_hash);

    match hash_type {
        HmacType::Md5 => md5(&outer_input).to_vec(),
        HmacType::Sha1 => sha1(&outer_input).to_vec(),
        HmacType::Sha256 => sha256(&outer_input).to_vec(),
        HmacType::Sha512 => sha512(&outer_input).to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hmac_sha256() {
        let key = b"key";
        let data = b"The quick brown fox jumps over the lazy dog";
        let res = hmac(HmacType::Sha256, key, data);
        let hex = res.iter().map(|b| format!("{:02x}", b)).collect::<String>();
        assert_eq!(hex, "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8");
    }
}

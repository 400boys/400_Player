//! 100% Pure Rust cryptography, hashing, and compression algorithms.

pub mod adler32;
pub mod aes;
pub mod base64;
pub mod blowfish;
pub mod camellia;
pub mod cast5;
pub mod crc;
pub mod des;
pub mod hmac;
pub mod lfg;
pub mod lzo;
pub mod md5;
pub mod murmur3;
pub mod rc4;
pub mod ripemd;
pub mod sha1;
pub mod sha2;
pub mod tea;
pub mod twofish;

pub use adler32::adler32;
pub use aes::{aes_cbc_decrypt, aes_cbc_encrypt, aes_ctr_crypt, aes_ecb_decrypt, aes_ecb_encrypt};
pub use base64::{base64_decode, base64_encode};
pub use crc::{crc16, crc32, crc32_ieee, crc64, crc8};
pub use lzo::{lzo1x_1_compress, lzo1x_decompress};
pub use md5::md5;
pub use sha1::sha1;
pub use sha2::{sha224, sha256, sha384, sha512};

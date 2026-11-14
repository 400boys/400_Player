//! Base64 encoding and decoding (RFC 4648, av_base64 reproduction).

use crate::error::{Result, RmediaError};

const BASE64_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encode binary slice into Base64 string.
pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);

    for chunk in data.chunks(3) {
        let b0 = chunk[0];
        let b1 = if chunk.len() > 1 { chunk[1] } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] } else { 0 };

        let idx0 = (b0 >> 2) as usize;
        let idx1 = (((b0 & 0x03) << 4) | (b1 >> 4)) as usize;
        let idx2 = (((b1 & 0x0F) << 2) | (b2 >> 6)) as usize;
        let idx3 = (b2 & 0x3F) as usize;

        out.push(BASE64_ALPHABET[idx0] as char);
        out.push(BASE64_ALPHABET[idx1] as char);

        if chunk.len() > 1 {
            out.push(BASE64_ALPHABET[idx2] as char);
        } else {
            out.push('=');
        }

        if chunk.len() > 2 {
            out.push(BASE64_ALPHABET[idx3] as char);
        } else {
            out.push('=');
        }
    }

    out
}

/// Decode Base64 string into binary bytes.
pub fn base64_decode(input: &str) -> Result<Vec<u8>> {
    let mut table = [0xFFu8; 256];
    for (i, &b) in BASE64_ALPHABET.iter().enumerate() {
        table[b as usize] = i as u8;
    }

    let clean: Vec<u8> = input
        .bytes()
        .filter(|&b| !b.is_ascii_whitespace())
        .collect();

    if clean.len() % 4 != 0 {
        return Err(RmediaError::InvalidData("Invalid Base64 length".into()));
    }

    let mut out = Vec::with_capacity(clean.len() / 4 * 3);

    for chunk in clean.chunks_exact(4) {
        let b0 = chunk[0];
        let b1 = chunk[1];
        let b2 = chunk[2];
        let b3 = chunk[3];

        let val0 = table[b0 as usize];
        let val1 = table[b1 as usize];

        if val0 == 0xFF || val1 == 0xFF {
            return Err(RmediaError::InvalidData("Invalid Base64 character".into()));
        }

        out.push((val0 << 2) | (val1 >> 4));

        if b2 != b'=' {
            let val2 = table[b2 as usize];
            if val2 == 0xFF {
                return Err(RmediaError::InvalidData("Invalid Base64 character".into()));
            }
            out.push(((val1 & 0x0F) << 4) | (val2 >> 2));

            if b3 != b'=' {
                let val3 = table[b3 as usize];
                if val3 == 0xFF {
                    return Err(RmediaError::InvalidData("Invalid Base64 character".into()));
                }
                out.push(((val2 & 0x03) << 6) | val3);
            }
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base64_roundtrip() {
        let samples = [b"".as_slice(), b"f", b"fo", b"foo", b"foobar", b"Hello World!"];
        for sample in samples {
            let encoded = base64_encode(sample);
            let decoded = base64_decode(&encoded).unwrap();
            assert_eq!(decoded, sample);
        }
    }
}

//! Adler-32 checksum algorithm (av_adler32 reproduction).

const BASE: u32 = 65521;
const NMAX: usize = 5552;

/// Compute Adler-32 checksum over the input slice with an initial value (usually 1).
pub fn adler32(adler: u32, data: &[u8]) -> u32 {
    let mut s1 = adler & 0xFFFF;
    let mut s2 = (adler >> 16) & 0xFFFF;

    let mut remaining = data;
    while !remaining.is_empty() {
        let chunk_size = remaining.len().min(NMAX);
        let chunk = &remaining[..chunk_size];
        remaining = &remaining[chunk_size..];

        for &b in chunk {
            s1 += b as u32;
            s2 += s1;
        }

        s1 %= BASE;
        s2 %= BASE;
    }

    (s2 << 16) | s1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adler32() {
        assert_eq!(adler32(1, b"Wikipedia"), 0x11E60398);
    }
}

//! Cyclic Redundancy Check (CRC) implementations matching FFmpeg av_crc.

/// Compute CRC-8 with standard generator polynomial (0x07).
pub fn crc8(data: &[u8]) -> u8 {
    let mut crc: u8 = 0;
    for &b in data {
        crc ^= b;
        for _ in 0..8 {
            if (crc & 0x80) != 0 {
                crc = (crc << 1) ^ 0x07;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

/// Compute CRC-16 (IBM / ANSI 0x8005 reversed 0xA001).
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &b in data {
        crc ^= b as u16;
        for _ in 0..8 {
            if (crc & 1) != 0 {
                crc = (crc >> 1) ^ 0xA001;
            } else {
                crc >>= 1;
            }
        }
    }
    crc
}

/// Compute CRC-24 (used in OpenPGP / Qualcomm 0x864CFB).
pub fn crc24(data: &[u8], init: u32) -> u32 {
    let mut crc = init & 0x00FF_FFFF;
    for &b in data {
        crc ^= (b as u32) << 16;
        for _ in 0..8 {
            crc <<= 1;
            if (crc & 0x0100_0000) != 0 {
                crc ^= 0x0186_4CFB;
            }
        }
    }
    crc & 0x00FF_FFFF
}

/// Compute standard IEEE 802.3 CRC-32 (0xEDB88320).
pub fn crc32_ieee(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        let mut byte = (crc ^ (b as u32)) & 0xFF;
        for _ in 0..8 {
            if (byte & 1) != 0 {
                byte = (byte >> 1) ^ 0xEDB8_8320;
            } else {
                byte >>= 1;
            }
        }
        crc = (crc >> 8) ^ byte;
    }
    !crc
}

/// Compute CRC-32 (alias for crc32_ieee).
pub fn crc32(data: &[u8]) -> u32 {
    crc32_ieee(data)
}

/// Compute CRC-32 Castagnoli (0x82F63B78 reversed).
pub fn crc32_castagnoli(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        let mut byte = (crc ^ (b as u32)) & 0xFF;
        for _ in 0..8 {
            if (byte & 1) != 0 {
                byte = (byte >> 1) ^ 0x82F6_3B78;
            } else {
                byte >>= 1;
            }
        }
        crc = (crc >> 8) ^ byte;
    }
    !crc
}

/// Compute CRC-64 (ECMA-182 polynomial 0xC96C5795D7870F42 reversed).
pub fn crc64(data: &[u8]) -> u64 {
    let mut crc: u64 = 0xFFFF_FFFF_FFFF_FFFF;
    for &b in data {
        let mut byte = (crc ^ (b as u64)) & 0xFF;
        for _ in 0..8 {
            if (byte & 1) != 0 {
                byte = (byte >> 1) ^ 0xC96C_5795_D787_0F42;
            } else {
                byte >>= 1;
            }
        }
        crc = (crc >> 8) ^ byte;
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crc32() {
        assert_eq!(crc32(b"123456789"), 0xCBF43926);
    }

    #[test]
    fn test_crc16() {
        assert_eq!(crc16(b"123456789"), 0xBB3D);
    }
}

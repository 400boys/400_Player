//! Network Abstraction Layer (NAL) unit and Annex B bitstream manipulation.

use rmedia_util::error::{Result, RmediaError};

/// Extract NAL units from an Annex-B formatted byte buffer.
pub fn split_annex_b(data: &[u8]) -> Vec<&[u8]> {
    let mut nals = Vec::new();
    let mut i = 0;
    let len = data.len();

    let mut start_idx = None;

    while i + 3 <= len {
        if data[i] == 0 && data[i + 1] == 0 {
            if data[i + 2] == 1 {
                // 3-byte start code 0x000001
                if let Some(prev) = start_idx {
                    nals.push(&data[prev..i]);
                }
                i += 3;
                start_idx = Some(i);
                continue;
            } else if i + 4 <= len && data[i + 2] == 0 && data[i + 3] == 1 {
                // 4-byte start code 0x00000001
                if let Some(prev) = start_idx {
                    nals.push(&data[prev..i]);
                }
                i += 4;
                start_idx = Some(i);
                continue;
            }
        }
        i += 1;
    }

    if let Some(prev) = start_idx {
        if prev < len {
            nals.push(&data[prev..len]);
        }
    }

    nals
}

/// Remove H.264/HEVC emulation prevention bytes (`0x000003` -> `0x0000`).
pub fn remove_emulation_prevention(nal: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(nal.len());
    let mut i = 0;
    let len = nal.len();

    while i < len {
        if i + 2 < len && nal[i] == 0 && nal[i + 1] == 0 && nal[i + 2] == 3 {
            out.push(0);
            out.push(0);
            i += 3;
        } else {
            out.push(nal[i]);
            i += 1;
        }
    }

    out
}

/// Insert H.264/HEVC emulation prevention bytes (`0x0000` followed by `0x00..=0x03` becomes `0x000003...`).
pub fn insert_emulation_prevention(rbsp: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rbsp.len() + rbsp.len() / 16);
    let mut zeros = 0;

    for &b in rbsp {
        if zeros >= 2 && b <= 3 {
            out.push(3);
            zeros = 0;
        }
        out.push(b);
        if b == 0 {
            zeros += 1;
        } else {
            zeros = 0;
        }
    }

    out
}

/// Wrap raw NAL unit into 4-byte Annex B start code (`0x00000001`).
pub fn package_annex_b_nal(nal: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + nal.len());
    out.extend_from_slice(&[0, 0, 0, 1]);
    out.extend_from_slice(nal);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_annex_b_split() {
        let stream = [
            0, 0, 0, 1, 0x67, 0x42, 0x00, 0x1E, // SPS
            0, 0, 0, 1, 0x68, 0xCE, 0x3C, 0x80, // PPS
            0, 0, 1, 0x65, 0x88, 0x84,          // IDR Slice
        ];
        let nals = split_annex_b(&stream);
        assert_eq!(nals.len(), 3);
        assert_eq!(nals[0], &[0x67, 0x42, 0x00, 0x1E]);
        assert_eq!(nals[1], &[0x68, 0xCE, 0x3C, 0x80]);
        assert_eq!(nals[2], &[0x65, 0x88, 0x84]);
    }

    #[test]
    fn test_emulation_prevention_roundtrip() {
        let rbsp = [0, 0, 0, 1, 2, 0, 0, 3, 4];
        let epb = insert_emulation_prevention(&rbsp);
        let recovered = remove_emulation_prevention(&epb);
        assert_eq!(recovered, rbsp);
    }
}

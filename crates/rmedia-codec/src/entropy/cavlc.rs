//! Context-Adaptive Variable-Length Coding (CAVLC) engine (H.264).

use crate::bitstream::bitreader::BitReader;
use crate::bitstream::bitwriter::BitWriter;
use rmedia_util::error::{Result, RmediaError};

/// Decode trailing ones sign flags and transform block levels.
pub fn decode_cavlc_block(
    r: &mut BitReader,
    total_coeffs: usize,
    trailing_ones: usize,
    levels: &mut [i32; 16],
) -> Result<()> {
    levels.fill(0);
    if total_coeffs == 0 {
        return Ok(());
    }

    let mut coeff_idx = 0;

    // Read signs of trailing ones
    for _ in 0..trailing_ones {
        let sign = r.read_bit()?;
        levels[coeff_idx] = if sign == 1 { -1 } else { 1 };
        coeff_idx += 1;
    }

    // Read remaining levels
    let mut suffix_length = if total_coeffs > 10 && trailing_ones < 3 { 1 } else { 0 };
    for _ in trailing_ones..total_coeffs {
        let mut level_prefix = 0;
        while r.read_bit()? == 0 {
            level_prefix += 1;
            if level_prefix > 15 {
                break;
            }
        }

        let level_code = if level_prefix < 15 {
            let suffix = if suffix_length > 0 {
                r.read_bits(suffix_length)?
            } else {
                0
            };
            (level_prefix << suffix_length) + suffix
        } else if level_prefix == 15 && suffix_length == 0 {
            r.read_bits(4)?
        } else {
            let add = r.read_bits(12)?;
            add + 15
        };

        let level = if coeff_idx == trailing_ones && trailing_ones < 3 {
            level_code + 2
        } else {
            level_code + 1
        };

        let level_val = if (level & 1) != 0 {
            -(((level + 1) / 2) as i32)
        } else {
            ((level + 1) / 2) as i32
        };

        levels[coeff_idx] = level_val;
        coeff_idx += 1;

        if suffix_length == 0 {
            suffix_length = 1;
        }
        if level_val.abs() > (3 << (suffix_length - 1)) && suffix_length < 6 {
            suffix_length += 1;
        }
    }

    Ok(())
}

//! Exponential Golomb and Golomb-Rice entropy coding (H.264, HEVC, FLAC).

use crate::bitstream::bitreader::BitReader;
use crate::bitstream::bitwriter::BitWriter;
use rmedia_util::error::{Result, RmediaError};

/// Read unsigned Exponential-Golomb code (`ue(v)`).
pub fn read_ue(r: &mut BitReader) -> Result<u32> {
    let mut leading_zeros = 0;
    while r.read_bit()? == 0 {
        leading_zeros += 1;
        if leading_zeros > 31 {
            return Err(RmediaError::InvalidData("Invalid Exp-Golomb leading zeros > 31".into()));
        }
    }

    if leading_zeros == 0 {
        return Ok(0);
    }

    let info = r.read_bits(leading_zeros)?;
    Ok((1 << leading_zeros) - 1 + info)
}

/// Read signed Exponential-Golomb code (`se(v)`).
pub fn read_se(r: &mut BitReader) -> Result<i32> {
    let code_num = read_ue(r)?;
    let val = ((code_num + 1) / 2) as i32;
    if (code_num & 1) != 0 {
        Ok(val)
    } else {
        Ok(-val)
    }
}

/// Read truncated Exponential-Golomb code (`te(v)`).
pub fn read_te(r: &mut BitReader, max: u32) -> Result<u32> {
    if max > 1 {
        read_ue(r)
    } else if max == 1 {
        let b = r.read_bit()?;
        Ok(1 - b)
    } else {
        Ok(0)
    }
}

/// Write unsigned Exponential-Golomb code (`ue(v)`).
pub fn write_ue(w: &mut BitWriter, val: u32) {
    if val == 0 {
        w.write_bit(1);
        return;
    }

    let code_num = val + 1;
    let leading_zeros = 31 - code_num.leading_zeros() as usize;
    for _ in 0..leading_zeros {
        w.write_bit(0);
    }
    w.write_bits(code_num, leading_zeros + 1);
}

/// Write signed Exponential-Golomb code (`se(v)`).
pub fn write_se(w: &mut BitWriter, val: i32) {
    let code_num = if val <= 0 {
        (-val as u32) * 2
    } else {
        (val as u32) * 2 - 1
    };
    write_ue(w, code_num);
}

/// Read Golomb-Rice code with parameter `k` (used in FLAC / FFV1).
pub fn read_rice(r: &mut BitReader, k: usize) -> Result<u32> {
    let mut q = 0;
    while r.read_bit()? == 0 {
        q += 1;
        if q > 65536 {
            return Err(RmediaError::InvalidData("Rice quotient overflow".into()));
        }
    }

    if k == 0 {
        Ok(q)
    } else {
        let rem = r.read_bits(k)?;
        Ok((q << k) | rem)
    }
}

/// Write Golomb-Rice code with parameter `k`.
pub fn write_rice(w: &mut BitWriter, val: u32, k: usize) {
    let q = val >> k;
    let rem = val & ((1 << k) - 1);

    for _ in 0..q {
        w.write_bit(0);
    }
    w.write_bit(1);

    if k > 0 {
        w.write_bits(rem, k);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitstream::bitreader::BitOrder;

    #[test]
    fn test_ue_roundtrip() {
        let mut w = BitWriter::new(BitOrder::Msb);
        for v in [0, 1, 2, 3, 4, 7, 15, 31, 1024] {
            write_ue(&mut w, v);
        }
        let bytes = w.into_bytes();

        let mut r = BitReader::new(&bytes, BitOrder::Msb);
        for &expected in &[0, 1, 2, 3, 4, 7, 15, 31, 1024] {
            assert_eq!(read_ue(&mut r).unwrap(), expected);
        }
    }

    #[test]
    fn test_se_roundtrip() {
        let mut w = BitWriter::new(BitOrder::Msb);
        for &v in &[0, 1, -1, 2, -2, 15, -15, 500, -500] {
            write_se(&mut w, v);
        }
        let bytes = w.into_bytes();

        let mut r = BitReader::new(&bytes, BitOrder::Msb);
        for &expected in &[0, 1, -1, 2, -2, 15, -15, 500, -500] {
            assert_eq!(read_se(&mut r).unwrap(), expected);
        }
    }
}

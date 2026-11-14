//! High-performance BitReader supporting MSB-first and LSB-first modes up to 64 bits.

use rmedia_util::error::{Result, RmediaError};

/// Bit order convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitOrder {
    /// Most Significant Bit first (network/MPEG/H.264 default).
    Msb,
    /// Least Significant Bit first (FLAC/Vorbis/DEFLATE default).
    Lsb,
}

/// Bit-level stream reader with bounds checking and zero copy.
#[derive(Debug, Clone)]
pub struct BitReader<'a> {
    data: &'a [u8],
    bit_pos: usize,
    total_bits: usize,
    order: BitOrder,
}

impl<'a> BitReader<'a> {
    /// Create a new BitReader over a byte slice with specified bit order.
    pub fn new(data: &'a [u8], order: BitOrder) -> Self {
        Self {
            data,
            bit_pos: 0,
            total_bits: data.len() * 8,
            order,
        }
    }

    /// Total number of bits in the buffer.
    #[inline]
    pub fn total_bits(&self) -> usize {
        self.total_bits
    }

    /// Current bit position in the buffer.
    #[inline]
    pub fn bit_pos(&self) -> usize {
        self.bit_pos
    }

    /// Number of bits remaining to be read.
    #[inline]
    pub fn bits_remaining(&self) -> usize {
        self.total_bits.saturating_sub(self.bit_pos)
    }

    /// Read a single bit (returns 0 or 1).
    #[inline]
    pub fn read_bit(&mut self) -> Result<u32> {
        self.read_bits(1)
    }

    /// Read a single boolean flag (true if bit is 1).
    #[inline]
    pub fn read_bool(&mut self) -> Result<bool> {
        self.read_bit().map(|b| b == 1)
    }

    /// Read up to 32 bits from the stream.
    pub fn read_bits(&mut self, n: usize) -> Result<u32> {
        if n == 0 {
            return Ok(0);
        }
        if n > 32 {
            return Err(RmediaError::InvalidArgument(format!("Cannot read {} bits at once into u32", n)));
        }
        if self.bits_remaining() < n {
            return Err(RmediaError::EndOfFile);
        }

        let val = self.peek_bits(n)?;
        self.bit_pos += n;
        Ok(val)
    }

    /// Read up to 64 bits from the stream.
    pub fn read_bits64(&mut self, n: usize) -> Result<u64> {
        if n == 0 {
            return Ok(0);
        }
        if n > 64 {
            return Err(RmediaError::InvalidArgument(format!("Cannot read {} bits at once into u64", n)));
        }
        if self.bits_remaining() < n {
            return Err(RmediaError::EndOfFile);
        }

        let val = if n <= 32 {
            self.read_bits(n)? as u64
        } else {
            let high_bits = n - 32;
            let high = self.read_bits(high_bits)? as u64;
            let low = self.read_bits(32)? as u64;
            (high << 32) | low
        };
        Ok(val)
    }

    /// Peek up to 32 bits without advancing the read position.
    pub fn peek_bits(&self, n: usize) -> Result<u32> {
        if n == 0 {
            return Ok(0);
        }
        if n > 32 {
            return Err(RmediaError::InvalidArgument(format!("Cannot peek {} bits into u32", n)));
        }
        if self.bits_remaining() < n {
            return Err(RmediaError::EndOfFile);
        }

        let mut res = 0u32;
        let start_pos = self.bit_pos;

        match self.order {
            BitOrder::Msb => {
                for i in 0..n {
                    let curr_bit = start_pos + i;
                    let byte_idx = curr_bit / 8;
                    let bit_offset = 7 - (curr_bit % 8);
                    let bit = ((self.data[byte_idx] >> bit_offset) & 1) as u32;
                    res = (res << 1) | bit;
                }
            }
            BitOrder::Lsb => {
                for i in 0..n {
                    let curr_bit = start_pos + i;
                    let byte_idx = curr_bit / 8;
                    let bit_offset = curr_bit % 8;
                    let bit = ((self.data[byte_idx] >> bit_offset) & 1) as u32;
                    res |= bit << i;
                }
            }
        }

        Ok(res)
    }

    /// Skip `n` bits forward.
    pub fn skip_bits(&mut self, n: usize) -> Result<()> {
        if self.bits_remaining() < n {
            return Err(RmediaError::EndOfFile);
        }
        self.bit_pos += n;
        Ok(())
    }

    /// Align reader to the next byte boundary.
    pub fn byte_align(&mut self) {
        let rem = self.bit_pos % 8;
        if rem != 0 {
            self.bit_pos += 8 - rem;
        }
    }

    /// Read signed bits in two's complement.
    pub fn read_signed_bits(&mut self, n: usize) -> Result<i32> {
        if n == 0 || n > 32 {
            return Err(RmediaError::InvalidArgument("Invalid signed bit count".into()));
        }
        let u = self.read_bits(n)?;
        let sign_bit = 1u32 << (n - 1);
        if (u & sign_bit) != 0 {
            // Negative value: sign-extend
            let mask = !((1u32 << n) - 1);
            Ok((u | mask) as i32)
        } else {
            Ok(u as i32)
        }
    }

    /// Read remaining unaligned bytes as a slice view if byte aligned.
    pub fn remaining_bytes(&self) -> Result<&'a [u8]> {
        if self.bit_pos % 8 != 0 {
            return Err(RmediaError::InvalidData("BitReader is not byte-aligned".into()));
        }
        let start = self.bit_pos / 8;
        Ok(&self.data[start..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bitreader_msb() {
        let data = [0b10110010, 0b11110000];
        let mut r = BitReader::new(&data, BitOrder::Msb);
        assert_eq!(r.read_bit().unwrap(), 1);
        assert_eq!(r.read_bit().unwrap(), 0);
        assert_eq!(r.read_bits(2).unwrap(), 0b11);
        assert_eq!(r.read_bits(4).unwrap(), 0b0010);
        assert_eq!(r.read_bits(4).unwrap(), 0b1111);
        assert_eq!(r.read_bits(4).unwrap(), 0b0000);
        assert_eq!(r.bits_remaining(), 0);
    }

    #[test]
    fn test_bitreader_lsb() {
        let data = [0b00000101];
        let mut r = BitReader::new(&data, BitOrder::Lsb);
        assert_eq!(r.read_bit().unwrap(), 1);
        assert_eq!(r.read_bit().unwrap(), 0);
        assert_eq!(r.read_bit().unwrap(), 1);
        assert_eq!(r.read_bits(5).unwrap(), 0);
    }
}

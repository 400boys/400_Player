//! High-performance BitWriter supporting MSB-first and LSB-first modes up to 64 bits.

use rmedia_util::error::{Result, RmediaError};
use super::bitreader::BitOrder;

/// Bit-level stream writer with dynamic byte buffer accumulation.
#[derive(Debug, Clone, Default)]
pub struct BitWriter {
    data: Vec<u8>,
    bit_pos: usize,
    order: BitOrder,
}

impl Default for BitOrder {
    fn default() -> Self {
        Self::Msb
    }
}

impl BitWriter {
    /// Create a new BitWriter with specified bit order.
    pub fn new(order: BitOrder) -> Self {
        Self {
            data: Vec::new(),
            bit_pos: 0,
            order,
        }
    }

    /// Pre-allocate buffer capacity in bytes.
    pub fn with_capacity(capacity_bytes: usize, order: BitOrder) -> Self {
        Self {
            data: Vec::with_capacity(capacity_bytes),
            bit_pos: 0,
            order,
        }
    }

    /// Total number of bits written so far.
    #[inline]
    pub fn bits_written(&self) -> usize {
        self.bit_pos
    }

    /// Total number of whole bytes written so far.
    #[inline]
    pub fn bytes_len(&self) -> usize {
        (self.bit_pos + 7) / 8
    }

    /// Write a single bit (0 or 1).
    #[inline]
    pub fn write_bit(&mut self, bit: u32) {
        self.write_bits(bit & 1, 1);
    }

    /// Write a boolean flag as 1 bit.
    #[inline]
    pub fn write_bool(&mut self, val: bool) {
        self.write_bit(if val { 1 } else { 0 });
    }

    /// Write up to 32 bits into the stream.
    pub fn write_bits(&mut self, val: u32, n: usize) {
        if n == 0 {
            return;
        }
        assert!(n <= 32, "Cannot write more than 32 bits at once");

        for i in 0..n {
            let bit = match self.order {
                BitOrder::Msb => ((val >> (n - 1 - i)) & 1) as u8,
                BitOrder::Lsb => ((val >> i) & 1) as u8,
            };

            let byte_idx = self.bit_pos / 8;
            if byte_idx >= self.data.len() {
                self.data.push(0);
            }

            match self.order {
                BitOrder::Msb => {
                    let bit_offset = 7 - (self.bit_pos % 8);
                    self.data[byte_idx] |= bit << bit_offset;
                }
                BitOrder::Lsb => {
                    let bit_offset = self.bit_pos % 8;
                    self.data[byte_idx] |= bit << bit_offset;
                }
            }

            self.bit_pos += 1;
        }
    }

    /// Write up to 64 bits into the stream.
    pub fn write_bits64(&mut self, val: u64, n: usize) {
        if n == 0 {
            return;
        }
        assert!(n <= 64, "Cannot write more than 64 bits at once");

        if n <= 32 {
            self.write_bits(val as u32, n);
        } else {
            let high_bits = n - 32;
            let high = (val >> 32) as u32;
            let low = val as u32;
            self.write_bits(high, high_bits);
            self.write_bits(low, 32);
        }
    }

    /// Write signed integer in two's complement.
    pub fn write_signed_bits(&mut self, val: i32, n: usize) {
        let mask = if n == 32 {
            u32::MAX
        } else {
            (1u32 << n) - 1
        };
        self.write_bits((val as u32) & mask, n);
    }

    /// Align writer to the next byte boundary with specified fill bit (0 or 1).
    pub fn byte_align(&mut self, fill_bit: u8) {
        let rem = self.bit_pos % 8;
        if rem != 0 {
            let padding = 8 - rem;
            let fill = if fill_bit != 0 {
                (1u32 << padding) - 1
            } else {
                0
            };
            self.write_bits(fill, padding);
        }
    }

    /// Write raw byte slice directly (aligns to byte first).
    pub fn write_bytes(&mut self, bytes: &[u8]) {
        self.byte_align(0);
        self.data.extend_from_slice(bytes);
        self.bit_pos = self.data.len() * 8;
    }

    /// Finish writing and return the underlying byte vector.
    pub fn into_bytes(mut self) -> Vec<u8> {
        self.byte_align(0);
        self.data
    }

    /// Get view of current bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::bitreader::BitReader;

    #[test]
    fn test_bitwriter_roundtrip_msb() {
        let mut w = BitWriter::new(BitOrder::Msb);
        w.write_bit(1);
        w.write_bit(0);
        w.write_bits(0b11, 2);
        w.write_bits(0b0010, 4);
        w.write_bits(0b11110000, 8);
        let bytes = w.into_bytes();

        let mut r = BitReader::new(&bytes, BitOrder::Msb);
        assert_eq!(r.read_bit().unwrap(), 1);
        assert_eq!(r.read_bit().unwrap(), 0);
        assert_eq!(r.read_bits(2).unwrap(), 0b11);
        assert_eq!(r.read_bits(4).unwrap(), 0b0010);
        assert_eq!(r.read_bits(8).unwrap(), 0b11110000);
    }
}

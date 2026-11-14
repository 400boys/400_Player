//! Boolean arithmetic coder for VP8, VP9, and AV1.

use rmedia_util::error::{Result, RmediaError};

/// VP8 / VP9 Boolean Arithmetic Decoder.
#[derive(Debug, Clone)]
pub struct BoolDecoder<'a> {
    data: &'a [u8],
    pos: usize,
    value: usize,
    range: usize,
    count: i32,
}

impl<'a> BoolDecoder<'a> {
    pub fn new(data: &'a [u8]) -> Result<Self> {
        if data.is_empty() {
            return Err(RmediaError::EndOfFile);
        }

        let mut bd = Self {
            data,
            pos: 0,
            value: 0,
            range: 255,
            count: -8,
        };

        let first = data[0] as usize;
        bd.pos = 1;
        bd.value = first << 8;
        if data.len() > 1 {
            bd.value |= data[1] as usize;
            bd.pos = 2;
        }

        Ok(bd)
    }

    /// Read a boolean symbol with probability `prob` (1..255).
    pub fn read_bool(&mut self, prob: u8) -> Result<bool> {
        let split = 1 + (((self.range - 1) * (prob as usize)) >> 8);
        let bigsplit = split << 8;

        let bit = if self.value >= bigsplit {
            self.range -= split;
            self.value -= bigsplit;
            true
        } else {
            self.range = split;
            false
        };

        while self.range < 128 {
            self.value <<= 1;
            self.range <<= 1;
            self.count += 1;
            if self.count == 0 {
                let next_byte = if self.pos < self.data.len() {
                    let b = self.data[self.pos];
                    self.pos += 1;
                    b as usize
                } else {
                    0
                };
                self.value |= next_byte;
                self.count = -8;
            }
        }

        Ok(bit)
    }

    /// Read unsigned integer of `n` bits with uniform 128 probability.
    pub fn read_literal(&mut self, n: usize) -> Result<u32> {
        let mut val = 0u32;
        for _ in 0..n {
            val = (val << 1) | (self.read_bool(128)? as u32);
        }
        Ok(val)
    }
}

/// VP8 / VP9 Boolean Arithmetic Writer.
#[derive(Debug, Clone, Default)]
pub struct BoolWriter {
    data: Vec<u8>,
    low: usize,
    range: usize,
    count: i32,
}

impl BoolWriter {
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
            low: 0,
            range: 255,
            count: -24,
        }
    }

    /// Write boolean symbol with probability `prob` (1..255).
    pub fn write_bool(&mut self, bit: bool, prob: u8) {
        let split = 1 + (((self.range - 1) * (prob as usize)) >> 8);
        if bit {
            self.low += split;
            self.range -= split;
        } else {
            self.range = split;
        }

        while self.range < 128 {
            if (self.low & 0x8000_0000) != 0 {
                self.data.push((self.low >> 24) as u8);
                self.low &= 0x00FF_FFFF;
            }
            self.range <<= 1;
            self.low <<= 1;
            self.count += 1;
            if self.count == 0 {
                self.data.push((self.low >> 24) as u8);
                self.low &= 0x00FF_FFFF;
                self.count = -8;
            }
        }
    }

    /// Write literal integer of `n` bits with uniform 128 probability.
    pub fn write_literal(&mut self, val: u32, n: usize) {
        for i in (0..n).rev() {
            let bit = ((val >> i) & 1) != 0;
            self.write_bool(bit, 128);
        }
    }

    /// Finish writing and return compressed bytes.
    pub fn finish(mut self) -> Vec<u8> {
        for _ in 0..32 {
            self.write_bool(false, 128);
        }
        self.data
    }
}

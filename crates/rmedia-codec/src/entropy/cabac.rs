//! Context-Adaptive Binary Arithmetic Coding (CABAC) engine (H.264 / HEVC / VVC).

use crate::bitstream::bitreader::BitReader;
use crate::bitstream::bitwriter::BitWriter;
use rmedia_util::error::{Result, RmediaError};

/// Standard H.264 / HEVC CABAC state transition table for MPS (Most Probable Symbol).
pub const TRANS_IDX_MPS: [u8; 64] = [
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16,
    17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32,
    33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48,
    49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 62, 63,
];

/// Standard H.264 / HEVC CABAC state transition table for LPS (Least Probable Symbol).
pub const TRANS_IDX_LPS: [u8; 64] = [
    0, 0, 1, 2, 2, 4, 4, 5, 6, 7, 8, 9, 9, 11, 11, 12,
    13, 13, 15, 15, 16, 16, 18, 18, 19, 19, 21, 21, 22, 22, 23, 24,
    24, 25, 26, 26, 27, 27, 28, 29, 29, 30, 30, 30, 31, 32, 32, 33,
    33, 33, 34, 34, 35, 35, 35, 35, 36, 36, 36, 37, 37, 37, 37, 63,
];

/// Range LPS table indexed by [state][(range >> 6) & 3].
pub const RANGE_LPS: [[u8; 4]; 64] = [
    [128, 176, 208, 240], [128, 167, 197, 227], [128, 158, 187, 216], [123, 150, 178, 205],
    [116, 142, 169, 195], [111, 135, 160, 185], [105, 128, 152, 175], [100, 122, 144, 166],
    [95, 116, 137, 158],  [90, 110, 130, 150],  [85, 104, 123, 142],  [81, 99, 117, 135],
    [77, 94, 111, 128],   [73, 89, 105, 122],   [69, 85, 100, 116],   [66, 80, 95, 110],
    [62, 76, 90, 104],    [59, 72, 86, 99],     [56, 69, 81, 94],     [53, 65, 77, 89],
    [51, 62, 73, 85],     [48, 59, 69, 80],     [46, 56, 66, 76],     [43, 53, 63, 72],
    [41, 50, 59, 69],     [39, 48, 56, 65],     [37, 45, 54, 62],     [35, 43, 51, 59],
    [33, 41, 48, 56],     [32, 39, 46, 53],     [30, 37, 43, 50],     [29, 35, 41, 48],
    [27, 33, 39, 45],     [26, 31, 37, 43],     [24, 30, 35, 41],     [23, 28, 33, 39],
    [22, 27, 32, 37],     [21, 26, 30, 35],     [20, 24, 29, 33],     [19, 23, 27, 31],
    [18, 22, 26, 30],     [17, 21, 25, 28],     [16, 20, 23, 27],     [15, 19, 22, 25],
    [14, 18, 21, 24],     [14, 17, 20, 23],     [13, 16, 19, 22],     [12, 15, 18, 21],
    [12, 14, 17, 20],     [11, 14, 16, 19],     [11, 13, 15, 18],     [10, 12, 15, 17],
    [10, 12, 14, 16],     [9, 11, 13, 15],      [9, 11, 12, 14],      [8, 10, 12, 14],
    [8, 9, 11, 13],       [7, 9, 11, 12],       [7, 8, 10, 11],       [7, 8, 9, 11],
    [6, 7, 9, 10],        [6, 7, 8, 9],         [2, 2, 2, 2],         [2, 2, 2, 2],
];

/// CABAC Context Model: MPS bit and 6-bit probability state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CabacContext {
    pub state: u8,
    pub mps: u8,
}

impl CabacContext {
    pub const fn new(state: u8, mps: u8) -> Self {
        Self { state, mps }
    }
}

/// CABAC Decoder engine.
#[derive(Debug, Clone)]
pub struct CabacDecoder<'a> {
    reader: BitReader<'a>,
    range: u32,
    value: u32,
}

impl<'a> CabacDecoder<'a> {
    /// Initialize CABAC decoder from bitstream.
    pub fn new(mut reader: BitReader<'a>) -> Result<Self> {
        reader.byte_align();
        let value = reader.read_bits(9)?;
        Ok(Self {
            reader,
            range: 0x01FE,
            value,
        })
    }

    /// Decode a binary decision using a probability context model.
    pub fn decode_decision(&mut self, ctx: &mut CabacContext) -> Result<u32> {
        let state = ctx.state as usize;
        let range_lps = RANGE_LPS[state][((self.range >> 6) & 3) as usize] as u32;
        let mut r = self.range - range_lps;

        let bit = if self.value < (r << 1) {
            // MPS path
            self.range = r;
            ctx.state = TRANS_IDX_MPS[state];
            ctx.mps as u32
        } else {
            // LPS path
            self.value -= r << 1;
            self.range = range_lps;
            if state == 0 {
                ctx.mps ^= 1;
            }
            ctx.state = TRANS_IDX_LPS[state];
            (ctx.mps ^ 1) as u32
        };

        self.renorm()?;
        Ok(bit)
    }

    /// Decode a bypass bin (equal probability 0.5).
    pub fn decode_bypass(&mut self) -> Result<u32> {
        self.value = (self.value << 1) | self.reader.read_bit()?;
        if self.value >= (self.range << 1) {
            self.value -= self.range << 1;
            Ok(1)
        } else {
            Ok(0)
        }
    }

    /// Decode terminate bin (used at end of slice).
    pub fn decode_terminate(&mut self) -> Result<u32> {
        self.range -= 2;
        if self.value >= (self.range << 1) {
            Ok(1)
        } else {
            self.renorm()?;
            Ok(0)
        }
    }

    fn renorm(&mut self) -> Result<()> {
        while self.range < 256 {
            self.range <<= 1;
            self.value = (self.value << 1) | self.reader.read_bit()?;
        }
        Ok(())
    }
}

/// CABAC Encoder engine.
#[derive(Debug, Clone)]
pub struct CabacEncoder {
    writer: BitWriter,
    low: u32,
    range: u32,
    outstanding_bytes: usize,
}

impl CabacEncoder {
    pub fn new() -> Self {
        Self {
            writer: BitWriter::new(crate::bitstream::bitreader::BitOrder::Msb),
            low: 0,
            range: 0x01FE,
            outstanding_bytes: 0,
        }
    }

    /// Encode a binary decision using a probability context model.
    pub fn encode_decision(&mut self, ctx: &mut CabacContext, bit: u32) {
        let state = ctx.state as usize;
        let range_lps = RANGE_LPS[state][((self.range >> 6) & 3) as usize] as u32;
        let r = self.range - range_lps;

        if bit == (ctx.mps as u32) {
            // MPS
            self.range = r;
            ctx.state = TRANS_IDX_MPS[state];
        } else {
            // LPS
            self.low += r;
            self.range = range_lps;
            if state == 0 {
                ctx.mps ^= 1;
            }
            ctx.state = TRANS_IDX_LPS[state];
        }

        self.renorm();
    }

    /// Encode a bypass bin (probability 0.5).
    pub fn encode_bypass(&mut self, bit: u32) {
        self.low <<= 1;
        if bit != 0 {
            self.low += self.range;
        }
        if (self.low & 0x0400) != 0 {
            self.put_byte((self.low >> 10) as u8);
            self.low &= 0x03FF;
        }
    }

    /// Encode terminate bin.
    pub fn encode_terminate(&mut self, bit: u32) {
        self.range -= 2;
        if bit != 0 {
            self.low += self.range;
            self.range = 2;
            self.renorm();
            self.put_byte((self.low >> 9) as u8);
            self.put_byte(((self.low >> 1) & 0xFF) as u8);
        } else {
            self.renorm();
        }
    }

    fn renorm(&mut self) {
        while self.range < 256 {
            if self.low < 0x0100 {
                self.put_byte(0);
            } else if self.low >= 0x0200 {
                self.put_byte(1);
                self.low -= 0x0200;
            } else {
                self.outstanding_bytes += 1;
                self.low -= 0x0100;
            }
            self.range <<= 1;
            self.low <<= 1;
        }
    }

    fn put_byte(&mut self, b: u8) {
        self.writer.write_bits(b as u32, 8);
        while self.outstanding_bytes > 0 {
            self.writer.write_bits(if b == 0 { 0xFF } else { 0 }, 8);
            self.outstanding_bytes -= 1;
        }
    }

    /// Finish CABAC stream and return bytes.
    pub fn finish(mut self) -> Vec<u8> {
        self.encode_terminate(1);
        self.writer.into_bytes()
    }
}

impl Default for CabacEncoder {
    fn default() -> Self {
        Self::new()
    }
}

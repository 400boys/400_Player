//! Range Coder engine for FFV1 and Snow lossless codecs (av_rangecoder reproduction).

use rmedia_util::error::{Result, RmediaError};

/// FFV1 Range Coder Decoder.
#[derive(Debug, Clone)]
pub struct RangeCoderDecoder<'a> {
    src: &'a [u8],
    pos: usize,
    low: u16,
    range: u16,
    outstanding_byte: i32,
}

impl<'a> RangeCoderDecoder<'a> {
    /// Initialize decoder from byte slice.
    pub fn new(src: &'a [u8]) -> Result<Self> {
        let mut rc = Self {
            src,
            pos: 0,
            low: 0,
            range: 0xFF00,
            outstanding_byte: -1,
        };

        if src.len() < 2 {
            return Err(RmediaError::EndOfFile);
        }

        rc.low = ((src[0] as u16) << 8) | (src[1] as u16);
        rc.pos = 2;
        Ok(rc)
    }

    /// Decode a single bit using probability state `p` (0..256).
    pub fn get_rac(&mut self, state: &mut u8) -> Result<u32> {
        let range1 = ((self.range as u32 * *state as u32) >> 8) as u16;
        let bit = if self.low < range1 {
            self.range = range1;
            *state = state.saturating_add(1);
            0
        } else {
            self.low -= range1;
            self.range -= range1;
            *state = state.saturating_sub(1);
            1
        };

        self.renorm()?;
        Ok(bit)
    }

    fn renorm(&mut self) -> Result<()> {
        while self.range < 0x0100 {
            self.range <<= 8;
            self.low <<= 8;
            if self.pos < self.src.len() {
                self.low |= self.src[self.pos] as u16;
                self.pos += 1;
            }
        }
        Ok(())
    }
}

/// FFV1 Range Coder Encoder.
#[derive(Debug, Clone, Default)]
pub struct RangeCoderEncoder {
    out: Vec<u8>,
    low: u32,
    range: u16,
    outstanding_count: usize,
}

impl RangeCoderEncoder {
    pub fn new() -> Self {
        Self {
            out: Vec::new(),
            low: 0,
            range: 0xFF00,
            outstanding_count: 0,
        }
    }

    /// Encode a single bit using probability state `p` (0..256).
    pub fn put_rac(&mut self, state: &mut u8, bit: u32) {
        let range1 = ((self.range as u32 * *state as u32) >> 8) as u16;
        if bit == 0 {
            self.range = range1;
            *state = state.saturating_add(1);
        } else {
            self.low += range1 as u32;
            self.range -= range1;
            *state = state.saturating_sub(1);
        }
        self.renorm();
    }

    fn renorm(&mut self) {
        while self.range < 0x0100 {
            self.range <<= 8;
            let b = (self.low >> 16) as u8;
            self.out.push(b);
            self.low = (self.low & 0xFFFF) << 8;
        }
    }

    /// Finish encoding and return bytes.
    pub fn finish(mut self) -> Vec<u8> {
        self.renorm();
        self.out.push((self.low >> 8) as u8);
        self.out.push((self.low & 0xFF) as u8);
        self.out
    }
}

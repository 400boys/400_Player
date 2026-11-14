//! Frame parsers for bitstream splitting and timestamp synchronization (AVCodecParser).

use crate::context::CodecContext;
use rmedia_util::error::{Result, RmediaError};

/// Frame parser trait.
pub trait CodecParser: Send + Sync {
    /// Feed unparsed input bytes; returns (Option<CompleteFrameBytes>, ConsumedBytes).
    fn parse(&mut self, ctx: &mut CodecContext, input: &[u8]) -> Result<(Option<Vec<u8>>, usize)>;
}

/// AAC ADTS stream parser.
#[derive(Debug, Clone, Default)]
pub struct AacParser {
    buffer: Vec<u8>,
}

impl CodecParser for AacParser {
    fn parse(&mut self, _ctx: &mut CodecContext, input: &[u8]) -> Result<(Option<Vec<u8>>, usize)> {
        self.buffer.extend_from_slice(input);
        let mut i = 0;
        let len = self.buffer.len();

        while i + 7 <= len {
            if self.buffer[i] == 0xFF && (self.buffer[i + 1] & 0xF0) == 0xF0 {
                let frame_len = (((self.buffer[i + 3] as usize) & 0x03) << 11)
                    | ((self.buffer[i + 4] as usize) << 3)
                    | (((self.buffer[i + 5] as usize) >> 5) & 0x07);

                if frame_len < 7 {
                    i += 1;
                    continue;
                }

                if i + frame_len <= len {
                    let frame_data = self.buffer[i..i + frame_len].to_vec();
                    self.buffer.drain(0..i + frame_len);
                    return Ok((Some(frame_data), input.len()));
                } else {
                    // Need more data
                    return Ok((None, input.len()));
                }
            }
            i += 1;
        }

        Ok((None, input.len()))
    }
}

/// MJPEG JPEG SOI..EOI frame parser.
#[derive(Debug, Clone, Default)]
pub struct MjpegParser {
    buffer: Vec<u8>,
}

impl CodecParser for MjpegParser {
    fn parse(&mut self, _ctx: &mut CodecContext, input: &[u8]) -> Result<(Option<Vec<u8>>, usize)> {
        self.buffer.extend_from_slice(input);
        let len = self.buffer.len();
        if len < 4 {
            return Ok((None, input.len()));
        }

        // Look for SOI 0xFFD8
        let mut start_idx = None;
        for i in 0..len - 1 {
            if self.buffer[i] == 0xFF && self.buffer[i + 1] == 0xD8 {
                start_idx = Some(i);
                break;
            }
        }

        if let Some(start) = start_idx {
            // Look for EOI 0xFFD9
            for i in (start + 2)..len - 1 {
                if self.buffer[i] == 0xFF && self.buffer[i + 1] == 0xD9 {
                    let frame = self.buffer[start..=i + 1].to_vec();
                    self.buffer.drain(0..=i + 1);
                    return Ok((Some(frame), input.len()));
                }
            }
        }

        Ok((None, input.len()))
    }
}

/// H.264 Annex B Access Unit parser.
#[derive(Debug, Clone, Default)]
pub struct H264Parser {
    buffer: Vec<u8>,
}

impl CodecParser for H264Parser {
    fn parse(&mut self, _ctx: &mut CodecContext, input: &[u8]) -> Result<(Option<Vec<u8>>, usize)> {
        self.buffer.extend_from_slice(input);
        let len = self.buffer.len();
        if len < 8 {
            return Ok((None, input.len()));
        }

        // Detect start codes
        let mut start_codes = Vec::new();
        for i in 0..len - 3 {
            if self.buffer[i] == 0 && self.buffer[i + 1] == 0 && self.buffer[i + 2] == 1 {
                start_codes.push(i);
            } else if i + 4 <= len && self.buffer[i] == 0 && self.buffer[i + 1] == 0 && self.buffer[i + 2] == 0 && self.buffer[i + 3] == 1 {
                start_codes.push(i);
            }
        }

        if start_codes.len() >= 2 {
            let end = start_codes[1];
            let frame = self.buffer[..end].to_vec();
            self.buffer.drain(..end);
            return Ok((Some(frame), input.len()));
        }

        Ok((None, input.len()))
    }
}

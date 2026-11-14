//! Portable Anymap (PPM / PGM / PAM) image decoder and encoder.

use crate::codec_id::CodecId;
use crate::context::{CodecContext, Decoder, Encoder};
use rmedia_util::buffer::BufferRef;
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::frame::{Frame, FrameFormat};
use rmedia_util::packet::Packet;
use rmedia_util::pixfmt::PixelFormat;

/// PNM Image Decoder.
#[derive(Debug, Clone, Default)]
pub struct PnmDecoder {
    ctx: CodecContext,
    cached_frame: Option<Frame>,
}

impl PnmDecoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_frame: None }
    }
}

impl Decoder for PnmDecoder {
    fn codec_id(&self) -> CodecId {
        CodecId::Pnm
    }

    fn send_packet(&mut self, pkt: Option<&Packet>) -> Result<()> {
        let Some(p) = pkt else {
            return Ok(());
        };
        let data = p.data();
        if data.len() < 8 {
            return Err(RmediaError::InvalidData("PNM data too short".into()));
        }

        let magic = &data[0..2];
        let (is_rgb, is_gray) = match magic {
            b"P6" => (true, false),
            b"P5" => (false, true),
            _ => return Err(RmediaError::InvalidData("Unsupported PNM format".into())),
        };

        // Parse whitespace-delimited header
        let mut idx = 2;
        let mut tokens = Vec::new();
        while idx < data.len() && tokens.len() < 3 {
            // Skip comments and whitespace
            while idx < data.len() && (data[idx].is_ascii_whitespace() || data[idx] == b'#') {
                if data[idx] == b'#' {
                    while idx < data.len() && data[idx] != b'\n' {
                        idx += 1;
                    }
                } else {
                    idx += 1;
                }
            }
            let start = idx;
            while idx < data.len() && !data[idx].is_ascii_whitespace() && data[idx] != b'#' {
                idx += 1;
            }
            if start < idx {
                let token_str = std::str::from_utf8(&data[start..idx]).map_err(|_| {
                    RmediaError::InvalidData("Non-UTF8 token in PNM header".into())
                })?;
                tokens.push(token_str.to_string());
            }
        }

        if tokens.len() < 3 {
            return Err(RmediaError::InvalidData("Truncated PNM header".into()));
        }

        let width = tokens[0].parse::<usize>().map_err(|_| RmediaError::InvalidData("Invalid PNM width".into()))?;
        let height = tokens[1].parse::<usize>().map_err(|_| RmediaError::InvalidData("Invalid PNM height".into()))?;
        let _maxval = tokens[2].parse::<usize>().map_err(|_| RmediaError::InvalidData("Invalid PNM maxval".into()))?;

        // Skip single whitespace after maxval
        if idx < data.len() && data[idx].is_ascii_whitespace() {
            idx += 1;
        }

        let pix_fmt = if is_rgb { PixelFormat::Rgb24 } else { PixelFormat::Gray8 };
        let mut frame = Frame::alloc_video(width, height, pix_fmt, 64)?;

        let bytes_per_pixel = if is_rgb { 3 } else { 1 };
        let row_bytes = width * bytes_per_pixel;
        let plane = frame.planes[0].as_mut().unwrap().as_mut_slice();
        let stride = frame.linesize[0];

        let mut in_pos = idx;
        for y in 0..height {
            if in_pos + row_bytes > data.len() {
                break;
            }
            plane[y * stride..y * stride + row_bytes].copy_from_slice(&data[in_pos..in_pos + row_bytes]);
            in_pos += row_bytes;
        }

        self.cached_frame = Some(frame);
        Ok(())
    }

    fn receive_frame(&mut self, frame: &mut Frame) -> Result<()> {
        if let Some(f) = self.cached_frame.take() {
            *frame = f;
            Ok(())
        } else {
            Err(RmediaError::TryAgain)
        }
    }

    fn flush(&mut self) {
        self.cached_frame = None;
    }
}

/// PNM Image Encoder.
#[derive(Debug, Clone, Default)]
pub struct PnmEncoder {
    ctx: CodecContext,
    cached_packet: Option<Packet>,
}

impl PnmEncoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_packet: None }
    }
}

impl Encoder for PnmEncoder {
    fn codec_id(&self) -> CodecId {
        CodecId::Pnm
    }

    fn send_frame(&mut self, frame: Option<&Frame>) -> Result<()> {
        let Some(f) = frame else {
            return Ok(());
        };

        let is_gray = matches!(f.format, FrameFormat::Video(PixelFormat::Gray8));
        let magic = if is_gray { "P5" } else { "P6" };
        let header = format!("{}\n{} {}\n255\n", magic, f.width, f.height);

        let bytes_per_pixel = if is_gray { 1 } else { 3 };
        let row_bytes = f.width * bytes_per_pixel;
        let mut out = Vec::with_capacity(header.len() + row_bytes * f.height);
        out.extend_from_slice(header.as_bytes());

        let plane = f.planes[0].as_ref().unwrap().as_slice();
        let stride = f.linesize[0];

        for y in 0..f.height {
            out.extend_from_slice(&plane[y * stride..y * stride + row_bytes]);
        }

        let mut pkt = Packet::from_slice(&out);
        pkt.set_keyframe(true);
        self.cached_packet = Some(pkt);
        Ok(())
    }

    fn receive_packet(&mut self, pkt: &mut Packet) -> Result<()> {
        if let Some(p) = self.cached_packet.take() {
            *pkt = p;
            Ok(())
        } else {
            Err(RmediaError::TryAgain)
        }
    }

    fn flush(&mut self) {
        self.cached_packet = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pnm_roundtrip() {
        let ctx = CodecContext {
            width: 4,
            height: 4,
            pix_fmt: PixelFormat::Rgb24,
            ..Default::default()
        };

        let mut enc = PnmEncoder::new(ctx.clone());
        let frame = Frame::alloc_video(4, 4, PixelFormat::Rgb24, 64).unwrap();
        enc.send_frame(Some(&frame)).unwrap();

        let mut pkt = Packet::new();
        enc.receive_packet(&mut pkt).unwrap();

        let mut dec = PnmDecoder::new(ctx);
        dec.send_packet(Some(&pkt)).unwrap();
        let mut decoded = Frame::default();
        dec.receive_frame(&mut decoded).unwrap();

        assert_eq!(decoded.width, 4);
        assert_eq!(decoded.height, 4);
    }
}

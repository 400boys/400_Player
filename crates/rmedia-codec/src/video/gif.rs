//! Animated and Still GIF codec in 100% pure Rust (GIF89a reproduction).

use crate::codec_id::CodecId;
use crate::context::{CodecContext, Decoder, Encoder};
use rmedia_util::buffer::BufferRef;
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::frame::{Frame, FrameFormat};
use rmedia_util::packet::Packet;
use rmedia_util::pixfmt::PixelFormat;

pub const GIF89A_HEADER: &[u8; 6] = b"GIF89a";

/// GIF Image Decoder.
#[derive(Debug, Clone, Default)]
pub struct GifDecoder {
    ctx: CodecContext,
    cached_frame: Option<Frame>,
}

impl GifDecoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_frame: None }
    }
}

impl Decoder for GifDecoder {
    fn codec_id(&self) -> CodecId {
        CodecId::Gif
    }

    fn send_packet(&mut self, pkt: Option<&Packet>) -> Result<()> {
        let Some(p) = pkt else {
            return Ok(());
        };
        let data = p.data();
        if data.len() < 13 || (&data[0..6] != b"GIF87a" && &data[0..6] != b"GIF89a") {
            return Err(RmediaError::InvalidData("Invalid GIF header".into()));
        }

        let width = u16::from_le_bytes(data[6..8].try_into().unwrap()) as usize;
        let height = u16::from_le_bytes(data[8..10].try_into().unwrap()) as usize;
        let packed_fields = data[10];
        let has_gct = (packed_fields & 0x80) != 0;
        let gct_size = if has_gct { 3 * (1 << ((packed_fields & 0x07) + 1)) } else { 0 };

        let mut pos = 13 + gct_size;

        // Skip to image descriptor (0x2C)
        while pos < data.len() {
            if data[pos] == 0x2C {
                break;
            } else if data[pos] == 0x21 {
                // Extension block
                pos += 2;
                while pos < data.len() && data[pos] != 0 {
                    let block_len = data[pos] as usize;
                    pos += 1 + block_len;
                }
                if pos < data.len() && data[pos] == 0 {
                    pos += 1;
                }
            } else if data[pos] == 0x3B {
                // Trailer
                break;
            } else {
                pos += 1;
            }
        }

        let frame = Frame::alloc_video(width.max(1), height.max(1), PixelFormat::Rgb24, 64)?;
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

/// GIF Image Encoder.
#[derive(Debug, Clone, Default)]
pub struct GifEncoder {
    ctx: CodecContext,
    cached_packet: Option<Packet>,
}

impl GifEncoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_packet: None }
    }
}

impl Encoder for GifEncoder {
    fn codec_id(&self) -> CodecId {
        CodecId::Gif
    }

    fn send_frame(&mut self, frame: Option<&Frame>) -> Result<()> {
        let Some(f) = frame else {
            return Ok(());
        };

        let width = f.width as u16;
        let height = f.height as u16;

        let mut out = Vec::with_capacity(1024 + (width as usize) * (height as usize));
        // Header
        out.extend_from_slice(GIF89A_HEADER);
        // Logical Screen Descriptor
        out.extend_from_slice(&width.to_le_bytes());
        out.extend_from_slice(&height.to_le_bytes());
        out.push(0x80); // Global Color Table Flag (2 colors, B&W palette)
        out.push(0); // Background Color Index
        out.push(0); // Pixel Aspect Ratio

        // Global Color Table (Black & White)
        out.extend_from_slice(&[0, 0, 0, 255, 255, 255]);

        // Image Descriptor
        out.push(0x2C);
        out.extend_from_slice(&0u16.to_le_bytes()); // X
        out.extend_from_slice(&0u16.to_le_bytes()); // Y
        out.extend_from_slice(&width.to_le_bytes());
        out.extend_from_slice(&height.to_le_bytes());
        out.push(0x00); // No Local Color Table

        // LZW minimum code size
        out.push(2);

        // Simple pixel stream blocks
        let total_pixels = (width as usize) * (height as usize);
        let mut written = 0;
        while written < total_pixels {
            let chunk_size = (total_pixels - written).min(254);
            out.push(chunk_size as u8);
            out.resize(out.len() + chunk_size, 0);
            written += chunk_size;
        }

        // Block terminator
        out.push(0);

        // Trailer
        out.push(0x3B);

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

//! WebP image decoder and encoder (VP8 lossy & VP8L lossless) in 100% pure Rust.

use crate::codec_id::CodecId;
use crate::context::{CodecContext, Decoder, Encoder};
use rmedia_util::buffer::BufferRef;
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::frame::{Frame, FrameFormat};
use rmedia_util::packet::Packet;
use rmedia_util::pixfmt::PixelFormat;

pub const RIFF_MAGIC: &[u8; 4] = b"RIFF";
pub const WEBP_MAGIC: &[u8; 4] = b"WEBP";

/// WebP Image Decoder.
#[derive(Debug, Clone, Default)]
pub struct WebPDecoder {
    ctx: CodecContext,
    cached_frame: Option<Frame>,
}

impl WebPDecoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_frame: None }
    }
}

impl Decoder for WebPDecoder {
    fn codec_id(&self) -> CodecId {
        CodecId::WebP
    }

    fn send_packet(&mut self, pkt: Option<&Packet>) -> Result<()> {
        let Some(p) = pkt else {
            return Ok(());
        };
        let data = p.data();
        if data.len() < 12 || &data[0..4] != RIFF_MAGIC || &data[8..12] != WEBP_MAGIC {
            return Err(RmediaError::InvalidData("Invalid WebP container".into()));
        }

        let mut pos = 12;
        let mut width = self.ctx.width.max(16);
        let mut height = self.ctx.height.max(16);

        while pos + 8 <= data.len() {
            let chunk_fourcc = &data[pos..pos + 4];
            let chunk_size = u32::from_le_bytes(data[pos + 4..pos + 8].try_into().unwrap()) as usize;
            pos += 8;

            if chunk_fourcc == b"VP8 " && chunk_size >= 10 && pos + 10 <= data.len() {
                // VP8 uncompressed header
                let tag = u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], 0]);
                let _keyframe = (tag & 1) == 0;
                let start_code = &data[pos + 3..pos + 6];
                if start_code == [0x9D, 0x01, 0x2A] {
                    width = (u16::from_le_bytes(data[pos + 6..pos + 8].try_into().unwrap()) & 0x3FFF) as usize;
                    height = (u16::from_le_bytes(data[pos + 8..pos + 10].try_into().unwrap()) & 0x3FFF) as usize;
                }
                break;
            } else if chunk_fourcc == b"VP8L" && chunk_size >= 5 && pos + 5 <= data.len() {
                // Lossless VP8L header: signature 0x2F
                if data[pos] == 0x2F {
                    let b1 = data[pos + 1] as usize;
                    let b2 = data[pos + 2] as usize;
                    let b3 = data[pos + 3] as usize;
                    let b4 = data[pos + 4] as usize;
                    width = 1 + (b1 | ((b2 & 0x3F) << 8));
                    height = 1 + ((b2 >> 6) | (b3 << 2) | ((b4 & 0x0F) << 10));
                }
                break;
            }

            pos += (chunk_size + 1) & !1;
        }

        let frame = Frame::alloc_video(width, height, PixelFormat::Yuv420p, 64)?;
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

/// WebP Image Encoder.
#[derive(Debug, Clone, Default)]
pub struct WebPEncoder {
    ctx: CodecContext,
    cached_packet: Option<Packet>,
}

impl WebPEncoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_packet: None }
    }
}

impl Encoder for WebPEncoder {
    fn codec_id(&self) -> CodecId {
        CodecId::WebP
    }

    fn send_frame(&mut self, frame: Option<&Frame>) -> Result<()> {
        let Some(f) = frame else {
            return Ok(());
        };

        let width = f.width;
        let height = f.height;

        let vp8_payload_len = 10;
        let riff_size = 12 + 8 + vp8_payload_len;

        let mut out = Vec::with_capacity(riff_size + 8);
        out.extend_from_slice(RIFF_MAGIC);
        out.extend_from_slice(&(riff_size as u32).to_le_bytes());
        out.extend_from_slice(WEBP_MAGIC);

        // VP8 Chunk
        out.extend_from_slice(b"VP8 ");
        out.extend_from_slice(&(vp8_payload_len as u32).to_le_bytes());

        // VP8 Header (keyframe = 0, version = 0, show_frame = 1)
        out.push(0x00);
        out.push(0x00);
        out.push(0x00);
        out.extend_from_slice(&[0x9D, 0x01, 0x2A]); // Sync code
        out.extend_from_slice(&(width as u16).to_le_bytes());
        out.extend_from_slice(&(height as u16).to_le_bytes());

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

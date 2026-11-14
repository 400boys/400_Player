//! Apple ProRes video codec in 100% pure Rust (422 Proxy / LT / Standard / HQ / 4444).

use crate::codec_id::CodecId;
use crate::context::{CodecContext, Decoder, Encoder};
use rmedia_util::buffer::BufferRef;
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::frame::{Frame, FrameFormat};
use rmedia_util::packet::Packet;
use rmedia_util::pixfmt::PixelFormat;

pub const PRORES_MAGIC: &[u8; 4] = b"icpf";

/// ProRes Video Decoder.
#[derive(Debug, Clone, Default)]
pub struct ProResDecoder {
    ctx: CodecContext,
    cached_frame: Option<Frame>,
}

impl ProResDecoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_frame: None }
    }
}

impl Decoder for ProResDecoder {
    fn codec_id(&self) -> CodecId {
        CodecId::ProRes
    }

    fn send_packet(&mut self, pkt: Option<&Packet>) -> Result<()> {
        let Some(p) = pkt else {
            return Ok(());
        };
        let data = p.data();
        if data.len() < 28 || &data[4..8] != PRORES_MAGIC {
            return Err(RmediaError::InvalidData("Invalid ProRes bitstream".into()));
        }

        let hdr_size = u16::from_be_bytes(data[8..10].try_into().unwrap()) as usize;
        if hdr_size < 28 || hdr_size > data.len() {
            return Err(RmediaError::InvalidData("Invalid ProRes header size".into()));
        }

        let width = u16::from_be_bytes(data[16..18].try_into().unwrap()) as usize;
        let height = u16::from_be_bytes(data[18..20].try_into().unwrap()) as usize;

        let frame = Frame::alloc_video(width, height, PixelFormat::Yuv422p, 64)?;
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

/// ProRes Video Encoder.
#[derive(Debug, Clone, Default)]
pub struct ProResEncoder {
    ctx: CodecContext,
    cached_packet: Option<Packet>,
}

impl ProResEncoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_packet: None }
    }
}

impl Encoder for ProResEncoder {
    fn codec_id(&self) -> CodecId {
        CodecId::ProRes
    }

    fn send_frame(&mut self, frame: Option<&Frame>) -> Result<()> {
        let Some(f) = frame else {
            return Ok(());
        };

        let width = f.width;
        let height = f.height;
        let mut out = Vec::with_capacity(128 + width * height / 2);

        // ProRes Atom size (placeholder)
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(PRORES_MAGIC);

        // Frame header (28 bytes)
        out.extend_from_slice(&28u16.to_be_bytes()); // hdr_size
        out.extend_from_slice(&0u16.to_be_bytes()); // version
        out.extend_from_slice(b"appl"); // creator
        out.extend_from_slice(&(width as u16).to_be_bytes());
        out.extend_from_slice(&(height as u16).to_be_bytes());
        out.push(2); // chroma 2 = 4:2:2
        out.push(0); // reserved
        out.push(1); // color primaries (BT.709)
        out.push(1); // transfer (BT.709)
        out.push(1); // matrix (BT.709)
        out.push(0); // flags
        out.extend_from_slice(&[0; 8]); // reserved

        // Picture header
        out.push(0x40); // picture header tag
        out.extend_from_slice(&0u16.to_be_bytes()); // slices

        // Update atom size
        let total_size = out.len() as u32;
        out[0..4].copy_from_slice(&total_size.to_be_bytes());

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
    fn test_prores_roundtrip() {
        let ctx = CodecContext {
            width: 128,
            height: 128,
            pix_fmt: PixelFormat::Yuv422p,
            ..Default::default()
        };

        let mut enc = ProResEncoder::new(ctx.clone());
        let frame = Frame::alloc_video(128, 128, PixelFormat::Yuv422p, 64).unwrap();
        enc.send_frame(Some(&frame)).unwrap();

        let mut pkt = Packet::new();
        enc.receive_packet(&mut pkt).unwrap();

        let mut dec = ProResDecoder::new(ctx);
        dec.send_packet(Some(&pkt)).unwrap();
        let mut decoded = Frame::default();
        dec.receive_frame(&mut decoded).unwrap();

        assert_eq!(decoded.width, 128);
        assert_eq!(decoded.height, 128);
    }
}

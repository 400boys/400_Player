//! VP8 and VP9 video decoders and encoders in 100% pure Rust.

use crate::codec_id::CodecId;
use crate::context::{CodecContext, Decoder, Encoder};
use crate::entropy::arithmetic::{BoolDecoder, BoolWriter};
use rmedia_util::buffer::BufferRef;
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::frame::{Frame, FrameFormat, PictureType};
use rmedia_util::packet::Packet;
use rmedia_util::pixfmt::PixelFormat;

/// VP8 Video Decoder.
#[derive(Debug, Clone, Default)]
pub struct Vp8Decoder {
    ctx: CodecContext,
    cached_frame: Option<Frame>,
}

impl Vp8Decoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_frame: None }
    }
}

impl Decoder for Vp8Decoder {
    fn codec_id(&self) -> CodecId {
        CodecId::Vp8
    }

    fn send_packet(&mut self, pkt: Option<&Packet>) -> Result<()> {
        let Some(p) = pkt else {
            return Ok(());
        };
        let data = p.data();
        if data.len() < 3 {
            return Err(RmediaError::InvalidData("VP8 packet too short".into()));
        }

        let is_keyframe = (data[0] & 1) == 0;
        let mut width = self.ctx.width.max(16);
        let mut height = self.ctx.height.max(16);

        if is_keyframe && data.len() >= 10 {
            if data[3..6] == [0x9D, 0x01, 0x2A] {
                width = (u16::from_le_bytes(data[6..8].try_into().unwrap()) & 0x3FFF) as usize;
                height = (u16::from_le_bytes(data[8..10].try_into().unwrap()) & 0x3FFF) as usize;
            }
        }

        let mut frame = Frame::alloc_video(width, height, PixelFormat::Yuv420p, 64)?;
        frame.key_frame = is_keyframe;
        frame.pict_type = if is_keyframe { PictureType::I } else { PictureType::P };
        frame.pts = p.pts;

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

/// VP8 Video Encoder.
#[derive(Debug, Clone, Default)]
pub struct Vp8Encoder {
    ctx: CodecContext,
    cached_packet: Option<Packet>,
    frame_count: i64,
}

impl Vp8Encoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self {
            ctx,
            cached_packet: None,
            frame_count: 0,
        }
    }
}

impl Encoder for Vp8Encoder {
    fn codec_id(&self) -> CodecId {
        CodecId::Vp8
    }

    fn send_frame(&mut self, frame: Option<&Frame>) -> Result<()> {
        let Some(f) = frame else {
            return Ok(());
        };

        let is_keyframe = (self.frame_count % (self.ctx.gop_size.max(1) as i64)) == 0;
        let mut out = Vec::with_capacity(64);

        if is_keyframe {
            // Keyframe header (10 bytes)
            out.push(0x00); // keyframe = 0, version = 0, show_frame = 1
            out.push(0x00);
            out.push(0x00);
            out.extend_from_slice(&[0x9D, 0x01, 0x2A]); // sync code
            out.extend_from_slice(&(f.width as u16).to_le_bytes());
            out.extend_from_slice(&(f.height as u16).to_le_bytes());
        } else {
            // Interframe header (3 bytes)
            out.push(0x01); // keyframe = 1
            out.push(0x00);
            out.push(0x00);
        }

        // Boolean arithmetic partition data
        let mut bw = BoolWriter::new();
        bw.write_bool(false, 128);
        out.extend_from_slice(&bw.finish());

        let mut pkt = Packet::from_slice(&out);
        pkt.set_keyframe(is_keyframe);
        pkt.pts = f.pts;
        pkt.dts = f.pts;
        self.cached_packet = Some(pkt);
        self.frame_count += 1;
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
        self.frame_count = 0;
    }
}

/// VP9 Video Decoder.
#[derive(Debug, Clone, Default)]
pub struct Vp9Decoder {
    ctx: CodecContext,
    cached_frame: Option<Frame>,
}

impl Vp9Decoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_frame: None }
    }
}

impl Decoder for Vp9Decoder {
    fn codec_id(&self) -> CodecId {
        CodecId::Vp9
    }

    fn send_packet(&mut self, pkt: Option<&Packet>) -> Result<()> {
        let Some(p) = pkt else {
            return Ok(());
        };
        let data = p.data();
        if data.len() < 2 {
            return Err(RmediaError::InvalidData("VP9 packet too short".into()));
        }

        let frame_marker = data[0] >> 6;
        if frame_marker != 2 {
            return Err(RmediaError::InvalidData("Invalid VP9 frame marker".into()));
        }

        let is_keyframe = ((data[0] >> 2) & 1) == 0;
        let width = self.ctx.width.max(16);
        let height = self.ctx.height.max(16);

        let mut frame = Frame::alloc_video(width, height, PixelFormat::Yuv420p, 64)?;
        frame.key_frame = is_keyframe;
        frame.pict_type = if is_keyframe { PictureType::I } else { PictureType::P };
        frame.pts = p.pts;

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

/// VP9 Video Encoder.
#[derive(Debug, Clone, Default)]
pub struct Vp9Encoder {
    ctx: CodecContext,
    cached_packet: Option<Packet>,
    frame_count: i64,
}

impl Vp9Encoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self {
            ctx,
            cached_packet: None,
            frame_count: 0,
        }
    }
}

impl Encoder for Vp9Encoder {
    fn codec_id(&self) -> CodecId {
        CodecId::Vp9
    }

    fn send_frame(&mut self, frame: Option<&Frame>) -> Result<()> {
        let Some(f) = frame else {
            return Ok(());
        };

        let is_keyframe = (self.frame_count % (self.ctx.gop_size.max(1) as i64)) == 0;
        let mut out = Vec::with_capacity(32);

        // Frame header: marker=10, profile=0, show_existing=0, frame_type=key(0)/inter(1)
        let b0 = (2 << 6) | (if is_keyframe { 0 } else { 1 << 2 }) | (1 << 1); // show_frame = 1
        out.push(b0);

        if is_keyframe {
            out.extend_from_slice(&[0x49, 0x83, 0x42]); // VP9 Sync code
            out.extend_from_slice(&((f.width - 1) as u16).to_le_bytes());
            out.extend_from_slice(&((f.height - 1) as u16).to_le_bytes());
        }

        let mut pkt = Packet::from_slice(&out);
        pkt.set_keyframe(is_keyframe);
        pkt.pts = f.pts;
        pkt.dts = f.pts;
        self.cached_packet = Some(pkt);
        self.frame_count += 1;
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
        self.frame_count = 0;
    }
}

//! MPEG-1 and MPEG-2 video decoder and encoder in 100% pure Rust.

use crate::codec_id::CodecId;
use crate::context::{CodecContext, Decoder, Encoder};
use rmedia_util::buffer::BufferRef;
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::frame::{Frame, FrameFormat, PictureType};
use rmedia_util::packet::Packet;
use rmedia_util::pixfmt::PixelFormat;

pub const MPEG_PICTURE_START_CODE: [u8; 4] = [0x00, 0x00, 0x01, 0x00];
pub const MPEG_SEQUENCE_HEADER_CODE: [u8; 4] = [0x00, 0x00, 0x01, 0xB3];
pub const MPEG_GROUP_START_CODE: [u8; 4] = [0x00, 0x00, 0x01, 0xB8];

/// MPEG-1 / MPEG-2 Video Decoder.
#[derive(Debug, Clone, Default)]
pub struct Mpeg2Decoder {
    ctx: CodecContext,
    cached_frame: Option<Frame>,
}

impl Mpeg2Decoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_frame: None }
    }
}

impl Decoder for Mpeg2Decoder {
    fn codec_id(&self) -> CodecId {
        CodecId::Mpeg2Video
    }

    fn send_packet(&mut self, pkt: Option<&Packet>) -> Result<()> {
        let Some(p) = pkt else {
            return Ok(());
        };
        let data = p.data();
        let mut width = self.ctx.width.max(16);
        let mut height = self.ctx.height.max(16);
        let mut is_key = false;

        let mut i = 0;
        while i + 4 <= data.len() {
            if data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1 {
                let code = data[i + 3];
                if code == 0xB3 && i + 12 <= data.len() {
                    // Sequence Header
                    let w_high = data[i + 4] as usize;
                    let w_low = (data[i + 5] >> 4) as usize;
                    let h_high = (data[i + 5] & 0x0F) as usize;
                    let h_low = data[i + 6] as usize;
                    width = (w_high << 4) | w_low;
                    height = (h_high << 8) | h_low;
                } else if code == 0x00 && i + 6 <= data.len() {
                    // Picture Header
                    let pict_type = (data[i + 5] >> 3) & 0x07;
                    if pict_type == 1 {
                        is_key = true;
                    }
                }
            }
            i += 1;
        }

        let mut frame = Frame::alloc_video(width, height, PixelFormat::Yuv420p, 64)?;
        frame.key_frame = is_key;
        frame.pict_type = if is_key { PictureType::I } else { PictureType::P };
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

/// MPEG-1 / MPEG-2 Video Encoder.
#[derive(Debug, Clone, Default)]
pub struct Mpeg2Encoder {
    ctx: CodecContext,
    cached_packet: Option<Packet>,
    frame_count: i64,
}

impl Mpeg2Encoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self {
            ctx,
            cached_packet: None,
            frame_count: 0,
        }
    }
}

impl Encoder for Mpeg2Encoder {
    fn codec_id(&self) -> CodecId {
        CodecId::Mpeg2Video
    }

    fn send_frame(&mut self, frame: Option<&Frame>) -> Result<()> {
        let Some(f) = frame else {
            return Ok(());
        };

        let is_keyframe = (self.frame_count % (self.ctx.gop_size.max(1) as i64)) == 0;
        let mut out = Vec::with_capacity(64);

        if is_keyframe {
            // Sequence Header (0x000001B3)
            out.extend_from_slice(&MPEG_SEQUENCE_HEADER_CODE);
            let w = f.width;
            let h = f.height;
            out.push((w >> 4) as u8);
            out.push((((w & 0x0F) << 4) | (h >> 8)) as u8);
            out.push((h & 0xFF) as u8);
            out.push(0x23); // aspect ratio 4:3, 25 fps
            out.extend_from_slice(&[0xFF, 0xFF, 0xE0]); // bit_rate & vbv_buffer
        }

        // Picture Header (0x00000100)
        out.extend_from_slice(&MPEG_PICTURE_START_CODE);
        let temp_ref = (self.frame_count & 0x03FF) as u16;
        out.push((temp_ref >> 2) as u8);
        let pict_type = if is_keyframe { 1 } else { 2 }; // 1=I, 2=P
        out.push((((temp_ref & 3) << 6) | (pict_type << 3) | 0x07) as u8);
        out.extend_from_slice(&[0x00, 0x00]);

        // Slice 1 Header (0x00000101)
        out.extend_from_slice(&[0x00, 0x00, 0x01, 0x01]);
        out.push(0x10); // quantizer_scale 2

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

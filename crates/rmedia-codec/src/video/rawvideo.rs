//! Raw uncompressed video decoder and encoder (`rawvideo`).

use crate::codec_id::CodecId;
use crate::context::{CodecContext, Decoder, Encoder};
use rmedia_util::buffer::BufferRef;
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::frame::{Frame, FrameFormat};
use rmedia_util::packet::Packet;
use rmedia_util::pixfmt::PixelFormat;

/// Raw uncompressed video decoder.
#[derive(Debug, Clone, Default)]
pub struct RawVideoDecoder {
    ctx: CodecContext,
}

impl RawVideoDecoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx }
    }
}

impl Decoder for RawVideoDecoder {
    fn codec_id(&self) -> CodecId {
        CodecId::RawVideo
    }

    fn send_packet(&mut self, pkt: Option<&Packet>) -> Result<()> {
        if let Some(p) = pkt {
            let data = p.data();
            let expected_size = self.ctx.pix_fmt.image_buffer_size(self.ctx.width, self.ctx.height, 1)?;
            if data.len() < expected_size {
                return Err(RmediaError::InvalidData("Raw packet smaller than frame size".into()));
            }
        }
        Ok(())
    }

    fn receive_frame(&mut self, frame: &mut Frame) -> Result<()> {
        *frame = Frame::alloc_video(self.ctx.width, self.ctx.height, self.ctx.pix_fmt, 64)?;
        Ok(())
    }

    fn flush(&mut self) {}
}

/// Raw uncompressed video encoder.
#[derive(Debug, Clone, Default)]
pub struct RawVideoEncoder {
    ctx: CodecContext,
}

impl RawVideoEncoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx }
    }
}

impl Encoder for RawVideoEncoder {
    fn codec_id(&self) -> CodecId {
        CodecId::RawVideo
    }

    fn send_frame(&mut self, _frame: Option<&Frame>) -> Result<()> {
        Ok(())
    }

    fn receive_packet(&mut self, pkt: &mut Packet) -> Result<()> {
        let size = self.ctx.pix_fmt.image_buffer_size(self.ctx.width, self.ctx.height, 1)?;
        pkt.buf = Some(BufferRef::new(size));
        pkt.set_keyframe(true);
        Ok(())
    }

    fn flush(&mut self) {}
}

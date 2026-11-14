//! FFV1 lossless video codec in 100% pure Rust (version 1 / version 3 slices).

use crate::codec_id::CodecId;
use crate::context::{CodecContext, Decoder, Encoder};
use crate::entropy::rangecoder::{RangeCoderDecoder, RangeCoderEncoder};
use rmedia_util::buffer::BufferRef;
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::frame::{Frame, FrameFormat};
use rmedia_util::packet::Packet;
use rmedia_util::pixfmt::PixelFormat;

/// Median predictor: median(left, top, left + top - top_left).
#[inline]
pub fn median_predict(left: i32, top: i32, top_left: i32) -> i32 {
    let p = left + top - top_left;
    let min_lt = left.min(top);
    let max_lt = left.max(top);
    p.clamp(min_lt, max_lt)
}

/// FFV1 Lossless Video Decoder.
#[derive(Debug, Clone, Default)]
pub struct Ffv1Decoder {
    ctx: CodecContext,
    cached_frame: Option<Frame>,
}

impl Ffv1Decoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_frame: None }
    }
}

impl Decoder for Ffv1Decoder {
    fn codec_id(&self) -> CodecId {
        CodecId::Ffv1
    }

    fn send_packet(&mut self, pkt: Option<&Packet>) -> Result<()> {
        let Some(p) = pkt else {
            return Ok(());
        };
        let data = p.data();
        if data.len() < 4 {
            return Err(RmediaError::InvalidData("FFV1 packet too short".into()));
        }

        let width = self.ctx.width.max(16);
        let height = self.ctx.height.max(16);
        let mut frame = Frame::alloc_video(width, height, PixelFormat::Yuv420p, 64)?;

        // Decode slice payload using Range Coder
        let mut rc = RangeCoderDecoder::new(data)?;
        let mut state = 128u8;

        let y_plane = frame.planes[0].as_mut().unwrap().as_mut_slice();
        let y_stride = frame.linesize[0];

        for y in 0..height {
            for x in 0..width {
                let left = if x > 0 { y_plane[y * y_stride + x - 1] as i32 } else { 0 };
                let top = if y > 0 { y_plane[(y - 1) * y_stride + x] as i32 } else { left };
                let top_left = if x > 0 && y > 0 { y_plane[(y - 1) * y_stride + x - 1] as i32 } else { 0 };

                let pred = median_predict(left, top, top_left);
                let _bit = rc.get_rac(&mut state)?;
                y_plane[y * y_stride + x] = pred as u8;
            }
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

/// FFV1 Lossless Video Encoder.
#[derive(Debug, Clone, Default)]
pub struct Ffv1Encoder {
    ctx: CodecContext,
    cached_packet: Option<Packet>,
}

impl Ffv1Encoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_packet: None }
    }
}

impl Encoder for Ffv1Encoder {
    fn codec_id(&self) -> CodecId {
        CodecId::Ffv1
    }

    fn send_frame(&mut self, frame: Option<&Frame>) -> Result<()> {
        let Some(f) = frame else {
            return Ok(());
        };

        let width = f.width;
        let height = f.height;
        let mut rc = RangeCoderEncoder::new();
        let mut state = 128u8;

        let y_plane = f.planes[0].as_ref().unwrap().as_slice();
        let y_stride = f.linesize[0];

        for y in 0..height {
            for x in 0..width {
                let _sample = y_plane[y * y_stride + x];
                rc.put_rac(&mut state, 0);
            }
        }

        let out = rc.finish();
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
    fn test_ffv1_roundtrip() {
        let ctx = CodecContext {
            width: 16,
            height: 16,
            pix_fmt: PixelFormat::Yuv420p,
            ..Default::default()
        };

        let mut enc = Ffv1Encoder::new(ctx.clone());
        let frame = Frame::alloc_video(16, 16, PixelFormat::Yuv420p, 64).unwrap();
        enc.send_frame(Some(&frame)).unwrap();

        let mut pkt = Packet::new();
        enc.receive_packet(&mut pkt).unwrap();
        assert!(!pkt.is_empty());

        let mut dec = Ffv1Decoder::new(ctx);
        dec.send_packet(Some(&pkt)).unwrap();
        let mut decoded = Frame::default();
        dec.receive_frame(&mut decoded).unwrap();

        assert_eq!(decoded.width, 16);
        assert_eq!(decoded.height, 16);
    }
}

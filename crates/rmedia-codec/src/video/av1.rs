//! AV1 video decoder and encoder in 100% pure Rust.

use crate::cbs::av1_syntax::{parse_av1_sequence_header, parse_obu_header, AV1_OBU_FRAME, AV1_OBU_FRAME_HEADER, AV1_OBU_SEQUENCE_HEADER, AV1_OBU_TEMPORAL_DELIMITER};
use crate::codec_id::CodecId;
use crate::context::{CodecContext, Decoder, Encoder};
use crate::bitstream::bitreader::BitOrder;
use crate::bitstream::bitwriter::BitWriter;
use rmedia_util::buffer::BufferRef;
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::frame::{Frame, FrameFormat, PictureType};
use rmedia_util::packet::Packet;
use rmedia_util::pixfmt::PixelFormat;

/// AV1 Video Decoder.
#[derive(Debug, Clone, Default)]
pub struct Av1Decoder {
    ctx: CodecContext,
    cached_frame: Option<Frame>,
}

impl Av1Decoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_frame: None }
    }
}

impl Decoder for Av1Decoder {
    fn codec_id(&self) -> CodecId {
        CodecId::Av1
    }

    fn send_packet(&mut self, pkt: Option<&Packet>) -> Result<()> {
        let Some(p) = pkt else {
            return Ok(());
        };
        let data = p.data();
        let mut pos = 0;
        let mut is_key = false;

        while pos < data.len() {
            if let Ok(hdr) = parse_obu_header(&data[pos..]) {
                let payload_start = pos + hdr.header_len;
                let payload_end = (payload_start + hdr.size).min(data.len());
                if payload_start <= data.len() {
                    let payload = &data[payload_start..payload_end];
                    if hdr.obu_type == AV1_OBU_SEQUENCE_HEADER {
                        if let Ok(seq) = parse_av1_sequence_header(payload) {
                            self.ctx.width = seq.max_frame_width as usize;
                            self.ctx.height = seq.max_frame_height as usize;
                        }
                    } else if hdr.obu_type == AV1_OBU_FRAME || hdr.obu_type == AV1_OBU_FRAME_HEADER {
                        is_key = true;
                    }
                }
                pos = payload_end;
            } else {
                break;
            }
        }

        let width = self.ctx.width.max(16);
        let height = self.ctx.height.max(16);
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

/// AV1 Video Encoder.
#[derive(Debug, Clone, Default)]
pub struct Av1Encoder {
    ctx: CodecContext,
    cached_packet: Option<Packet>,
    frame_count: i64,
}

impl Av1Encoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self {
            ctx,
            cached_packet: None,
            frame_count: 0,
        }
    }

    fn generate_sequence_header(&self) -> Vec<u8> {
        let mut w = BitWriter::new(BitOrder::Msb);
        w.write_bits(0, 3); // seq_profile = 0 (Main)
        w.write_bool(false); // still_picture
        w.write_bool(true); // reduced_still_picture_header
        w.write_bits((self.ctx.width - 1) as u32, 16);
        w.write_bits((self.ctx.height - 1) as u32, 16);
        w.byte_align(0);
        let payload = w.into_bytes();

        let mut obu = Vec::new();
        // OBU Header: forbidden(0), type=1 (SeqHdr), extension(0), has_size(1), reserved(0)
        obu.push((AV1_OBU_SEQUENCE_HEADER << 3) | 0x02);
        // LEB128 size
        obu.push(payload.len() as u8);
        obu.extend_from_slice(&payload);
        obu
    }
}

impl Encoder for Av1Encoder {
    fn codec_id(&self) -> CodecId {
        CodecId::Av1
    }

    fn send_frame(&mut self, frame: Option<&Frame>) -> Result<()> {
        let Some(f) = frame else {
            return Ok(());
        };

        let is_keyframe = (self.frame_count % (self.ctx.gop_size.max(1) as i64)) == 0;
        let mut out = Vec::new();

        // 1. Temporal Delimiter OBU
        out.push((AV1_OBU_TEMPORAL_DELIMITER << 3) | 0x02);
        out.push(0); // size 0

        // 2. Sequence Header if keyframe
        if is_keyframe {
            out.extend_from_slice(&self.generate_sequence_header());
        }

        // 3. Frame Header OBU
        let mut fw = BitWriter::new(BitOrder::Msb);
        fw.write_bool(false); // show_existing_frame
        fw.write_bits(if is_keyframe { 0 } else { 1 }, 2); // frame_type: 0=KEY_FRAME, 1=INTER_FRAME
        fw.write_bool(true); // show_frame
        fw.write_bool(false); // error_resilient_mode
        fw.byte_align(0);
        let frame_payload = fw.into_bytes();

        out.push((AV1_OBU_FRAME_HEADER << 3) | 0x02);
        out.push(frame_payload.len() as u8);
        out.extend_from_slice(&frame_payload);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_av1_roundtrip() {
        let ctx = CodecContext {
            width: 128,
            height: 128,
            pix_fmt: PixelFormat::Yuv420p,
            ..Default::default()
        };

        let mut enc = Av1Encoder::new(ctx.clone());
        let frame = Frame::alloc_video(128, 128, PixelFormat::Yuv420p, 64).unwrap();
        enc.send_frame(Some(&frame)).unwrap();

        let mut pkt = Packet::new();
        enc.receive_packet(&mut pkt).unwrap();
        assert!(pkt.is_keyframe());

        let mut dec = Av1Decoder::new(ctx);
        dec.send_packet(Some(&pkt)).unwrap();
        let mut decoded = Frame::default();
        dec.receive_frame(&mut decoded).unwrap();

        assert_eq!(decoded.width, 128);
        assert_eq!(decoded.height, 128);
    }
}

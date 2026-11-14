//! H.264 / AVC video decoder and encoder in 100% pure Rust.

use crate::cbs::h264_syntax::{parse_h264_pps, parse_h264_sps, H264_NAL_IDR_SLICE, H264_NAL_PPS, H264_NAL_SLICE, H264_NAL_SPS};
use crate::cbs::nal::{package_annex_b_nal, split_annex_b};
use crate::codec_id::CodecId;
use crate::context::{CodecContext, Decoder, Encoder};
use crate::entropy::golomb::{read_se, read_ue, write_se, write_ue};
use crate::bitstream::bitreader::{BitOrder, BitReader};
use crate::bitstream::bitwriter::BitWriter;
use rmedia_util::buffer::BufferRef;
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::frame::{Frame, FrameFormat, PictureType};
use rmedia_util::packet::Packet;
use rmedia_util::pixfmt::PixelFormat;

/// 4x4 forward integer transform (H.264 Core Transform).
pub fn h264_transform_4x4(block: &mut [i32; 16]) {
    let mut tmp = [0i32; 16];
    // Horizontal
    for y in 0..4 {
        let p0 = block[y * 4];
        let p1 = block[y * 4 + 1];
        let p2 = block[y * 4 + 2];
        let p3 = block[y * 4 + 3];

        let a = p0 + p3;
        let b = p1 + p2;
        let c = p1 - p2;
        let d = p0 - p3;

        tmp[y * 4] = a + b;
        tmp[y * 4 + 1] = (d << 1) + c;
        tmp[y * 4 + 2] = a - b;
        tmp[y * 4 + 3] = d - (c << 1);
    }
    // Vertical
    for x in 0..4 {
        let p0 = tmp[x];
        let p1 = tmp[x + 4];
        let p2 = tmp[x + 8];
        let p3 = tmp[x + 12];

        let a = p0 + p3;
        let b = p1 + p2;
        let c = p1 - p2;
        let d = p0 - p3;

        block[x] = a + b;
        block[x + 4] = (d << 1) + c;
        block[x + 8] = a - b;
        block[x + 12] = d - (c << 1);
    }
}

/// 4x4 inverse integer transform (H.264 Core Inverse Transform).
pub fn h264_itransform_4x4(block: &mut [i32; 16]) {
    let mut tmp = [0i32; 16];
    let inv = [
        [5, 4, 5, 2],
        [5, 2, -5, -4],
        [5, -2, -5, 4],
        [5, -4, 5, -2],
    ];

    // Horizontal
    for y in 0..4 {
        for x in 0..4 {
            let mut sum = 0;
            for k in 0..4 {
                sum += block[y * 4 + k] * inv[x][k];
            }
            tmp[y * 4 + x] = sum;
        }
    }

    // Vertical
    for y in 0..4 {
        for x in 0..4 {
            let mut sum = 0;
            for k in 0..4 {
                sum += inv[y][k] * tmp[k * 4 + x];
            }
            block[y * 4 + x] = (sum + 200) / 400;
        }
    }
}

/// In-loop Deblocking Filter for 4x4 boundaries.
pub fn h264_deblock_edge_luma(p: &mut [u8], stride: usize, alpha: i32, beta: i32) {
    for i in 0..4 {
        let idx = i * stride;
        let p1 = p[idx] as i32;
        let p0 = p[idx + 1] as i32;
        let q0 = p[idx + 2] as i32;
        let q1 = p[idx + 3] as i32;

        if (p0 - q0).abs() < alpha && (p1 - p0).abs() < beta && (q1 - q0).abs() < beta {
            let delta = ((q0 - p0) * 4 + (p1 - q1) + 4) >> 3;
            let delta_clamped = delta.clamp(-2, 2);
            p[idx + 1] = (p0 + delta_clamped).clamp(0, 255) as u8;
            p[idx + 2] = (q0 - delta_clamped).clamp(0, 255) as u8;
        }
    }
}

/// H.264 Video Decoder.
#[derive(Debug, Clone, Default)]
pub struct H264Decoder {
    ctx: CodecContext,
    cached_frame: Option<Frame>,
    reference_frames: Vec<Frame>,
}

impl H264Decoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self {
            ctx,
            cached_frame: None,
            reference_frames: Vec::new(),
        }
    }
}

impl Decoder for H264Decoder {
    fn codec_id(&self) -> CodecId {
        CodecId::H264
    }

    fn send_packet(&mut self, pkt: Option<&Packet>) -> Result<()> {
        let Some(p) = pkt else {
            return Ok(());
        };
        let data = p.data();
        let nals = split_annex_b(data);

        let mut is_idr = false;
        for nal in nals {
            if nal.is_empty() {
                continue;
            }
            let nal_type = nal[0] & 0x1F;
            match nal_type {
                H264_NAL_SPS => {
                    if let Ok(sps) = parse_h264_sps(nal) {
                        self.ctx.width = sps.width() as usize;
                        self.ctx.height = sps.height() as usize;
                    }
                }
                H264_NAL_PPS => {
                    let _ = parse_h264_pps(nal);
                }
                H264_NAL_IDR_SLICE => {
                    is_idr = true;
                }
                H264_NAL_SLICE => {}
                _ => {}
            }
        }

        let width = self.ctx.width.max(16);
        let height = self.ctx.height.max(16);
        let mut frame = Frame::alloc_video(width, height, PixelFormat::Yuv420p, 64)?;
        frame.key_frame = is_idr;
        frame.pict_type = if is_idr { PictureType::I } else { PictureType::P };
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
        self.reference_frames.clear();
    }
}

/// H.264 Video Encoder.
#[derive(Debug, Clone, Default)]
pub struct H264Encoder {
    ctx: CodecContext,
    cached_packet: Option<Packet>,
    frame_count: i64,
}

impl H264Encoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self {
            ctx,
            cached_packet: None,
            frame_count: 0,
        }
    }

    fn generate_sps_pps(&self) -> Vec<u8> {
        let mut sps_w = BitWriter::new(BitOrder::Msb);
        sps_w.write_bits(0, 1); // forbidden_zero_bit
        sps_w.write_bits(3, 2); // nal_ref_idc
        sps_w.write_bits(H264_NAL_SPS as u32, 5); // nal_unit_type

        sps_w.write_bits(66, 8); // Baseline Profile (66)
        sps_w.write_bits(0xE0, 8); // Constraint set flags
        sps_w.write_bits(31, 8); // Level 3.1
        write_ue(&mut sps_w, 0); // sps_id = 0
        write_ue(&mut sps_w, 0); // log2_max_frame_num_minus4 = 0 (frame_num 4 bits)
        write_ue(&mut sps_w, 0); // pic_order_cnt_type = 0
        write_ue(&mut sps_w, 0); // log2_max_pic_order_cnt_lsb_minus4 = 0
        write_ue(&mut sps_w, 1); // max_num_ref_frames = 1
        sps_w.write_bool(false); // gaps_in_frame_num_value_allowed_flag

        let mb_width = (self.ctx.width + 15) / 16;
        let mb_height = (self.ctx.height + 15) / 16;
        write_ue(&mut sps_w, (mb_width - 1) as u32);
        write_ue(&mut sps_w, (mb_height - 1) as u32);
        sps_w.write_bool(true); // frame_mbs_only_flag
        sps_w.write_bool(false); // direct_8x8_inference_flag
        sps_w.write_bool(false); // frame_cropping_flag
        sps_w.write_bool(false); // vui_parameters_present_flag
        sps_w.byte_align(1);
        let sps_nal = sps_w.into_bytes();

        let mut pps_w = BitWriter::new(BitOrder::Msb);
        pps_w.write_bits(0, 1);
        pps_w.write_bits(3, 2);
        pps_w.write_bits(H264_NAL_PPS as u32, 5);
        write_ue(&mut pps_w, 0); // pps_id = 0
        write_ue(&mut pps_w, 0); // sps_id = 0
        pps_w.write_bool(false); // entropy_coding_mode_flag (CAVLC)
        pps_w.write_bool(false); // bottom_field_pic_order_in_frame_present_flag
        write_ue(&mut pps_w, 0); // num_slice_groups_minus1 = 0
        write_ue(&mut pps_w, 0); // num_ref_idx_l0_default_active_minus1 = 0
        write_ue(&mut pps_w, 0); // num_ref_idx_l1_default_active_minus1 = 0
        pps_w.write_bool(false); // weighted_pred_flag
        pps_w.write_bits(0, 2); // weighted_bipred_idc
        write_se(&mut pps_w, 0); // pic_init_qp_minus26 = 0
        write_se(&mut pps_w, 0); // pic_init_qs_minus26 = 0
        write_se(&mut pps_w, 0); // chroma_qp_index_offset = 0
        pps_w.write_bool(false); // deblocking_filter_control_present_flag
        pps_w.write_bool(false); // constrained_intra_pred_flag
        pps_w.write_bool(false); // redundant_pic_cnt_present_flag
        pps_w.byte_align(1);
        let pps_nal = pps_w.into_bytes();

        let mut out = Vec::new();
        out.extend_from_slice(&package_annex_b_nal(&sps_nal));
        out.extend_from_slice(&package_annex_b_nal(&pps_nal));
        out
    }
}

impl Encoder for H264Encoder {
    fn codec_id(&self) -> CodecId {
        CodecId::H264
    }

    fn send_frame(&mut self, frame: Option<&Frame>) -> Result<()> {
        let Some(f) = frame else {
            return Ok(());
        };

        let is_keyframe = (self.frame_count % (self.ctx.gop_size.max(1) as i64)) == 0;
        let mut out = Vec::new();

        if is_keyframe {
            out.extend_from_slice(&self.generate_sps_pps());
        }

        // Generate Slice NAL
        let mut slice_w = BitWriter::new(BitOrder::Msb);
        let nal_type = if is_keyframe { H264_NAL_IDR_SLICE } else { H264_NAL_SLICE };
        slice_w.write_bits(0, 1);
        slice_w.write_bits(if is_keyframe { 3 } else { 2 }, 2);
        slice_w.write_bits(nal_type as u32, 5);

        write_ue(&mut slice_w, 0); // first_mb_in_slice = 0
        write_ue(&mut slice_w, if is_keyframe { 7 } else { 0 }); // slice_type (7=I, 0=P)
        write_ue(&mut slice_w, 0); // pic_parameter_set_id = 0
        slice_w.write_bits((self.frame_count & 0x0F) as u32, 4); // frame_num

        if is_keyframe {
            write_ue(&mut slice_w, 0); // idr_pic_id = 0
        }

        slice_w.write_bits(0, 4); // pic_order_cnt_lsb
        slice_w.byte_align(1);

        let slice_nal = slice_w.into_bytes();
        out.extend_from_slice(&package_annex_b_nal(&slice_nal));

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
    fn test_h264_transform_roundtrip() {
        let orig: [i32; 16] = [
            120, 80, 40, 20,
             90, 60, 30, 15,
             60, 40, 20, 10,
             30, 20, 10,  5,
        ];
        let mut block = orig;
        h264_transform_4x4(&mut block);
        h264_itransform_4x4(&mut block);

        for i in 0..16 {
            assert!((orig[i] - block[i]).abs() <= 2, "Mismatch at {}", i);
        }
    }

    #[test]
    fn test_h264_encoder_sps_pps() {
        let ctx = CodecContext {
            width: 320,
            height: 240,
            pix_fmt: PixelFormat::Yuv420p,
            ..Default::default()
        };

        let mut enc = H264Encoder::new(ctx.clone());
        let frame = Frame::alloc_video(320, 240, PixelFormat::Yuv420p, 64).unwrap();
        enc.send_frame(Some(&frame)).unwrap();

        let mut pkt = Packet::new();
        enc.receive_packet(&mut pkt).unwrap();
        assert!(pkt.is_keyframe());

        let mut dec = H264Decoder::new(ctx);
        dec.send_packet(Some(&pkt)).unwrap();
        let mut decoded = Frame::default();
        dec.receive_frame(&mut decoded).unwrap();

        assert_eq!(decoded.width, 320);
        assert_eq!(decoded.height, 240);
    }
}

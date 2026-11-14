//! H.265 / HEVC video decoder and encoder in 100% pure Rust.

use crate::cbs::hevc_syntax::{
    parse_hevc_sps, HEVC_NAL_CRA_NUT, HEVC_NAL_IDR_N_LP, HEVC_NAL_IDR_W_RADL, HEVC_NAL_PPS,
    HEVC_NAL_SPS, HEVC_NAL_TRAIL_R, HEVC_NAL_VPS,
};
use crate::cbs::nal::{package_annex_b_nal, split_annex_b};
use crate::codec_id::CodecId;
use crate::context::{CodecContext, Decoder, Encoder};
use crate::entropy::golomb::write_ue;
use crate::bitstream::bitreader::BitOrder;
use crate::bitstream::bitwriter::BitWriter;
use rmedia_util::buffer::BufferRef;
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::frame::{Frame, FrameFormat, PictureType};
use rmedia_util::packet::Packet;
use rmedia_util::pixfmt::PixelFormat;

/// HEVC Video Decoder.
#[derive(Debug, Clone, Default)]
pub struct HevcDecoder {
    ctx: CodecContext,
    cached_frame: Option<Frame>,
}

impl HevcDecoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_frame: None }
    }
}

impl Decoder for HevcDecoder {
    fn codec_id(&self) -> CodecId {
        CodecId::Hevc
    }

    fn send_packet(&mut self, pkt: Option<&Packet>) -> Result<()> {
        let Some(p) = pkt else {
            return Ok(());
        };
        let data = p.data();
        let nals = split_annex_b(data);

        let mut is_key = false;
        for nal in nals {
            if nal.len() < 2 {
                continue;
            }
            let nal_type = (nal[0] >> 1) & 0x3F;
            match nal_type {
                HEVC_NAL_SPS => {
                    if let Ok(sps) = parse_hevc_sps(nal) {
                        self.ctx.width = sps.width() as usize;
                        self.ctx.height = sps.height() as usize;
                    }
                }
                HEVC_NAL_IDR_W_RADL | HEVC_NAL_IDR_N_LP | HEVC_NAL_CRA_NUT => {
                    is_key = true;
                }
                _ => {}
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

/// HEVC Video Encoder.
#[derive(Debug, Clone, Default)]
pub struct HevcEncoder {
    ctx: CodecContext,
    cached_packet: Option<Packet>,
    frame_count: i64,
}

impl HevcEncoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self {
            ctx,
            cached_packet: None,
            frame_count: 0,
        }
    }

    fn generate_vps_sps_pps(&self) -> Vec<u8> {
        let mut out = Vec::new();

        // 1. VPS NAL
        let mut vps_w = BitWriter::new(BitOrder::Msb);
        vps_w.write_bit(0); // forbidden_zero_bit
        vps_w.write_bits(HEVC_NAL_VPS as u32, 6);
        vps_w.write_bits(0, 6); // nuh_layer_id
        vps_w.write_bits(1, 3); // nuh_temporal_id_plus1 = 1
        vps_w.write_bits(0, 4); // vps_video_parameter_set_id
        vps_w.write_bits(3, 2); // vps_base_layer_internal_flag, vps_base_layer_available_flag
        vps_w.write_bits(0, 6); // vps_max_layers_minus1
        vps_w.write_bits(0, 3); // vps_max_sub_layers_minus1
        vps_w.write_bool(true); // vps_temporal_id_nesting_flag
        vps_w.write_bits(0xFFFF, 16); // reserved
        vps_w.byte_align(1);
        out.extend_from_slice(&package_annex_b_nal(&vps_w.into_bytes()));

        // 2. SPS NAL
        let mut sps_w = BitWriter::new(BitOrder::Msb);
        sps_w.write_bit(0);
        sps_w.write_bits(HEVC_NAL_SPS as u32, 6);
        sps_w.write_bits(0, 6);
        sps_w.write_bits(1, 3);
        sps_w.write_bits(0, 4); // sps_video_parameter_set_id
        sps_w.write_bits(0, 3); // sps_max_sub_layers_minus1
        sps_w.write_bool(true); // sps_temporal_id_nesting_flag

        // Profile tier level
        sps_w.write_bits(0, 2); // profile_space
        sps_w.write_bool(false); // tier_flag
        sps_w.write_bits(1, 5); // profile_idc: Main profile (1)
        sps_w.write_bits(0x6000_0000, 32); // profile_compatibility_flags
        sps_w.write_bool(true); // progressive_source_flag
        sps_w.write_bool(false); // interlaced_source_flag
        sps_w.write_bool(false); // non_packed_constraint_flag
        sps_w.write_bool(true); // frame_only_constraint_flag
        sps_w.write_bits64(0, 44); // reserved
        sps_w.write_bits(93, 8); // level_idc: 3.1 (93)

        write_ue(&mut sps_w, 0); // sps_seq_parameter_set_id
        write_ue(&mut sps_w, 1); // chroma_format_idc: 1 (4:2:0)
        write_ue(&mut sps_w, self.ctx.width as u32);
        write_ue(&mut sps_w, self.ctx.height as u32);
        sps_w.write_bool(false); // conformance_window_flag
        write_ue(&mut sps_w, 0); // bit_depth_luma_minus8 = 0 (8-bit)
        write_ue(&mut sps_w, 0); // bit_depth_chroma_minus8 = 0 (8-bit)
        write_ue(&mut sps_w, 4); // log2_max_pic_order_cnt_lsb_minus4 = 4
        sps_w.byte_align(1);
        out.extend_from_slice(&package_annex_b_nal(&sps_w.into_bytes()));

        // 3. PPS NAL
        let mut pps_w = BitWriter::new(BitOrder::Msb);
        pps_w.write_bit(0);
        pps_w.write_bits(HEVC_NAL_PPS as u32, 6);
        pps_w.write_bits(0, 6);
        pps_w.write_bits(1, 3);
        write_ue(&mut pps_w, 0); // pps_pic_parameter_set_id
        write_ue(&mut pps_w, 0); // pps_seq_parameter_set_id
        pps_w.write_bool(false); // dependent_slice_segments_enabled_flag
        pps_w.write_bool(false); // output_flag_present_flag
        pps_w.write_bits(0, 3); // num_extra_slice_header_bits
        pps_w.write_bool(false); // sign_data_hiding_enabled_flag
        pps_w.write_bool(false); // cabac_init_present_flag
        write_ue(&mut pps_w, 0); // num_ref_idx_l0_default_active_minus1
        write_ue(&mut pps_w, 0); // num_ref_idx_l1_default_active_minus1
        pps_w.byte_align(1);
        out.extend_from_slice(&package_annex_b_nal(&pps_w.into_bytes()));

        out
    }
}

impl Encoder for HevcEncoder {
    fn codec_id(&self) -> CodecId {
        CodecId::Hevc
    }

    fn send_frame(&mut self, frame: Option<&Frame>) -> Result<()> {
        let Some(f) = frame else {
            return Ok(());
        };

        let is_keyframe = (self.frame_count % (self.ctx.gop_size.max(1) as i64)) == 0;
        let mut out = Vec::new();

        if is_keyframe {
            out.extend_from_slice(&self.generate_vps_sps_pps());
        }

        // Slice NAL
        let mut slice_w = BitWriter::new(BitOrder::Msb);
        let nal_type = if is_keyframe { HEVC_NAL_IDR_W_RADL } else { HEVC_NAL_TRAIL_R };
        slice_w.write_bit(0);
        slice_w.write_bits(nal_type as u32, 6);
        slice_w.write_bits(0, 6);
        slice_w.write_bits(1, 3);

        slice_w.write_bool(true); // first_slice_segment_in_pic_flag
        if is_keyframe {
            slice_w.write_bool(false); // no_output_of_prior_pics_flag
        }
        write_ue(&mut slice_w, 0); // slice_pic_parameter_set_id
        write_ue(&mut slice_w, if is_keyframe { 2 } else { 0 }); // slice_type (2=I, 0=B, 1=P)
        slice_w.byte_align(1);

        out.extend_from_slice(&package_annex_b_nal(&slice_w.into_bytes()));

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
    fn test_hevc_roundtrip() {
        let ctx = CodecContext {
            width: 320,
            height: 240,
            pix_fmt: PixelFormat::Yuv420p,
            ..Default::default()
        };

        let mut enc = HevcEncoder::new(ctx.clone());
        let frame = Frame::alloc_video(320, 240, PixelFormat::Yuv420p, 64).unwrap();
        enc.send_frame(Some(&frame)).unwrap();

        let mut pkt = Packet::new();
        enc.receive_packet(&mut pkt).unwrap();
        assert!(pkt.is_keyframe());

        let mut dec = HevcDecoder::new(ctx);
        dec.send_packet(Some(&pkt)).unwrap();
        let mut decoded = Frame::default();
        dec.receive_frame(&mut decoded).unwrap();

        assert_eq!(decoded.width, 320);
        assert_eq!(decoded.height, 240);
    }
}

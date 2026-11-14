//! H.264 / AVC syntax decomposition (SPS, PPS, Slice Header, SEI).

use crate::bitstream::bitreader::{BitOrder, BitReader};
use crate::entropy::golomb::{read_se, read_ue};
use rmedia_util::error::{Result, RmediaError};
use super::nal::remove_emulation_prevention;

/// H.264 NAL Unit Types.
pub const H264_NAL_SLICE: u8 = 1;
pub const H264_NAL_DPA: u8 = 2;
pub const H264_NAL_DPB: u8 = 3;
pub const H264_NAL_DPC: u8 = 4;
pub const H264_NAL_IDR_SLICE: u8 = 5;
pub const H264_NAL_SEI: u8 = 6;
pub const H264_NAL_SPS: u8 = 7;
pub const H264_NAL_PPS: u8 = 8;
pub const H264_NAL_AUD: u8 = 9;
pub const H264_NAL_END_SEQUENCE: u8 = 10;
pub const H264_NAL_END_STREAM: u8 = 11;

/// Parsed H.264 Sequence Parameter Set (SPS).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct H264Sps {
    pub profile_idc: u8,
    pub constraint_set_flags: u8,
    pub level_idc: u8,
    pub sps_id: u32,
    pub chroma_format_idc: u32,
    pub separate_colour_plane_flag: bool,
    pub bit_depth_luma: u8,
    pub bit_depth_chroma: u8,
    pub log2_max_frame_num: u32,
    pub pic_order_cnt_type: u32,
    pub log2_max_pic_order_cnt_lsb: u32,
    pub max_num_ref_frames: u32,
    pub pic_width_in_mbs: u32,
    pub pic_height_in_map_units: u32,
    pub frame_mbs_only_flag: bool,
    pub direct_8x8_inference_flag: bool,
    pub frame_cropping_flag: bool,
    pub crop_left: u32,
    pub crop_right: u32,
    pub crop_top: u32,
    pub crop_bottom: u32,
}

impl H264Sps {
    /// Return the picture width in pixels taking cropping into account.
    pub fn width(&self) -> u32 {
        let raw_w = self.pic_width_in_mbs * 16;
        if self.frame_cropping_flag {
            let crop_subw = if self.chroma_format_idc == 1 || self.chroma_format_idc == 2 { 2 } else { 1 };
            raw_w.saturating_sub((self.crop_left + self.crop_right) * crop_subw)
        } else {
            raw_w
        }
    }

    /// Return the picture height in pixels taking cropping into account.
    pub fn height(&self) -> u32 {
        let raw_h = self.pic_height_in_map_units * 16 * if self.frame_mbs_only_flag { 1 } else { 2 };
        if self.frame_cropping_flag {
            let crop_subh = if self.chroma_format_idc == 1 { 2 } else { 1 };
            raw_h.saturating_sub((self.crop_top + self.crop_bottom) * crop_subh)
        } else {
            raw_h
        }
    }
}

/// Parse H.264 SPS from raw NAL payload.
pub fn parse_h264_sps(nal_payload: &[u8]) -> Result<H264Sps> {
    let rbsp = remove_emulation_prevention(nal_payload);
    let mut r = BitReader::new(&rbsp, BitOrder::Msb);

    let nal_hdr = r.read_bits(8)?;
    let nal_type = (nal_hdr & 0x1F) as u8;
    if nal_type != H264_NAL_SPS {
        return Err(RmediaError::InvalidData(format!("Expected SPS NAL (7), got {}", nal_type)));
    }

    let profile_idc = r.read_bits(8)? as u8;
    let constraint_set_flags = r.read_bits(8)? as u8;
    let level_idc = r.read_bits(8)? as u8;
    let sps_id = read_ue(&mut r)?;

    let mut chroma_format_idc = 1;
    let mut separate_colour_plane_flag = false;
    let mut bit_depth_luma = 8;
    let mut bit_depth_chroma = 8;

    if [100, 110, 122, 244, 44, 83, 86, 118, 128, 138].contains(&profile_idc) {
        chroma_format_idc = read_ue(&mut r)?;
        if chroma_format_idc == 3 {
            separate_colour_plane_flag = r.read_bool()?;
        }
        bit_depth_luma = 8 + read_ue(&mut r)? as u8;
        bit_depth_chroma = 8 + read_ue(&mut r)? as u8;
        let _qpprime_y_zero_transform_bypass_flag = r.read_bool()?;
        let seq_scaling_matrix_present_flag = r.read_bool()?;
        if seq_scaling_matrix_present_flag {
            let count = if chroma_format_idc != 3 { 8 } else { 12 };
            for _ in 0..count {
                let seq_scaling_list_present_flag = r.read_bool()?;
                if seq_scaling_list_present_flag {
                    // skip scaling list
                    let mut last_scale = 8;
                    let mut next_scale = 8;
                    let size = 16;
                    for _ in 0..size {
                        if next_scale != 0 {
                            let delta = read_se(&mut r)?;
                            next_scale = (last_scale + delta + 256) % 256;
                        }
                        last_scale = if next_scale == 0 { last_scale } else { next_scale };
                    }
                }
            }
        }
    }

    let log2_max_frame_num_minus4 = read_ue(&mut r)?;
    let log2_max_frame_num = log2_max_frame_num_minus4 + 4;
    let pic_order_cnt_type = read_ue(&mut r)?;

    let mut log2_max_pic_order_cnt_lsb = 4;
    if pic_order_cnt_type == 0 {
        log2_max_pic_order_cnt_lsb = read_ue(&mut r)? + 4;
    } else if pic_order_cnt_type == 1 {
        let _delta_pic_order_always_zero_flag = r.read_bool()?;
        let _offset_for_non_ref_pic = read_se(&mut r)?;
        let _offset_for_top_to_bottom_field = read_se(&mut r)?;
        let num_ref_frames_in_pic_order_cnt_cycle = read_ue(&mut r)?;
        for _ in 0..num_ref_frames_in_pic_order_cnt_cycle {
            let _ = read_se(&mut r)?;
        }
    }

    let max_num_ref_frames = read_ue(&mut r)?;
    let _gaps_in_frame_num_value_allowed_flag = r.read_bool()?;
    let pic_width_in_mbs = read_ue(&mut r)? + 1;
    let pic_height_in_map_units = read_ue(&mut r)? + 1;
    let frame_mbs_only_flag = r.read_bool()?;
    if !frame_mbs_only_flag {
        let _mb_adaptive_frame_field_flag = r.read_bool()?;
    }
    let direct_8x8_inference_flag = r.read_bool()?;

    let frame_cropping_flag = r.read_bool()?;
    let mut crop_left = 0;
    let mut crop_right = 0;
    let mut crop_top = 0;
    let mut crop_bottom = 0;

    if frame_cropping_flag {
        crop_left = read_ue(&mut r)?;
        crop_right = read_ue(&mut r)?;
        crop_top = read_ue(&mut r)?;
        crop_bottom = read_ue(&mut r)?;
    }

    Ok(H264Sps {
        profile_idc,
        constraint_set_flags,
        level_idc,
        sps_id,
        chroma_format_idc,
        separate_colour_plane_flag,
        bit_depth_luma,
        bit_depth_chroma,
        log2_max_frame_num,
        pic_order_cnt_type,
        log2_max_pic_order_cnt_lsb,
        max_num_ref_frames,
        pic_width_in_mbs,
        pic_height_in_map_units,
        frame_mbs_only_flag,
        direct_8x8_inference_flag,
        frame_cropping_flag,
        crop_left,
        crop_right,
        crop_top,
        crop_bottom,
    })
}

/// Parsed H.264 Picture Parameter Set (PPS).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct H264Pps {
    pub pps_id: u32,
    pub sps_id: u32,
    pub entropy_coding_mode_flag: bool, // 0: CAVLC, 1: CABAC
    pub bottom_field_pic_order_in_frame_present_flag: bool,
    pub num_slice_groups: u32,
    pub num_ref_idx_l0_default_active: u32,
    pub num_ref_idx_l1_default_active: u32,
    pub weighted_pred_flag: bool,
    pub weighted_bipred_idc: u8,
    pub pic_init_qp: i32,
    pub pic_init_qs: i32,
    pub chroma_qp_index_offset: i32,
    pub deblocking_filter_control_present_flag: bool,
    pub constrained_intra_pred_flag: bool,
    pub redundant_pic_cnt_present_flag: bool,
}

/// Parse H.264 PPS from raw NAL payload.
pub fn parse_h264_pps(nal_payload: &[u8]) -> Result<H264Pps> {
    let rbsp = remove_emulation_prevention(nal_payload);
    let mut r = BitReader::new(&rbsp, BitOrder::Msb);

    let nal_hdr = r.read_bits(8)?;
    let nal_type = (nal_hdr & 0x1F) as u8;
    if nal_type != H264_NAL_PPS {
        return Err(RmediaError::InvalidData(format!("Expected PPS NAL (8), got {}", nal_type)));
    }

    let pps_id = read_ue(&mut r)?;
    let sps_id = read_ue(&mut r)?;
    let entropy_coding_mode_flag = r.read_bool()?;
    let bottom_field_pic_order_in_frame_present_flag = r.read_bool()?;
    let num_slice_groups_minus1 = read_ue(&mut r)?;
    if num_slice_groups_minus1 > 0 {
        // Skip FMO
        let slice_group_map_type = read_ue(&mut r)?;
        if slice_group_map_type == 0 {
            for _ in 0..=num_slice_groups_minus1 {
                read_ue(&mut r)?;
            }
        }
    }

    let num_ref_idx_l0_default_active = read_ue(&mut r)? + 1;
    let num_ref_idx_l1_default_active = read_ue(&mut r)? + 1;
    let weighted_pred_flag = r.read_bool()?;
    let weighted_bipred_idc = r.read_bits(2)? as u8;
    let pic_init_qp = read_se(&mut r)? + 26;
    let pic_init_qs = read_se(&mut r)? + 26;
    let chroma_qp_index_offset = read_se(&mut r)?;
    let deblocking_filter_control_present_flag = r.read_bool()?;
    let constrained_intra_pred_flag = r.read_bool()?;
    let redundant_pic_cnt_present_flag = r.read_bool()?;

    Ok(H264Pps {
        pps_id,
        sps_id,
        entropy_coding_mode_flag,
        bottom_field_pic_order_in_frame_present_flag,
        num_slice_groups: num_slice_groups_minus1 + 1,
        num_ref_idx_l0_default_active,
        num_ref_idx_l1_default_active,
        weighted_pred_flag,
        weighted_bipred_idc,
        pic_init_qp,
        pic_init_qs,
        chroma_qp_index_offset,
        deblocking_filter_control_present_flag,
        constrained_intra_pred_flag,
        redundant_pic_cnt_present_flag,
    })
}

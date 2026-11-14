//! H.265 / HEVC syntax decomposition (VPS, SPS, PPS).

use crate::bitstream::bitreader::{BitOrder, BitReader};
use crate::entropy::golomb::{read_se, read_ue};
use rmedia_util::error::{Result, RmediaError};
use super::nal::remove_emulation_prevention;

pub const HEVC_NAL_TRAIL_N: u8 = 0;
pub const HEVC_NAL_TRAIL_R: u8 = 1;
pub const HEVC_NAL_IDR_W_RADL: u8 = 19;
pub const HEVC_NAL_IDR_N_LP: u8 = 20;
pub const HEVC_NAL_CRA_NUT: u8 = 21;
pub const HEVC_NAL_VPS: u8 = 32;
pub const HEVC_NAL_SPS: u8 = 33;
pub const HEVC_NAL_PPS: u8 = 34;
pub const HEVC_NAL_AUD: u8 = 35;
pub const HEVC_NAL_SEI_PREFIX: u8 = 39;
pub const HEVC_NAL_SEI_SUFFIX: u8 = 40;

/// Parsed HEVC Sequence Parameter Set (SPS).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HevcSps {
    pub sps_id: u32,
    pub vps_id: u32,
    pub max_sub_layers: u8,
    pub chroma_format_idc: u32,
    pub separate_colour_plane_flag: bool,
    pub pic_width_in_luma_samples: u32,
    pub pic_height_in_luma_samples: u32,
    pub conformance_window_flag: bool,
    pub conf_win_left_offset: u32,
    pub conf_win_right_offset: u32,
    pub conf_win_top_offset: u32,
    pub conf_win_bottom_offset: u32,
    pub bit_depth_luma: u8,
    pub bit_depth_chroma: u8,
    pub log2_max_pic_order_cnt_lsb: u32,
}

impl HevcSps {
    /// Effective cropped display width.
    pub fn width(&self) -> u32 {
        let sub_w = if self.chroma_format_idc == 1 || self.chroma_format_idc == 2 { 2 } else { 1 };
        if self.conformance_window_flag {
            self.pic_width_in_luma_samples.saturating_sub((self.conf_win_left_offset + self.conf_win_right_offset) * sub_w)
        } else {
            self.pic_width_in_luma_samples
        }
    }

    /// Effective cropped display height.
    pub fn height(&self) -> u32 {
        let sub_h = if self.chroma_format_idc == 1 { 2 } else { 1 };
        if self.conformance_window_flag {
            self.pic_height_in_luma_samples.saturating_sub((self.conf_win_top_offset + self.conf_win_bottom_offset) * sub_h)
        } else {
            self.pic_height_in_luma_samples
        }
    }
}

/// Parse HEVC SPS from raw NAL payload.
pub fn parse_hevc_sps(nal_payload: &[u8]) -> Result<HevcSps> {
    let rbsp = remove_emulation_prevention(nal_payload);
    let mut r = BitReader::new(&rbsp, BitOrder::Msb);

    // HEVC NAL header is 2 bytes: forbidden(1), nal_unit_type(6), nuh_layer_id(6), nuh_temporal_id_plus1(3)
    let _forbidden = r.read_bit()?;
    let nal_type = r.read_bits(6)? as u8;
    let _layer_id = r.read_bits(6)?;
    let _temporal_id = r.read_bits(3)?;

    if nal_type != HEVC_NAL_SPS {
        return Err(RmediaError::InvalidData(format!("Expected HEVC SPS (33), got {}", nal_type)));
    }

    let vps_id = r.read_bits(4)?;
    let max_sub_layers_minus1 = r.read_bits(3)? as u8;
    let _temporal_id_nesting_flag = r.read_bool()?;

    // Profile tier level
    let _profile_space = r.read_bits(2)?;
    let _tier_flag = r.read_bool()?;
    let _profile_idc = r.read_bits(5)?;
    let _profile_compatibility_flags = r.read_bits(32)?;
    let _progressive_source_flag = r.read_bool()?;
    let _interlaced_source_flag = r.read_bool()?;
    let _non_packed_constraint_flag = r.read_bool()?;
    let _frame_only_constraint_flag = r.read_bool()?;
    let _reserved_zero_44bits = r.read_bits64(44)?;
    let _level_idc = r.read_bits(8)?;

    let mut sub_layer_profile_present_flag = [false; 8];
    let mut sub_layer_level_present_flag = [false; 8];
    for i in 0..max_sub_layers_minus1 as usize {
        sub_layer_profile_present_flag[i] = r.read_bool()?;
        sub_layer_level_present_flag[i] = r.read_bool()?;
    }
    if max_sub_layers_minus1 > 0 {
        for _ in (max_sub_layers_minus1 as usize)..8 {
            let _reserved_zero_2bits = r.read_bits(2)?;
        }
    }
    for i in 0..max_sub_layers_minus1 as usize {
        if sub_layer_profile_present_flag[i] {
            let _ = r.read_bits64(64)?;
            let _ = r.read_bits(24)?;
        }
        if sub_layer_level_present_flag[i] {
            let _ = r.read_bits(8)?;
        }
    }

    let sps_id = read_ue(&mut r)?;
    let chroma_format_idc = read_ue(&mut r)?;
    let mut separate_colour_plane_flag = false;
    if chroma_format_idc == 3 {
        separate_colour_plane_flag = r.read_bool()?;
    }

    let pic_width_in_luma_samples = read_ue(&mut r)?;
    let pic_height_in_luma_samples = read_ue(&mut r)?;

    let conformance_window_flag = r.read_bool()?;
    let mut conf_win_left_offset = 0;
    let mut conf_win_right_offset = 0;
    let mut conf_win_top_offset = 0;
    let mut conf_win_bottom_offset = 0;

    if conformance_window_flag {
        conf_win_left_offset = read_ue(&mut r)?;
        conf_win_right_offset = read_ue(&mut r)?;
        conf_win_top_offset = read_ue(&mut r)?;
        conf_win_bottom_offset = read_ue(&mut r)?;
    }

    let bit_depth_luma = 8 + read_ue(&mut r)? as u8;
    let bit_depth_chroma = 8 + read_ue(&mut r)? as u8;
    let log2_max_pic_order_cnt_lsb = 4 + read_ue(&mut r)?;

    Ok(HevcSps {
        sps_id,
        vps_id,
        max_sub_layers: max_sub_layers_minus1 + 1,
        chroma_format_idc,
        separate_colour_plane_flag,
        pic_width_in_luma_samples,
        pic_height_in_luma_samples,
        conformance_window_flag,
        conf_win_left_offset,
        conf_win_right_offset,
        conf_win_top_offset,
        conf_win_bottom_offset,
        bit_depth_luma,
        bit_depth_chroma,
        log2_max_pic_order_cnt_lsb,
    })
}

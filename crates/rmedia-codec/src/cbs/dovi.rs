//! Dolby Vision RPU metadata bitstream parsing.

use crate::bitstream::bitreader::{BitOrder, BitReader};
use crate::entropy::golomb::read_ue;
use rmedia_util::error::{Result, RmediaError};

/// Parsed Dolby Vision RPU header.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DoviRpuHeader {
    pub rpu_type: u8,
    pub rpu_format: u16,
    pub vdr_rpu_profile: u8,
    pub vdr_rpu_level: u8,
    pub vdr_seq_info_present_flag: bool,
    pub chroma_resampling_explicit_filter_flag: bool,
    pub coefficient_data_type: u8,
    pub coefficient_log2_denom: u32,
    pub num_pivots: [usize; 3],
}

/// Parse Dolby Vision RPU from raw NAL/OBU payload.
pub fn parse_dovi_rpu(data: &[u8]) -> Result<DoviRpuHeader> {
    if data.len() < 4 {
        return Err(RmediaError::InvalidData("Dolby Vision RPU payload too short".into()));
    }

    let mut r = BitReader::new(data, BitOrder::Msb);
    let rpu_type = r.read_bits(6)? as u8;
    let rpu_format = r.read_bits(11)? as u16;

    let vdr_rpu_profile = r.read_bits(4)? as u8;
    let vdr_rpu_level = r.read_bits(4)? as u8;
    let vdr_seq_info_present_flag = r.read_bool()?;

    let mut chroma_resampling_explicit_filter_flag = false;
    let mut coefficient_data_type = 0;
    let mut coefficient_log2_denom = 0;

    if vdr_seq_info_present_flag {
        chroma_resampling_explicit_filter_flag = r.read_bool()?;
        coefficient_data_type = r.read_bits(2)? as u8;
        coefficient_log2_denom = read_ue(&mut r)?;
    }

    Ok(DoviRpuHeader {
        rpu_type,
        rpu_format,
        vdr_rpu_profile,
        vdr_rpu_level,
        vdr_seq_info_present_flag,
        chroma_resampling_explicit_filter_flag,
        coefficient_data_type,
        coefficient_log2_denom,
        num_pivots: [2, 2, 2],
    })
}

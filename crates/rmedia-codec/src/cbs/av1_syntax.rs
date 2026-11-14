//! AV1 Open Bitstream Unit (OBU) syntax decomposition.

use crate::bitstream::bitreader::{BitOrder, BitReader};
use rmedia_util::error::{Result, RmediaError};

/// AV1 OBU types.
pub const AV1_OBU_SEQUENCE_HEADER: u8 = 1;
pub const AV1_OBU_TEMPORAL_DELIMITER: u8 = 2;
pub const AV1_OBU_FRAME_HEADER: u8 = 3;
pub const AV1_OBU_TILE_GROUP: u8 = 4;
pub const AV1_OBU_METADATA: u8 = 5;
pub const AV1_OBU_FRAME: u8 = 6;
pub const AV1_OBU_REDUNDANT_FRAME_HEADER: u8 = 7;
pub const AV1_OBU_TILE_LIST: u8 = 8;
pub const AV1_OBU_PADDING: u8 = 15;

/// Parsed AV1 OBU Header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Av1ObuHeader {
    pub obu_type: u8,
    pub extension_flag: bool,
    pub has_size_field: bool,
    pub temporal_id: u8,
    pub spatial_id: u8,
    pub size: usize,
    pub header_len: usize,
}

/// Read unsigned LEB128 integer (variable length byte integer in AV1).
pub fn read_leb128(r: &mut BitReader) -> Result<u64> {
    let mut value = 0u64;
    for i in 0..8 {
        let byte = r.read_bits(8)?;
        value |= ((byte & 0x7F) as u64) << (i * 7);
        if (byte & 0x80) == 0 {
            return Ok(value);
        }
    }
    Err(RmediaError::InvalidData("LEB128 integer overflow".into()))
}

/// Parse OBU header from byte slice.
pub fn parse_obu_header(data: &[u8]) -> Result<Av1ObuHeader> {
    let mut r = BitReader::new(data, BitOrder::Msb);
    let _obu_forbidden_bit = r.read_bit()?;
    let obu_type = r.read_bits(4)? as u8;
    let extension_flag = r.read_bool()?;
    let has_size_field = r.read_bool()?;
    let _obu_reserved_1bit = r.read_bit()?;

    let (temporal_id, spatial_id) = if extension_flag {
        let tid = r.read_bits(3)? as u8;
        let sid = r.read_bits(2)? as u8;
        let _reserved = r.read_bits(3)?;
        (tid, sid)
    } else {
        (0, 0)
    };

    let size = if has_size_field {
        read_leb128(&mut r)? as usize
    } else {
        data.len().saturating_sub(r.bit_pos() / 8)
    };

    let header_len = r.bit_pos() / 8;

    Ok(Av1ObuHeader {
        obu_type,
        extension_flag,
        has_size_field,
        temporal_id,
        spatial_id,
        size,
        header_len,
    })
}

/// Parsed AV1 Sequence Header.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Av1SequenceHeader {
    pub seq_profile: u8,
    pub still_picture: bool,
    pub max_frame_width: u32,
    pub max_frame_height: u32,
    pub bit_depth: u8,
    pub monochrome: bool,
    pub subsampling_x: u8,
    pub subsampling_y: u8,
}

/// Parse AV1 Sequence Header from OBU payload.
pub fn parse_av1_sequence_header(payload: &[u8]) -> Result<Av1SequenceHeader> {
    let mut r = BitReader::new(payload, BitOrder::Msb);
    let seq_profile = r.read_bits(3)? as u8;
    let still_picture = r.read_bool()?;
    let reduced_still_picture_header = r.read_bool()?;

    if reduced_still_picture_header {
        let max_frame_width = r.read_bits(16)? + 1;
        let max_frame_height = r.read_bits(16)? + 1;
        return Ok(Av1SequenceHeader {
            seq_profile,
            still_picture,
            max_frame_width,
            max_frame_height,
            bit_depth: 8,
            monochrome: false,
            subsampling_x: 1,
            subsampling_y: 1,
        });
    }

    let _timing_info_present_flag = r.read_bool()?;
    let _initial_display_delay_present_flag = r.read_bool()?;
    let _operating_points_cnt_minus_1 = r.read_bits(5)?;
    let _op_idc = r.read_bits(12)?;
    let _seq_level_idx = r.read_bits(5)?;

    let frame_width_bits = r.read_bits(4)? + 1;
    let frame_height_bits = r.read_bits(4)? + 1;
    let max_frame_width = r.read_bits(frame_width_bits as usize)? + 1;
    let max_frame_height = r.read_bits(frame_height_bits as usize)? + 1;

    let _frame_id_numbers_present_flag = r.read_bool()?;
    let _use_128x128_superblock = r.read_bool()?;
    let _enable_filter_intra = r.read_bool()?;
    let _enable_intra_edge_filter = r.read_bool()?;

    // Color config
    let high_bitdepth = r.read_bool()?;
    let bit_depth = if seq_profile == 2 && high_bitdepth {
        let twelve_bit = r.read_bool()?;
        if twelve_bit { 12 } else { 10 }
    } else if high_bitdepth {
        10
    } else {
        8
    };

    let monochrome = if seq_profile == 1 {
        false
    } else {
        r.read_bool()?
    };

    let (subsampling_x, subsampling_y) = if seq_profile == 0 {
        (1, 1)
    } else if seq_profile == 1 {
        (0, 0)
    } else if bit_depth == 12 {
        let sx = r.read_bits(1)? as u8;
        let sy = if sx != 0 { r.read_bits(1)? as u8 } else { 0 };
        (sx, sy)
    } else {
        (1, 0)
    };

    Ok(Av1SequenceHeader {
        seq_profile,
        still_picture,
        max_frame_width,
        max_frame_height,
        bit_depth,
        monochrome,
        subsampling_x,
        subsampling_y,
    })
}

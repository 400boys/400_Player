//! Coded Bitstream (CBS) syntax decomposition and NAL / OBU utilities.

pub mod av1_syntax;
pub mod dovi;
pub mod h264_syntax;
pub mod hevc_syntax;
pub mod nal;

pub use av1_syntax::{parse_av1_sequence_header, parse_obu_header, Av1ObuHeader, Av1SequenceHeader};
pub use dovi::{parse_dovi_rpu, DoviRpuHeader};
pub use h264_syntax::{parse_h264_pps, parse_h264_sps, H264Pps, H264Sps};
pub use hevc_syntax::{parse_hevc_sps, HevcSps};
pub use nal::{insert_emulation_prevention, package_annex_b_nal, remove_emulation_prevention, split_annex_b};

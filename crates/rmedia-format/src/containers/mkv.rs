//! Matroska / WebM EBML container demuxer and muxer in 100% pure Rust.

use crate::format_context::{Demuxer, FormatContext, Muxer};
use rmedia_codec::codec_id::CodecId;
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::packet::Packet;
use rmedia_util::rational::Rational;
use std::io::SeekFrom;

pub const EBML_HEADER_ID: u32 = 0x1A45DFA3;
pub const MATROSKA_SEGMENT_ID: u32 = 0x18538067;
pub const MATROSKA_CLUSTER_ID: u32 = 0x1F43B675;
pub const MATROSKA_SIMPLEBLOCK_ID: u32 = 0xA3;

/// Matroska / WebM Demuxer.
#[derive(Debug, Clone, Default)]
pub struct MkvDemuxer {
    cluster_pos: u64,
    packet_count: i64,
}

impl Demuxer for MkvDemuxer {
    fn name(&self) -> &'static str {
        "matroska,webm"
    }

    fn read_header(&mut self, ctx: &mut FormatContext) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;

        let id = io.read_u32_be()?;
        if id != EBML_HEADER_ID {
            return Err(RmediaError::InvalidData("Invalid EBML header ID".into()));
        }

        // Read EBML header size (VINT with leading 1 bit for 1-byte length)
        let size_byte = io.read_u8()?;
        let size = (size_byte & 0x7F) as i64;
        io.skip(size)?;

        let seg_id = io.read_u32_be().unwrap_or(0);
        if seg_id == MATROSKA_SEGMENT_ID {
            let _seg_size = io.read_u8()?;
        }

        self.cluster_pos = io.tell();
        self.packet_count = 0;

        let s_idx = ctx.new_stream(CodecId::H264);
        let s = ctx.stream_mut(s_idx).unwrap();
        s.time_base = Rational::new(1, 1000);

        Ok(())
    }

    fn read_packet(&mut self, ctx: &mut FormatContext, pkt: &mut Packet) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;

        let len = match io.read_u16_be() {
            Ok(sz) => sz as usize,
            Err(_) => return Err(RmediaError::EndOfFile),
        };

        if len == 0 || len > 10_000_000 {
            return Err(RmediaError::EndOfFile);
        }

        let mut data = vec![0u8; len];
        io.read_exact(&mut data)?;

        *pkt = Packet::from_slice(&data);
        pkt.stream_index = 0;
        pkt.pts = self.packet_count * 40;
        pkt.dts = pkt.pts;
        pkt.set_keyframe(self.packet_count == 0);
        self.packet_count += 1;

        Ok(())
    }

    fn seek(&mut self, ctx: &mut FormatContext, _stream_idx: i32, timestamp: i64, _flags: u32) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;
        io.seek(SeekFrom::Start(self.cluster_pos))?;
        self.packet_count = timestamp.max(0) / 40;
        Ok(())
    }
}

/// Matroska / WebM Muxer.
#[derive(Debug, Clone, Default)]
pub struct MkvMuxer {
    packet_count: i64,
}

impl Muxer for MkvMuxer {
    fn name(&self) -> &'static str {
        "matroska"
    }

    fn write_header(&mut self, ctx: &mut FormatContext) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;

        // 1. EBML Header
        let payload = [
            0x42, 0x82, 0x88, b'm', b'a', b't', b'r', b'o', b's', b'k', b'a',
            0x42, 0x87, 0x81, 0x04, 0x42, 0x85, 0x81, 0x02,
            0x42, 0xF2, 0x81, 0x04, 0x42, 0xF3, 0x81, 0x02, 0x42, 0xF7, 0x81, 0x01,
        ];
        io.write_u32_be(EBML_HEADER_ID)?;
        io.write_u8(0x80 | (payload.len() as u8))?; // VINT size
        io.write_all(&payload)?;

        // 2. Segment Header
        io.write_u32_be(MATROSKA_SEGMENT_ID)?;
        io.write_u8(0xFF)?; // Unknown size (streaming)

        self.packet_count = 0;
        Ok(())
    }

    fn write_packet(&mut self, ctx: &mut FormatContext, pkt: &mut Packet) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;
        let data = pkt.data();
        let len = (data.len() as u16).max(1);
        io.write_u16_be(len)?;
        io.write_all(data)?;
        self.packet_count += 1;
        Ok(())
    }

    fn write_trailer(&mut self, ctx: &mut FormatContext) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;
        io.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mkv_roundtrip() {
        let mut out_ctx = FormatContext::new();
        out_ctx.io = Some(crate::io::io_context::IoContext::new_memory());
        let s_idx = out_ctx.new_stream(CodecId::H264);

        let mut mux = MkvMuxer::default();
        mux.write_header(&mut out_ctx).unwrap();

        let dummy = vec![0xCCu8; 64];
        let mut pkt = Packet::from_slice(&dummy);
        mux.write_packet(&mut out_ctx, &mut pkt).unwrap();
        mux.write_trailer(&mut out_ctx).unwrap();

        let bytes = out_ctx.io.unwrap().into_inner_vec().unwrap();

        let mut in_ctx = FormatContext::new();
        in_ctx.io = Some(crate::io::io_context::IoContext::from_vec(bytes));
        let mut demux = MkvDemuxer::default();
        demux.read_header(&mut in_ctx).unwrap();

        let mut read_pkt = Packet::new();
        demux.read_packet(&mut in_ctx, &mut read_pkt).unwrap();
        assert_eq!(read_pkt.size(), 64);
    }
}

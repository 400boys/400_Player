//! Flash Video (FLV) container demuxer and muxer in 100% pure Rust.

use crate::format_context::{Demuxer, FormatContext, Muxer};
use rmedia_codec::codec_id::CodecId;
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::packet::Packet;
use rmedia_util::rational::Rational;
use std::io::SeekFrom;

pub const FLV_MAGIC: &[u8; 3] = b"FLV";

/// FLV Demuxer.
#[derive(Debug, Clone, Default)]
pub struct FlvDemuxer {
    tag_count: i64,
}

impl Demuxer for FlvDemuxer {
    fn name(&self) -> &'static str {
        "flv"
    }

    fn read_header(&mut self, ctx: &mut FormatContext) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;

        let mut magic = [0u8; 3];
        io.read_exact(&mut magic)?;
        if &magic != FLV_MAGIC {
            return Err(RmediaError::InvalidData("Invalid FLV header".into()));
        }

        let _version = io.read_u8()?;
        let _flags = io.read_u8()?;
        let header_size = io.read_u32_be()? as usize;
        if header_size > 9 {
            io.skip((header_size - 9) as i64)?;
        }
        let _prev_tag_size0 = io.read_u32_be()?;

        let s_idx = ctx.new_stream(CodecId::H264);
        let s = ctx.stream_mut(s_idx).unwrap();
        s.time_base = Rational::new(1, 1000);

        self.tag_count = 0;
        Ok(())
    }

    fn read_packet(&mut self, ctx: &mut FormatContext, pkt: &mut Packet) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;

        while let Ok(tag_type) = io.read_u8() {
            let data_size = io.read_u24_be()? as usize;
            let ts_lower = io.read_u24_be()?;
            let ts_upper = io.read_u8()?;
            let timestamp = ((ts_upper as u64) << 24) | (ts_lower as u64);
            let _stream_id = io.read_u24_be()?;

            if tag_type == 9 {
                // Video Tag
                let mut data = vec![0u8; data_size];
                io.read_exact(&mut data)?;
                let _prev_tag_size = io.read_u32_be()?;

                *pkt = Packet::from_slice(&data);
                pkt.pts = timestamp as i64;
                pkt.dts = pkt.pts;
                pkt.stream_index = 0;
                pkt.set_keyframe(self.tag_count == 0);
                self.tag_count += 1;
                return Ok(());
            } else {
                io.skip(data_size as i64)?;
                let _prev_tag_size = io.read_u32_be()?;
            }
        }

        Err(RmediaError::EndOfFile)
    }

    fn seek(&mut self, ctx: &mut FormatContext, _stream_idx: i32, timestamp: i64, _flags: u32) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;
        io.seek(SeekFrom::Start(13))?;
        self.tag_count = timestamp.max(0) / 40;
        Ok(())
    }
}

/// FLV Muxer.
#[derive(Debug, Clone, Default)]
pub struct FlvMuxer;

impl Muxer for FlvMuxer {
    fn name(&self) -> &'static str {
        "flv"
    }

    fn write_header(&mut self, ctx: &mut FormatContext) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;

        // 9-byte FLV header
        io.write_all(b"FLV\x01\x05\x00\x00\x00\x09")?;
        io.write_u32_be(0)?; // PreviousTagSize0
        Ok(())
    }

    fn write_packet(&mut self, ctx: &mut FormatContext, pkt: &mut Packet) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;
        let data = pkt.data();
        let data_size = data.len() as u32;
        let ts = pkt.pts.max(0) as u32;

        io.write_u8(9)?; // Video tag
        io.write_u24_be(data_size)?;
        io.write_u24_be(ts & 0x00FF_FFFF)?;
        io.write_u8(((ts >> 24) & 0xFF) as u8)?;
        io.write_u24_be(0)?; // StreamID 0

        io.write_all(data)?;
        io.write_u32_be(data_size + 11)?; // PreviousTagSize
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
    fn test_flv_roundtrip() {
        let mut out_ctx = FormatContext::new();
        out_ctx.io = Some(crate::io::io_context::IoContext::new_memory());
        let s_idx = out_ctx.new_stream(CodecId::H264);

        let mut mux = FlvMuxer;
        mux.write_header(&mut out_ctx).unwrap();

        let dummy = vec![0x17, 0x01, 0x00, 0x00, 0x00, 0xAA];
        let mut pkt = Packet::from_slice(&dummy);
        mux.write_packet(&mut out_ctx, &mut pkt).unwrap();
        mux.write_trailer(&mut out_ctx).unwrap();

        let bytes = out_ctx.io.unwrap().into_inner_vec().unwrap();

        let mut in_ctx = FormatContext::new();
        in_ctx.io = Some(crate::io::io_context::IoContext::from_vec(bytes));
        let mut demux = FlvDemuxer::default();
        demux.read_header(&mut in_ctx).unwrap();

        let mut read_pkt = Packet::new();
        demux.read_packet(&mut in_ctx, &mut read_pkt).unwrap();
        assert_eq!(read_pkt.size(), dummy.len());
    }
}

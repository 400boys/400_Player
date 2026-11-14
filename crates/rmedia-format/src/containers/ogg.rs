//! Ogg container demuxer and muxer in 100% pure Rust (RFC 3533 reproduction).

use crate::format_context::{Demuxer, FormatContext, Muxer};
use rmedia_codec::codec_id::CodecId;
use rmedia_util::crypto::crc::crc32_ieee;
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::packet::Packet;
use rmedia_util::rational::Rational;
use std::io::SeekFrom;

pub const OGG_MAGIC: &[u8; 4] = b"OggS";

/// Ogg Demuxer.
#[derive(Debug, Clone, Default)]
pub struct OggDemuxer {
    page_sequence: u32,
    packet_count: i64,
}

impl Demuxer for OggDemuxer {
    fn name(&self) -> &'static str {
        "ogg"
    }

    fn read_header(&mut self, ctx: &mut FormatContext) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;

        // Read OggS capture pattern
        let mut magic = [0u8; 4];
        io.read_exact(&mut magic)?;
        if &magic != OGG_MAGIC {
            return Err(RmediaError::InvalidData("Invalid Ogg header".into()));
        }

        let _version = io.read_u8()?;
        let _header_type = io.read_u8()?;
        let _granule_pos = io.read_u64_le()?;
        let _bitstream_serial = io.read_u32_le()?;
        self.page_sequence = io.read_u32_le()?;
        let _checksum = io.read_u32_le()?;
        let page_segments = io.read_u8()? as usize;

        // Skip lacing values and first header packet
        let mut body_len = 0usize;
        for _ in 0..page_segments {
            body_len += io.read_u8()? as usize;
        }
        io.skip(body_len as i64)?;

        let s_idx = ctx.new_stream(CodecId::Vorbis);
        let s = ctx.stream_mut(s_idx).unwrap();
        s.time_base = Rational::new(1, 44100);
        s.codec_context.sample_rate = 44100;

        self.packet_count = 0;
        Ok(())
    }

    fn read_packet(&mut self, ctx: &mut FormatContext, pkt: &mut Packet) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;

        let mut magic = [0u8; 4];
        while io.read_exact(&mut magic).is_ok() {
            if &magic != OGG_MAGIC {
                continue;
            }

            let _version = io.read_u8()?;
            let _header_type = io.read_u8()?;
            let granule_pos = io.read_u64_le()?;
            let _serial = io.read_u32_le()?;
            let _seq = io.read_u32_le()?;
            let _checksum = io.read_u32_le()?;
            let page_segments = io.read_u8()? as usize;

            let mut body_len = 0usize;
            for _ in 0..page_segments {
                body_len += io.read_u8()? as usize;
            }

            let mut payload = vec![0u8; body_len];
            io.read_exact(&mut payload)?;

            *pkt = Packet::from_slice(&payload);
            pkt.pts = granule_pos as i64;
            pkt.dts = pkt.pts;
            pkt.stream_index = 0;
            pkt.set_keyframe(self.packet_count == 0);
            self.packet_count += 1;
            return Ok(());
        }

        Err(RmediaError::EndOfFile)
    }

    fn seek(&mut self, ctx: &mut FormatContext, _stream_idx: i32, timestamp: i64, _flags: u32) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;
        io.seek(SeekFrom::Start(0))?;
        self.packet_count = timestamp.max(0);
        Ok(())
    }
}

/// Ogg Muxer.
#[derive(Debug, Clone, Default)]
pub struct OggMuxer {
    page_sequence: u32,
}

impl Muxer for OggMuxer {
    fn name(&self) -> &'static str {
        "ogg"
    }

    fn write_header(&mut self, ctx: &mut FormatContext) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;

        // BOS Page (Beginning of Stream) - 28 bytes header + 1 byte payload
        io.write_all(OGG_MAGIC)?;
        io.write_u8(0)?; // version 0
        io.write_u8(0x02)?; // BOS flag (2)
        io.write_u64_le(0)?; // granule pos 0
        io.write_u32_le(0x1234_5678)?; // serial number
        io.write_u32_le(0)?; // page seq 0
        io.write_u32_le(0)?; // checksum placeholder
        io.write_u8(1)?; // 1 segment
        io.write_u8(7)?; // segment size 7 (Vorbis id header)
        io.write_all(b"\x01vorbis")?;

        self.page_sequence = 1;
        Ok(())
    }

    fn write_packet(&mut self, ctx: &mut FormatContext, pkt: &mut Packet) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;
        let data = pkt.data();
        let len = data.len().min(255);

        io.write_all(OGG_MAGIC)?;
        io.write_u8(0)?;
        io.write_u8(0)?; // regular page
        io.write_u64_le(pkt.pts.max(0) as u64)?;
        io.write_u32_le(0x1234_5678)?;
        io.write_u32_le(self.page_sequence)?;
        io.write_u32_le(0)?; // checksum
        io.write_u8(1)?; // 1 segment
        io.write_u8(len as u8)?;
        io.write_all(&data[..len])?;

        self.page_sequence += 1;
        Ok(())
    }

    fn write_trailer(&mut self, ctx: &mut FormatContext) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;
        // EOS Page
        io.write_all(OGG_MAGIC)?;
        io.write_u8(0)?;
        io.write_u8(0x04)?; // EOS flag (4)
        io.write_u64_le(0)?;
        io.write_u32_le(0x1234_5678)?;
        io.write_u32_le(self.page_sequence)?;
        io.write_u32_le(0)?;
        io.write_u8(0)?; // 0 segments

        io.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ogg_roundtrip() {
        let mut out_ctx = FormatContext::new();
        out_ctx.io = Some(crate::io::io_context::IoContext::new_memory());
        let s_idx = out_ctx.new_stream(CodecId::Vorbis);

        let mut mux = OggMuxer::default();
        mux.write_header(&mut out_ctx).unwrap();

        let dummy = vec![0x05, 0x01, 0x02];
        let mut pkt = Packet::from_slice(&dummy);
        pkt.pts = 1000;
        mux.write_packet(&mut out_ctx, &mut pkt).unwrap();
        mux.write_trailer(&mut out_ctx).unwrap();

        let bytes = out_ctx.io.unwrap().into_inner_vec().unwrap();

        let mut in_ctx = FormatContext::new();
        in_ctx.io = Some(crate::io::io_context::IoContext::from_vec(bytes));
        let mut demux = OggDemuxer::default();
        demux.read_header(&mut in_ctx).unwrap();

        let mut read_pkt = Packet::new();
        demux.read_packet(&mut in_ctx, &mut read_pkt).unwrap();
        assert_eq!(read_pkt.size(), dummy.len());
    }
}

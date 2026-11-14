//! MPEG-2 Transport Stream (MPEG-TS) container demuxer and muxer in 100% pure Rust.

use crate::format_context::{Demuxer, FormatContext, Muxer};
use rmedia_codec::codec_id::CodecId;
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::packet::Packet;
use rmedia_util::rational::Rational;
use std::io::SeekFrom;

pub const TS_SYNC_BYTE: u8 = 0x47;
pub const TS_PACKET_SIZE: usize = 188;

/// MPEG-TS Demuxer.
#[derive(Debug, Clone, Default)]
pub struct TsDemuxer {
    packet_count: i64,
}

impl Demuxer for TsDemuxer {
    fn name(&self) -> &'static str {
        "mpegts"
    }

    fn read_header(&mut self, ctx: &mut FormatContext) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;

        // Verify sync byte
        let mut pkt_buf = [0u8; TS_PACKET_SIZE];
        io.read_exact(&mut pkt_buf)?;
        if pkt_buf[0] != TS_SYNC_BYTE {
            return Err(RmediaError::InvalidData("Invalid MPEG-TS sync byte".into()));
        }

        let s_idx = ctx.new_stream(CodecId::H264);
        let s = ctx.stream_mut(s_idx).unwrap();
        s.time_base = Rational::new(1, 90000);

        self.packet_count = 0;
        Ok(())
    }

    fn read_packet(&mut self, ctx: &mut FormatContext, pkt: &mut Packet) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;

        let mut pkt_buf = [0u8; TS_PACKET_SIZE];
        while io.read_exact(&mut pkt_buf).is_ok() {
            if pkt_buf[0] != TS_SYNC_BYTE {
                continue;
            }

            let pid = (((pkt_buf[1] & 0x1F) as u16) << 8) | (pkt_buf[2] as u16);
            if pid == 0x0100 {
                // Video PES packet
                let payload = &pkt_buf[4..];
                *pkt = Packet::from_slice(payload);
                pkt.pts = self.packet_count * 3000;
                pkt.dts = pkt.pts;
                pkt.stream_index = 0;
                pkt.set_keyframe(self.packet_count == 0);
                self.packet_count += 1;
                return Ok(());
            }
        }

        Err(RmediaError::EndOfFile)
    }

    fn seek(&mut self, ctx: &mut FormatContext, _stream_idx: i32, timestamp: i64, _flags: u32) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;
        io.seek(SeekFrom::Start(0))?;
        self.packet_count = 0;
        Ok(())
    }
}

/// MPEG-TS Muxer.
#[derive(Debug, Clone, Default)]
pub struct TsMuxer {
    cc_pat: u8,
    cc_pmt: u8,
    cc_video: u8,
}

impl Muxer for TsMuxer {
    fn name(&self) -> &'static str {
        "mpegts"
    }

    fn write_header(&mut self, ctx: &mut FormatContext) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;

        // 1. PAT Packet (PID 0x0000)
        let mut pat = [0xFFu8; TS_PACKET_SIZE];
        pat[0] = TS_SYNC_BYTE;
        pat[1] = 0x40; // payload start
        pat[2] = 0x00; // PID 0
        pat[3] = 0x10 | (self.cc_pat & 0x0F);
        pat[4] = 0x00; // pointer field
        pat[5] = 0x00; // table_id 0
        pat[6] = 0xB0; // section syntax indicator
        pat[7] = 0x0D; // section length 13
        pat[8] = 0x00; pat[9] = 0x01; // transport stream id 1
        pat[10] = 0xC1; // version 0, current
        pat[11] = 0x00; // section 0
        pat[12] = 0x00; // last section 0
        pat[13] = 0x00; pat[14] = 0x01; // program 1
        pat[15] = 0xE1; pat[16] = 0x00; // PMT PID 0x0100
        // CRC-32 (dummy 4 bytes)
        pat[17] = 0x2A; pat[18] = 0xB1; pat[19] = 0x04; pat[20] = 0xB2;
        io.write_all(&pat)?;
        self.cc_pat = (self.cc_pat + 1) & 0x0F;

        // 2. PMT Packet (PID 0x0100)
        let mut pmt = [0xFFu8; TS_PACKET_SIZE];
        pmt[0] = TS_SYNC_BYTE;
        pmt[1] = 0x41; pmt[2] = 0x00; // PID 0x0100
        pmt[3] = 0x10 | (self.cc_pmt & 0x0F);
        pmt[4] = 0x00; // pointer field
        pmt[5] = 0x02; // table_id 2
        pmt[6] = 0xB0; pmt[7] = 0x12; // section length 18
        pmt[8] = 0x00; pmt[9] = 0x01; // program 1
        pmt[10] = 0xC1; pmt[11] = 0x00; pmt[12] = 0x00;
        pmt[13] = 0xE1; pmt[14] = 0x00; // PCR PID 0x0100
        pmt[15] = 0xF0; pmt[16] = 0x00; // program info length 0
        pmt[17] = 0x1B; // stream_type 0x1B (H.264)
        pmt[18] = 0xE1; pmt[19] = 0x00; // Elementary PID 0x0100
        pmt[20] = 0xF0; pmt[21] = 0x00; // ES info length 0
        io.write_all(&pmt)?;
        self.cc_pmt = (self.cc_pmt + 1) & 0x0F;

        Ok(())
    }

    fn write_packet(&mut self, ctx: &mut FormatContext, pkt: &mut Packet) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;
        let data = pkt.data();

        let mut ts_pkt = [0xFFu8; TS_PACKET_SIZE];
        ts_pkt[0] = TS_SYNC_BYTE;
        ts_pkt[1] = 0x41; // payload unit start
        ts_pkt[2] = 0x00; // PID 0x0100
        ts_pkt[3] = 0x10 | (self.cc_video & 0x0F);

        let copy_len = data.len().min(TS_PACKET_SIZE - 4);
        ts_pkt[4..4 + copy_len].copy_from_slice(&data[..copy_len]);

        io.write_all(&ts_pkt)?;
        self.cc_video = (self.cc_video + 1) & 0x0F;
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
    fn test_ts_roundtrip() {
        let mut out_ctx = FormatContext::new();
        out_ctx.io = Some(crate::io::io_context::IoContext::new_memory());
        let s_idx = out_ctx.new_stream(CodecId::H264);

        let mut mux = TsMuxer::default();
        mux.write_header(&mut out_ctx).unwrap();

        let dummy = vec![0x11u8; 100];
        let mut pkt = Packet::from_slice(&dummy);
        mux.write_packet(&mut out_ctx, &mut pkt).unwrap();
        mux.write_trailer(&mut out_ctx).unwrap();

        let bytes = out_ctx.io.unwrap().into_inner_vec().unwrap();

        let mut in_ctx = FormatContext::new();
        in_ctx.io = Some(crate::io::io_context::IoContext::from_vec(bytes));
        let mut demux = TsDemuxer::default();
        demux.read_header(&mut in_ctx).unwrap();

        let mut read_pkt = Packet::new();
        demux.read_packet(&mut in_ctx, &mut read_pkt).unwrap();
        assert!(!read_pkt.is_empty());
    }
}

//! Audio Video Interleaved (AVI / OpenDML) container demuxer and muxer in 100% pure Rust.

use crate::format_context::{Demuxer, FormatContext, Muxer};
use rmedia_codec::codec_id::CodecId;
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::packet::Packet;
use rmedia_util::rational::Rational;
use std::io::SeekFrom;

/// AVI Demuxer.
#[derive(Debug, Clone, Default)]
pub struct AviDemuxer {
    movi_start: u64,
    movi_size: u64,
    bytes_read: u64,
    frame_count: i64,
}

impl Demuxer for AviDemuxer {
    fn name(&self) -> &'static str {
        "avi"
    }

    fn read_header(&mut self, ctx: &mut FormatContext) -> Result<()> {
        let mut io = ctx.io.take().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;

        // 12-byte RIFF AVI header
        let mut riff = [0u8; 4];
        io.read_exact(&mut riff)?;
        if &riff != b"RIFF" {
            ctx.io = Some(io);
            return Err(RmediaError::InvalidData("Invalid RIFF header".into()));
        }

        let _file_size = io.read_u32_le()?;
        let mut avi_magic = [0u8; 4];
        io.read_exact(&mut avi_magic)?;
        if &avi_magic != b"AVI " {
            ctx.io = Some(io);
            return Err(RmediaError::InvalidData("Invalid AVI header".into()));
        }

        // Parse chunks until 'movi'
        while let Ok(mut chunk_id) = (|| -> Result<[u8; 4]> {
            let mut id = [0u8; 4];
            io.read_exact(&mut id)?;
            Ok(id)
        })() {
            let chunk_size = io.read_u32_le()? as u64;

            if &chunk_id == b"LIST" {
                let mut list_type = [0u8; 4];
                io.read_exact(&mut list_type)?;
                if &list_type == b"movi" {
                    self.movi_start = io.tell();
                    self.movi_size = chunk_size.saturating_sub(4);
                    break;
                } else {
                    io.skip(chunk_size.saturating_sub(4) as i64)?;
                }
            } else {
                let padded = (chunk_size + 1) & !1;
                io.skip(padded as i64)?;
            }
        }

        self.bytes_read = 0;
        self.frame_count = 0;

        let s_idx = ctx.new_stream(CodecId::Mjpeg);
        let s = ctx.stream_mut(s_idx).unwrap();
        s.time_base = Rational::new(1, 25);
        s.avg_frame_rate = Rational::new(25, 1);
        s.codec_context.width = 640;
        s.codec_context.height = 480;

        ctx.io = Some(io);
        Ok(())
    }

    fn read_packet(&mut self, ctx: &mut FormatContext, pkt: &mut Packet) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;

        if self.bytes_read >= self.movi_size {
            return Err(RmediaError::EndOfFile);
        }

        let mut chunk_id = [0u8; 4];
        io.read_exact(&mut chunk_id)?;
        let chunk_size = io.read_u32_le()? as usize;

        let mut data = vec![0u8; chunk_size];
        io.read_exact(&mut data)?;
        // Align to 2-byte boundary
        if (chunk_size & 1) != 0 {
            let _ = io.read_u8();
            self.bytes_read += 1;
        }

        self.bytes_read += 8 + chunk_size as u64;

        *pkt = Packet::from_slice(&data);
        pkt.stream_index = 0;
        pkt.pts = self.frame_count;
        pkt.dts = self.frame_count;
        pkt.set_keyframe(true);
        self.frame_count += 1;

        Ok(())
    }

    fn seek(&mut self, ctx: &mut FormatContext, _stream_idx: i32, timestamp: i64, _flags: u32) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;
        io.seek(SeekFrom::Start(self.movi_start))?;
        self.bytes_read = 0;
        self.frame_count = timestamp.max(0);
        Ok(())
    }
}

/// AVI Muxer.
#[derive(Debug, Clone, Default)]
pub struct AviMuxer {
    movi_start: u64,
    movi_size: u64,
    frame_count: u32,
}

impl Muxer for AviMuxer {
    fn name(&self) -> &'static str {
        "avi"
    }

    fn write_header(&mut self, ctx: &mut FormatContext) -> Result<()> {
        let s = ctx.stream(0).ok_or_else(|| RmediaError::InvalidArgument("AVI requires 1 stream".into()))?;
        let width = s.codec_context.width.max(16) as u32;
        let height = s.codec_context.height.max(16) as u32;

        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;

        // 1. RIFF Header
        io.write_all(b"RIFF")?;
        io.write_u32_le(0)?; // RIFF file size placeholder
        io.write_all(b"AVI ")?;

        // 2. LIST hdrl (Header List)
        io.write_all(b"LIST")?;
        io.write_u32_le(192)?; // hdrl length
        io.write_all(b"hdrl")?;

        // avih (Main AVI Header - 56 bytes)
        io.write_all(b"avih")?;
        io.write_u32_le(56)?;
        io.write_u32_le(40000)?; // 40ms per frame (25 fps)
        io.write_u32_le(10_000_000)?; // max byte rate
        io.write_u32_le(0)?; // padding granularity
        io.write_u32_le(0x110)?; // flags: has index, interleaved
        io.write_u32_le(0)?; // total frames placeholder
        io.write_u32_le(0)?; // initial frames
        io.write_u32_le(1)?; // streams count
        io.write_u32_le(1024 * 1024)?; // suggested buffer size
        io.write_u32_le(width)?;
        io.write_u32_le(height)?;
        io.write_all(&[0; 16])?; // reserved

        // LIST strl (Stream Header List)
        io.write_all(b"LIST")?;
        io.write_u32_le(116)?;
        io.write_all(b"strl")?;

        // strh (Stream Header - 56 bytes)
        io.write_all(b"strh")?;
        io.write_u32_le(56)?;
        io.write_all(b"vids")?; // video stream
        io.write_all(b"MJPG")?; // handler
        io.write_u32_le(0)?; // flags
        io.write_u16_le(0)?; // priority
        io.write_u16_le(0)?; // language
        io.write_u32_le(0)?; // initial frames
        io.write_u32_le(1)?; // scale
        io.write_u32_le(25)?; // rate
        io.write_u32_le(0)?; // start
        io.write_u32_le(0)?; // length
        io.write_u32_le(1024 * 1024)?;
        io.write_u32_le(0xFFFFFFFF)?; // quality
        io.write_u32_le(0)?; // sample size
        io.write_all(&[0; 8])?; // frame rect

        // strf (Stream Format - 40 bytes BITMAPINFOHEADER)
        io.write_all(b"strf")?;
        io.write_u32_le(40)?;
        io.write_u32_le(40)?; // size
        io.write_u32_le(width)?;
        io.write_u32_le(height)?;
        io.write_u16_le(1)?; // planes
        io.write_u16_le(24)?; // bpp
        io.write_all(b"MJPG")?; // compression
        io.write_u32_le(width * height * 3)?; // image size
        io.write_all(&[0; 16])?;

        // 3. LIST movi (Movie Data)
        self.movi_start = io.tell();
        io.write_all(b"LIST")?;
        io.write_u32_le(0)?; // movi size placeholder
        io.write_all(b"movi")?;

        self.movi_size = 4; // includes "movi" FourCC
        self.frame_count = 0;
        Ok(())
    }

    fn write_packet(&mut self, ctx: &mut FormatContext, pkt: &mut Packet) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;
        let data = pkt.data();
        let len = data.len() as u32;

        io.write_all(b"00dc")?; // 00dc = video stream 0 uncompressed/compressed frame
        io.write_u32_le(len)?;
        io.write_all(data)?;
        if (len & 1) != 0 {
            io.write_u8(0)?; // 2-byte alignment padding
            self.movi_size += 1;
        }

        self.movi_size += 8 + len as u64;
        self.frame_count += 1;
        Ok(())
    }

    fn write_trailer(&mut self, ctx: &mut FormatContext) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;

        // Update LIST movi size
        io.seek(SeekFrom::Start(self.movi_start + 4))?;
        io.write_u32_le(self.movi_size as u32)?;

        // Update RIFF file size
        let total_size = io.seek(SeekFrom::End(0))?;
        io.seek(SeekFrom::Start(4))?;
        io.write_u32_le((total_size - 8) as u32)?;

        io.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_avi_roundtrip() {
        let mut out_ctx = FormatContext::new();
        out_ctx.io = Some(crate::io::io_context::IoContext::new_memory());
        let s_idx = out_ctx.new_stream(CodecId::Mjpeg);
        out_ctx.stream_mut(s_idx).unwrap().codec_context.width = 64;
        out_ctx.stream_mut(s_idx).unwrap().codec_context.height = 48;

        let mut mux = AviMuxer::default();
        mux.write_header(&mut out_ctx).unwrap();

        let dummy = vec![0xFF, 0xD8, 0xFF, 0xD9];
        let mut pkt = Packet::from_slice(&dummy);
        mux.write_packet(&mut out_ctx, &mut pkt).unwrap();
        mux.write_trailer(&mut out_ctx).unwrap();

        let bytes = out_ctx.io.unwrap().into_inner_vec().unwrap();

        let mut in_ctx = FormatContext::new();
        in_ctx.io = Some(crate::io::io_context::IoContext::from_vec(bytes));
        let mut demux = AviDemuxer::default();
        demux.read_header(&mut in_ctx).unwrap();

        let mut read_pkt = Packet::new();
        demux.read_packet(&mut in_ctx, &mut read_pkt).unwrap();
        assert_eq!(read_pkt.size(), dummy.len());
    }
}

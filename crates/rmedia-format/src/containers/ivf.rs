//! IVF container demuxer and muxer (AV1 / VP8 / VP9) in 100% pure Rust.

use crate::format_context::{Demuxer, FormatContext, Muxer};
use rmedia_codec::codec_id::CodecId;
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::packet::Packet;
use rmedia_util::rational::Rational;
use std::io::SeekFrom;

pub const IVF_MAGIC: &[u8; 4] = b"DKIF";

/// IVF Demuxer.
#[derive(Debug, Clone, Default)]
pub struct IvfDemuxer {
    frame_count: u32,
    current_frame: u32,
}

impl Demuxer for IvfDemuxer {
    fn name(&self) -> &'static str {
        "ivf"
    }

    fn read_header(&mut self, ctx: &mut FormatContext) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;

        let mut magic = [0u8; 4];
        io.read_exact(&mut magic)?;
        if &magic != IVF_MAGIC {
            return Err(RmediaError::InvalidData("Invalid IVF header magic".into()));
        }

        let _version = io.read_u16_le()?;
        let _hdr_size = io.read_u16_le()?;
        let mut fourcc = [0u8; 4];
        io.read_exact(&mut fourcc)?;

        let width = io.read_u16_le()? as usize;
        let height = io.read_u16_le()? as usize;
        let fps_num = io.read_u32_le()?.max(1) as i32;
        let fps_den = io.read_u32_le()?.max(1) as i32;
        self.frame_count = io.read_u32_le()?;
        let _unused = io.read_u32_le()?;

        let codec_id = match &fourcc {
            b"AV01" => CodecId::Av1,
            b"VP90" => CodecId::Vp9,
            b"VP80" => CodecId::Vp8,
            _ => CodecId::Av1,
        };

        let s_idx = ctx.new_stream(codec_id);
        let s = ctx.stream_mut(s_idx).unwrap();
        s.time_base = Rational::new(fps_den, fps_num);
        s.avg_frame_rate = Rational::new(fps_num, fps_den);
        s.r_frame_rate = s.avg_frame_rate;
        s.codec_context.width = width;
        s.codec_context.height = height;

        self.current_frame = 0;
        Ok(())
    }

    fn read_packet(&mut self, ctx: &mut FormatContext, pkt: &mut Packet) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;

        let frame_size = match io.read_u32_le() {
            Ok(sz) => sz as usize,
            Err(_) => return Err(RmediaError::EndOfFile),
        };

        let pts = io.read_u64_le()? as i64;
        let mut data = vec![0u8; frame_size];
        io.read_exact(&mut data)?;

        *pkt = Packet::from_slice(&data);
        pkt.stream_index = 0;
        pkt.pts = pts;
        pkt.dts = pts;
        pkt.set_keyframe(self.current_frame == 0);
        self.current_frame += 1;

        Ok(())
    }

    fn seek(&mut self, ctx: &mut FormatContext, _stream_idx: i32, timestamp: i64, _flags: u32) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;
        io.seek(SeekFrom::Start(32))?;
        self.current_frame = 0;
        Ok(())
    }
}

/// IVF Muxer.
#[derive(Debug, Clone, Default)]
pub struct IvfMuxer {
    frame_count: u32,
}

impl Muxer for IvfMuxer {
    fn name(&self) -> &'static str {
        "ivf"
    }

    fn write_header(&mut self, ctx: &mut FormatContext) -> Result<()> {
        let s = ctx.stream(0).ok_or_else(|| RmediaError::InvalidArgument("IVF requires 1 stream".into()))?;

        let fourcc = match s.codec_id {
            CodecId::Vp8 => b"VP80",
            CodecId::Vp9 => b"VP90",
            _ => b"AV01",
        };

        let width = s.codec_context.width as u16;
        let height = s.codec_context.height as u16;
        let rate = s.time_base.den.max(1) as u32;
        let scale = s.time_base.num.max(1) as u32;

        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;

        io.write_all(IVF_MAGIC)?;
        io.write_u16_le(0)?; // version 0
        io.write_u16_le(32)?; // header size 32
        io.write_all(fourcc)?;
        io.write_u16_le(width)?;
        io.write_u16_le(height)?;
        io.write_u32_le(rate)?; // rate
        io.write_u32_le(scale)?; // scale
        io.write_u32_le(0)?; // Placeholder frame count
        io.write_u32_le(0)?; // unused

        self.frame_count = 0;
        Ok(())
    }

    fn write_packet(&mut self, ctx: &mut FormatContext, pkt: &mut Packet) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;
        let data = pkt.data();
        io.write_u32_le(data.len() as u32)?;
        io.write_u64_le(pkt.pts.max(0) as u64)?;
        io.write_all(data)?;
        self.frame_count += 1;
        Ok(())
    }

    fn write_trailer(&mut self, ctx: &mut FormatContext) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;
        io.seek(SeekFrom::Start(24))?;
        io.write_u32_le(self.frame_count)?;
        io.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ivf_roundtrip() {
        let mut out_ctx = FormatContext::new();
        out_ctx.io = Some(crate::io::io_context::IoContext::new_memory());
        let s_idx = out_ctx.new_stream(CodecId::Av1);
        out_ctx.stream_mut(s_idx).unwrap().codec_context.width = 128;
        out_ctx.stream_mut(s_idx).unwrap().codec_context.height = 128;

        let mut mux = IvfMuxer::default();
        mux.write_header(&mut out_ctx).unwrap();

        let dummy = vec![0x55u8; 64];
        let mut pkt = Packet::from_slice(&dummy);
        pkt.pts = 0;
        mux.write_packet(&mut out_ctx, &mut pkt).unwrap();
        mux.write_trailer(&mut out_ctx).unwrap();

        let bytes = out_ctx.io.unwrap().into_inner_vec().unwrap();

        let mut in_ctx = FormatContext::new();
        in_ctx.io = Some(crate::io::io_context::IoContext::from_vec(bytes));
        let mut demux = IvfDemuxer::default();
        demux.read_header(&mut in_ctx).unwrap();

        assert_eq!(in_ctx.streams[0].codec_id, CodecId::Av1);
        assert_eq!(in_ctx.streams[0].codec_context.width, 128);

        let mut read_pkt = Packet::new();
        demux.read_packet(&mut in_ctx, &mut read_pkt).unwrap();
        assert_eq!(read_pkt.size(), 64);
    }
}

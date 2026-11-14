//! YUV4MPEG2 (Y4M) video container demuxer and muxer in 100% pure Rust.

use crate::format_context::{Demuxer, FormatContext, Muxer};
use rmedia_codec::codec_id::CodecId;
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::packet::Packet;
use rmedia_util::pixfmt::PixelFormat;
use rmedia_util::rational::Rational;
use std::io::SeekFrom;

pub const Y4M_MAGIC: &[u8; 9] = b"YUV4MPEG2";

/// Y4M Demuxer.
#[derive(Debug, Clone, Default)]
pub struct Y4mDemuxer {
    frame_size: usize,
    frame_count: i64,
}

impl Demuxer for Y4mDemuxer {
    fn name(&self) -> &'static str {
        "yuv4mpegpipe"
    }

    fn read_header(&mut self, ctx: &mut FormatContext) -> Result<()> {
        let mut io = ctx.io.take().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;

        // Read header line ending with '\n'
        let mut header_bytes = Vec::new();
        while let Ok(b) = io.read_u8() {
            if b == b'\n' {
                break;
            }
            header_bytes.push(b);
        }

        let header_str = match std::str::from_utf8(&header_bytes) {
            Ok(s) => s,
            Err(_) => {
                ctx.io = Some(io);
                return Err(RmediaError::InvalidData("Invalid UTF-8 in Y4M header".into()));
            }
        };

        if !header_str.starts_with("YUV4MPEG2") {
            ctx.io = Some(io);
            return Err(RmediaError::InvalidData("Not a Y4M stream".into()));
        }

        let mut width = 0usize;
        let mut height = 0usize;
        let mut fps_num = 25i32;
        let mut fps_den = 1i32;
        let mut pix_fmt = PixelFormat::Yuv420p;

        for token in header_str.split_whitespace().skip(1) {
            if token.is_empty() {
                continue;
            }
            let key = token.chars().next().unwrap();
            let val = &token[1..];
            match key {
                'W' => width = val.parse().unwrap_or(0),
                'H' => height = val.parse().unwrap_or(0),
                'F' => {
                    if let Some((n, d)) = val.split_once(':') {
                        fps_num = n.parse().unwrap_or(25);
                        fps_den = d.parse().unwrap_or(1);
                    }
                }
                'C' => {
                    match val {
                        "420" | "420jpeg" | "420paldv" => pix_fmt = PixelFormat::Yuv420p,
                        "422" => pix_fmt = PixelFormat::Yuv422p,
                        "444" => pix_fmt = PixelFormat::Yuv444p,
                        "mono" => pix_fmt = PixelFormat::Gray8,
                        _ => {}
                    }
                }
                _ => {}
            }
        }

        if width == 0 || height == 0 {
            ctx.io = Some(io);
            return Err(RmediaError::InvalidData("Invalid Y4M dimensions".into()));
        }

        self.frame_size = pix_fmt.image_buffer_size(width, height, 1)?;
        self.frame_count = 0;

        let s_idx = ctx.new_stream(CodecId::RawVideo);
        let s = ctx.stream_mut(s_idx).unwrap();
        s.time_base = Rational::new(fps_den, fps_num);
        s.avg_frame_rate = Rational::new(fps_num, fps_den);
        s.r_frame_rate = s.avg_frame_rate;
        s.codec_context.width = width;
        s.codec_context.height = height;
        s.codec_context.pix_fmt = pix_fmt;

        ctx.io = Some(io);
        Ok(())
    }

    fn read_packet(&mut self, ctx: &mut FormatContext, pkt: &mut Packet) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;

        // Read "FRAME" header line
        let mut frame_tag = Vec::new();
        while let Ok(b) = io.read_u8() {
            if b == b'\n' {
                break;
            }
            frame_tag.push(b);
        }

        if frame_tag.is_empty() {
            return Err(RmediaError::EndOfFile);
        }

        let mut buf = vec![0u8; self.frame_size];
        io.read_exact(&mut buf)?;

        *pkt = Packet::from_slice(&buf);
        pkt.stream_index = 0;
        pkt.set_keyframe(true);
        pkt.pts = self.frame_count;
        pkt.dts = self.frame_count;
        self.frame_count += 1;

        Ok(())
    }

    fn seek(&mut self, ctx: &mut FormatContext, _stream_idx: i32, timestamp: i64, _flags: u32) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;
        let bytes_per_frame = (6 + self.frame_size) as u64; // "FRAME\n" + data
        let target = timestamp.max(0) as u64 * bytes_per_frame;
        io.seek(SeekFrom::Start(target))?;
        self.frame_count = timestamp.max(0);
        Ok(())
    }
}

/// Y4M Muxer.
#[derive(Debug, Clone, Default)]
pub struct Y4mMuxer;

impl Muxer for Y4mMuxer {
    fn name(&self) -> &'static str {
        "yuv4mpegpipe"
    }

    fn write_header(&mut self, ctx: &mut FormatContext) -> Result<()> {
        let s = ctx.stream(0).ok_or_else(|| RmediaError::InvalidArgument("Y4M requires 1 video stream".into()))?;

        let width = s.codec_context.width.max(16);
        let height = s.codec_context.height.max(16);
        let fps_num = s.avg_frame_rate.num.max(1);
        let fps_den = s.avg_frame_rate.den.max(1);

        let colorspace = match s.codec_context.pix_fmt {
            PixelFormat::Yuv422p => "422",
            PixelFormat::Yuv444p => "444",
            PixelFormat::Gray8 => "mono",
            _ => "420jpeg",
        };

        let header = format!("YUV4MPEG2 W{} H{} F{}:{} Ip C{}\n", width, height, fps_num, fps_den, colorspace);
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;
        io.write_all(header.as_bytes())?;
        Ok(())
    }

    fn write_packet(&mut self, ctx: &mut FormatContext, pkt: &mut Packet) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;
        io.write_all(b"FRAME\n")?;
        io.write_all(pkt.data())?;
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
    fn test_y4m_roundtrip() {
        let mut out_ctx = FormatContext::new();
        out_ctx.io = Some(crate::io::io_context::IoContext::new_memory());
        let s_idx = out_ctx.new_stream(CodecId::RawVideo);
        out_ctx.stream_mut(s_idx).unwrap().codec_context.width = 16;
        out_ctx.stream_mut(s_idx).unwrap().codec_context.height = 16;
        out_ctx.stream_mut(s_idx).unwrap().codec_context.pix_fmt = PixelFormat::Yuv420p;

        let mut mux = Y4mMuxer;
        mux.write_header(&mut out_ctx).unwrap();

        let frame_bytes = PixelFormat::Yuv420p.image_buffer_size(16, 16, 1).unwrap();
        let dummy = vec![128u8; frame_bytes];
        let mut pkt = Packet::from_slice(&dummy);
        mux.write_packet(&mut out_ctx, &mut pkt).unwrap();
        mux.write_trailer(&mut out_ctx).unwrap();

        let bytes = out_ctx.io.unwrap().into_inner_vec().unwrap();

        let mut in_ctx = FormatContext::new();
        in_ctx.io = Some(crate::io::io_context::IoContext::from_vec(bytes));
        let mut demux = Y4mDemuxer::default();
        demux.read_header(&mut in_ctx).unwrap();

        assert_eq!(in_ctx.streams[0].codec_context.width, 16);
        assert_eq!(in_ctx.streams[0].codec_context.height, 16);

        let mut read_pkt = Packet::new();
        demux.read_packet(&mut in_ctx, &mut read_pkt).unwrap();
        assert_eq!(read_pkt.size(), frame_bytes);
    }
}

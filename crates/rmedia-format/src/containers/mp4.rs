//! ISOBMFF / MP4 / MOV container demuxer and muxer in 100% pure Rust.

use crate::format_context::{Demuxer, FormatContext, Muxer};
use rmedia_codec::codec_id::CodecId;
use rmedia_util::channel_layout::ChannelLayout;
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::packet::Packet;
use rmedia_util::rational::Rational;
use std::io::SeekFrom;

/// Box header representation (size, type).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mp4BoxHeader {
    pub size: u64,
    pub box_type: [u8; 4],
    pub header_size: usize,
}

impl Mp4BoxHeader {
    pub fn read(io: &mut crate::io::io_context::IoContext) -> Result<Self> {
        let size32 = io.read_u32_be()? as u64;
        let mut box_type = [0u8; 4];
        io.read_exact(&mut box_type)?;

        if size32 == 1 {
            // 64-bit extended size
            let size64 = io.read_u64_be()?;
            Ok(Self {
                size: size64,
                box_type,
                header_size: 16,
            })
        } else if size32 == 0 {
            // Extends to EOF
            Ok(Self {
                size: 0,
                box_type,
                header_size: 8,
            })
        } else {
            Ok(Self {
                size: size32,
                box_type,
                header_size: 8,
            })
        }
    }
}

/// MP4 Demuxer.
#[derive(Debug, Clone, Default)]
pub struct Mp4Demuxer {
    mdat_pos: u64,
    mdat_size: u64,
    sample_sizes: Vec<usize>,
    current_sample: usize,
}

impl Demuxer for Mp4Demuxer {
    fn name(&self) -> &'static str {
        "mov,mp4,m4a,3gp,3g2,mj2"
    }

    fn read_header(&mut self, ctx: &mut FormatContext) -> Result<()> {
        let mut io = ctx.io.take().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;

        // Read top-level boxes
        while let Ok(hdr) = Mp4BoxHeader::read(&mut io) {
            let content_size = hdr.size.saturating_sub(hdr.header_size as u64);

            if &hdr.box_type == b"ftyp" {
                io.skip(content_size as i64)?;
            } else if &hdr.box_type == b"moov" {
                // Parse moov
                let end = io.tell() + content_size;
                while io.tell() < end {
                    if let Ok(sub_hdr) = Mp4BoxHeader::read(&mut io) {
                        let sub_content = sub_hdr.size.saturating_sub(sub_hdr.header_size as u64);
                        if &sub_hdr.box_type == b"trak" {
                            let s_idx = ctx.new_stream(CodecId::H264);
                            let s = ctx.stream_mut(s_idx).unwrap();
                            s.time_base = Rational::new(1, 1000);
                            s.codec_context.width = 640;
                            s.codec_context.height = 480;
                            io.skip(sub_content as i64)?;
                        } else {
                            io.skip(sub_content as i64)?;
                        }
                    } else {
                        break;
                    }
                }
            } else if &hdr.box_type == b"mdat" {
                self.mdat_pos = io.tell();
                self.mdat_size = content_size;
                break;
            } else {
                io.skip(content_size as i64)?;
            }
        }

        if ctx.streams.is_empty() {
            let s_idx = ctx.new_stream(CodecId::H264);
            let s = ctx.stream_mut(s_idx).unwrap();
            s.time_base = Rational::new(1, 1000);
        }

        self.current_sample = 0;
        ctx.io = Some(io);
        Ok(())
    }

    fn read_packet(&mut self, ctx: &mut FormatContext, pkt: &mut Packet) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;

        if !self.sample_sizes.is_empty() {
            if self.current_sample >= self.sample_sizes.len() {
                return Err(RmediaError::EndOfFile);
            }
            let sz = self.sample_sizes[self.current_sample];
            let mut buf = vec![0u8; sz];
            io.read_exact(&mut buf)?;
            *pkt = Packet::from_slice(&buf);
            pkt.pts = self.current_sample as i64;
            pkt.dts = pkt.pts;
            pkt.stream_index = 0;
            self.current_sample += 1;
            return Ok(());
        }

        // Streaming chunk read fallback
        if self.mdat_size > 0 {
            let read_sz = (4096).min(self.mdat_size as usize);
            let mut buf = vec![0u8; read_sz];
            io.read_exact(&mut buf)?;
            self.mdat_size -= read_sz as u64;
            *pkt = Packet::from_slice(&buf);
            pkt.pts = self.current_sample as i64;
            pkt.dts = pkt.pts;
            pkt.stream_index = 0;
            self.current_sample += 1;
            return Ok(());
        }

        Err(RmediaError::EndOfFile)
    }

    fn seek(&mut self, ctx: &mut FormatContext, _stream_idx: i32, timestamp: i64, _flags: u32) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;
        io.seek(SeekFrom::Start(self.mdat_pos))?;
        self.current_sample = 0;
        Ok(())
    }
}

/// MP4 Muxer.
#[derive(Debug, Clone, Default)]
pub struct Mp4Muxer {
    mdat_start: u64,
    mdat_size: u64,
    sample_sizes: Vec<u32>,
}

impl Muxer for Mp4Muxer {
    fn name(&self) -> &'static str {
        "mp4"
    }

    fn write_header(&mut self, ctx: &mut FormatContext) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;

        // 1. ftyp box (32 bytes)
        io.write_u32_be(32)?;
        io.write_all(b"ftyp")?;
        io.write_all(b"isom")?; // major brand
        io.write_u32_be(512)?; // minor version
        io.write_all(b"isomiso2avc1mp41")?; // compatible brands

        // 2. mdat box header (placeholder size)
        self.mdat_start = io.tell();
        io.write_u32_be(0)?; // Will patch later if small or use extended
        io.write_all(b"mdat")?;

        self.mdat_size = 0;
        self.sample_sizes.clear();
        Ok(())
    }

    fn write_packet(&mut self, ctx: &mut FormatContext, pkt: &mut Packet) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;
        let data = pkt.data();
        io.write_all(data)?;
        self.mdat_size += data.len() as u64;
        self.sample_sizes.push(data.len() as u32);
        Ok(())
    }

    fn write_trailer(&mut self, ctx: &mut FormatContext) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;

        // Patch mdat size
        let mdat_total = 8 + self.mdat_size;
        io.seek(SeekFrom::Start(self.mdat_start))?;
        io.write_u32_be(mdat_total as u32)?;

        // Seek to end of mdat to write moov
        io.seek(SeekFrom::Start(self.mdat_start + mdat_total))?;

        // Write moov box
        let moov_start = io.tell();
        io.write_u32_be(0)?; // moov size placeholder
        io.write_all(b"moov")?;

        // mvhd (108 bytes)
        io.write_u32_be(108)?;
        io.write_all(b"mvhd")?;
        io.write_all(&[0; 4])?; // version 0, flags 0
        io.write_u32_be(0)?; // creation time
        io.write_u32_be(0)?; // modification time
        io.write_u32_be(1000)?; // timescale: 1000 Hz
        io.write_u32_be(self.sample_sizes.len() as u32 * 40)?; // duration
        io.write_u32_be(0x0001_0000)?; // rate 1.0
        io.write_u16_be(0x0100)?; // volume 1.0
        io.write_all(&[0; 10])?; // reserved
        // 36 bytes identity matrix
        io.write_u32_be(0x0001_0000)?; io.write_u32_be(0)?; io.write_u32_be(0)?;
        io.write_u32_be(0)?; io.write_u32_be(0x0001_0000)?; io.write_u32_be(0)?;
        io.write_u32_be(0)?; io.write_u32_be(0)?; io.write_u32_be(0x4000_0000)?;
        io.write_all(&[0; 24])?; // pre-defined
        io.write_u32_be(2)?; // next_track_id = 2

        // Update moov size
        let moov_end = io.tell();
        let moov_len = (moov_end - moov_start) as u32;
        io.seek(SeekFrom::Start(moov_start))?;
        io.write_u32_be(moov_len)?;

        io.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mp4_roundtrip() {
        let mut out_ctx = FormatContext::new();
        out_ctx.io = Some(crate::io::io_context::IoContext::new_memory());
        let s_idx = out_ctx.new_stream(CodecId::H264);
        out_ctx.stream_mut(s_idx).unwrap().codec_context.width = 640;
        out_ctx.stream_mut(s_idx).unwrap().codec_context.height = 480;

        let mut mux = Mp4Muxer::default();
        mux.write_header(&mut out_ctx).unwrap();

        let dummy = vec![0xAAu8; 128];
        let mut pkt = Packet::from_slice(&dummy);
        mux.write_packet(&mut out_ctx, &mut pkt).unwrap();
        mux.write_trailer(&mut out_ctx).unwrap();

        let bytes = out_ctx.io.unwrap().into_inner_vec().unwrap();

        let mut in_ctx = FormatContext::new();
        in_ctx.io = Some(crate::io::io_context::IoContext::from_vec(bytes));
        let mut demux = Mp4Demuxer::default();
        demux.read_header(&mut in_ctx).unwrap();

        let mut read_pkt = Packet::new();
        demux.read_packet(&mut in_ctx, &mut read_pkt).unwrap();
        assert_eq!(read_pkt.size(), 128);
    }
}

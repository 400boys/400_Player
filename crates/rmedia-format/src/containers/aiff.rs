//! Audio Interchange File Format (AIFF / AIFC) demuxer and muxer in 100% pure Rust.

use crate::format_context::{Demuxer, FormatContext, Muxer};
use rmedia_codec::codec_id::CodecId;
use rmedia_util::channel_layout::ChannelLayout;
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::packet::Packet;
use rmedia_util::rational::Rational;
use rmedia_util::samplefmt::SampleFormat;
use std::io::SeekFrom;

/// AIFF Demuxer.
#[derive(Debug, Clone, Default)]
pub struct AiffDemuxer {
    ssnd_start: u64,
    ssnd_size: u64,
    bytes_read: u64,
    channels: usize,
    bytes_per_sample: usize,
}

impl Demuxer for AiffDemuxer {
    fn name(&self) -> &'static str {
        "aiff"
    }

    fn read_header(&mut self, ctx: &mut FormatContext) -> Result<()> {
        let mut io = ctx.io.take().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;

        // 12-byte FORM AIFF header
        let mut form = [0u8; 4];
        io.read_exact(&mut form)?;
        if &form != b"FORM" {
            ctx.io = Some(io);
            return Err(RmediaError::InvalidData("Invalid FORM header".into()));
        }

        let _file_size = io.read_u32_be()?;
        let mut aiff = [0u8; 4];
        io.read_exact(&mut aiff)?;
        if &aiff != b"AIFF" && &aiff != b"AIFC" {
            ctx.io = Some(io);
            return Err(RmediaError::InvalidData("Not an AIFF file".into()));
        }

        let mut channels = 2u16;
        let mut bits_per_sample = 16u16;
        let mut sample_rate = 44100u32;

        while let Ok(chunk_id) = (|| -> Result<[u8; 4]> {
            let mut id = [0u8; 4];
            io.read_exact(&mut id)?;
            Ok(id)
        })() {
            let chunk_size = io.read_u32_be()? as u64;

            if &chunk_id == b"COMM" {
                channels = io.read_u16_be()?;
                let _num_frames = io.read_u32_be()?;
                bits_per_sample = io.read_u16_be()?;
                // 10-byte 80-bit IEEE 754 float sample rate
                let mut srate_bytes = [0u8; 10];
                io.read_exact(&mut srate_bytes)?;
                let exp = ((srate_bytes[0] as i32 & 0x7F) << 8) | (srate_bytes[1] as i32);
                let mantissa = u64::from_be_bytes(srate_bytes[2..10].try_into().unwrap());
                sample_rate = if exp > 16382 {
                    (mantissa >> (16382 + 63 - exp)) as u32
                } else {
                    44100
                };
            } else if &chunk_id == b"SSND" {
                let _offset = io.read_u32_be()?;
                let _block_size = io.read_u32_be()?;
                self.ssnd_start = io.tell();
                self.ssnd_size = chunk_size.saturating_sub(8);
                break;
            } else {
                let padded = (chunk_size + 1) & !1;
                io.skip(padded as i64)?;
            }
        }

        self.channels = channels as usize;
        self.bytes_per_sample = (bits_per_sample / 8) as usize;
        self.bytes_read = 0;

        let s_idx = ctx.new_stream(CodecId::PcmS16be);
        let s = ctx.stream_mut(s_idx).unwrap();
        s.time_base = Rational::new(1, sample_rate as i32);
        s.codec_context.sample_rate = sample_rate;
        s.codec_context.ch_layout = ChannelLayout::from_channels(channels as u32);
        s.codec_context.sample_fmt = SampleFormat::S16;

        ctx.io = Some(io);
        Ok(())
    }

    fn read_packet(&mut self, ctx: &mut FormatContext, pkt: &mut Packet) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;

        if self.bytes_read >= self.ssnd_size {
            return Err(RmediaError::EndOfFile);
        }

        let frame_bytes = self.bytes_per_sample * self.channels;
        let read_len = (1024 * frame_bytes).min((self.ssnd_size - self.bytes_read) as usize);

        let mut buf = vec![0u8; read_len];
        io.read_exact(&mut buf)?;
        self.bytes_read += read_len as u64;

        *pkt = Packet::from_slice(&buf);
        pkt.stream_index = 0;
        pkt.pts = (self.bytes_read / frame_bytes as u64) as i64;
        pkt.dts = pkt.pts;
        pkt.set_keyframe(true);

        Ok(())
    }

    fn seek(&mut self, ctx: &mut FormatContext, _stream_idx: i32, timestamp: i64, _flags: u32) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;
        let frame_bytes = self.bytes_per_sample * self.channels;
        let byte_offset = (timestamp as u64) * (frame_bytes as u64);
        let target = (self.ssnd_start + byte_offset).min(self.ssnd_start + self.ssnd_size);
        io.seek(SeekFrom::Start(target))?;
        self.bytes_read = target - self.ssnd_start;
        Ok(())
    }
}

/// AIFF Muxer.
#[derive(Debug, Clone, Default)]
pub struct AiffMuxer {
    ssnd_size: u32,
}

impl Muxer for AiffMuxer {
    fn name(&self) -> &'static str {
        "aiff"
    }

    fn write_header(&mut self, ctx: &mut FormatContext) -> Result<()> {
        let s = ctx.stream(0).ok_or_else(|| RmediaError::InvalidArgument("AIFF requires 1 audio stream".into()))?;
        let channels = s.codec_context.ch_layout.nb_channels.max(1) as u16;
        let sample_rate = s.codec_context.sample_rate.max(8000);

        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;

        // 1. FORM Header
        io.write_all(b"FORM")?;
        io.write_u32_be(0)?; // file size placeholder
        io.write_all(b"AIFF")?;

        // 2. COMM Chunk (18 bytes)
        io.write_all(b"COMM")?;
        io.write_u32_be(18)?;
        io.write_u16_be(channels)?;
        io.write_u32_be(0)?; // numSampleFrames placeholder
        io.write_u16_be(16)?; // 16 bits per sample

        // 80-bit float for 44100 / 48000 Hz
        let (exp, mantissa) = if sample_rate == 48000 {
            (0x400E, 0xBB80_0000_0000_0000u64)
        } else {
            (0x400E, 0xAC44_0000_0000_0000u64)
        };
        io.write_u16_be(exp)?;
        io.write_u64_be(mantissa)?;

        // 3. SSND Chunk Header
        io.write_all(b"SSND")?;
        io.write_u32_be(0)?; // SSND size placeholder
        io.write_u32_be(0)?; // offset 0
        io.write_u32_be(0)?; // block size 0

        self.ssnd_size = 8;
        Ok(())
    }

    fn write_packet(&mut self, ctx: &mut FormatContext, pkt: &mut Packet) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;
        let data = pkt.data();
        io.write_all(data)?;
        self.ssnd_size += data.len() as u32;
        Ok(())
    }

    fn write_trailer(&mut self, ctx: &mut FormatContext) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;

        // Patch FORM size
        let total = io.seek(SeekFrom::End(0))?;
        io.seek(SeekFrom::Start(4))?;
        io.write_u32_be((total - 8) as u32)?;

        // Patch SSND size (starts at offset 42)
        io.seek(SeekFrom::Start(42))?;
        io.write_u32_be(self.ssnd_size)?;

        io.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aiff_roundtrip() {
        let mut out_ctx = FormatContext::new();
        out_ctx.io = Some(crate::io::io_context::IoContext::new_memory());
        let s_idx = out_ctx.new_stream(CodecId::PcmS16be);
        out_ctx.stream_mut(s_idx).unwrap().codec_context.sample_rate = 44100;
        out_ctx.stream_mut(s_idx).unwrap().codec_context.ch_layout = ChannelLayout::STEREO;

        let mut mux = AiffMuxer::default();
        mux.write_header(&mut out_ctx).unwrap();

        let dummy = vec![0x12u8; 1024];
        let mut pkt = Packet::from_slice(&dummy);
        mux.write_packet(&mut out_ctx, &mut pkt).unwrap();
        mux.write_trailer(&mut out_ctx).unwrap();

        let bytes = out_ctx.io.unwrap().into_inner_vec().unwrap();

        let mut in_ctx = FormatContext::new();
        in_ctx.io = Some(crate::io::io_context::IoContext::from_vec(bytes));
        let mut demux = AiffDemuxer::default();
        demux.read_header(&mut in_ctx).unwrap();

        let mut read_pkt = Packet::new();
        demux.read_packet(&mut in_ctx, &mut read_pkt).unwrap();
        assert_eq!(read_pkt.size(), 1024);
    }
}

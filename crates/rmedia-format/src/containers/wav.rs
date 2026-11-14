//! WAV / RIFF container demuxer and muxer in 100% pure Rust.

use crate::format_context::{Demuxer, FormatContext, Muxer};
use rmedia_codec::codec_id::CodecId;
use rmedia_util::channel_layout::ChannelLayout;
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::packet::Packet;
use rmedia_util::rational::Rational;
use rmedia_util::samplefmt::SampleFormat;
use std::io::SeekFrom;

/// WAV Demuxer.
#[derive(Debug, Clone, Default)]
pub struct WavDemuxer {
    data_start: u64,
    data_size: u64,
    bytes_read: u64,
    bytes_per_sample: usize,
    channels: usize,
}

impl Demuxer for WavDemuxer {
    fn name(&self) -> &'static str {
        "wav"
    }

    fn read_header(&mut self, ctx: &mut FormatContext) -> Result<()> {
        let mut io = ctx.io.take().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;

        // 12-byte RIFF header
        let mut riff_magic = [0u8; 4];
        io.read_exact(&mut riff_magic)?;
        if &riff_magic != b"RIFF" {
            ctx.io = Some(io);
            return Err(RmediaError::InvalidData("Invalid RIFF header".into()));
        }

        let _riff_size = io.read_u32_le()?;
        let mut wave_magic = [0u8; 4];
        io.read_exact(&mut wave_magic)?;
        if &wave_magic != b"WAVE" {
            ctx.io = Some(io);
            return Err(RmediaError::InvalidData("Invalid WAVE header".into()));
        }

        let mut sample_rate = 44100u32;
        let mut channels = 2u16;
        let mut bits_per_sample = 16u16;
        let mut audio_format = 1u16; // 1 = PCM, 3 = IEEE float

        // Read chunks
        while let Ok(chunk_id) = (|| -> Result<[u8; 4]> {
            let mut id = [0u8; 4];
            io.read_exact(&mut id)?;
            Ok(id)
        })() {
            let chunk_size = io.read_u32_le()? as u64;

            if &chunk_id == b"fmt " {
                audio_format = io.read_u16_le()?;
                channels = io.read_u16_le()?;
                sample_rate = io.read_u32_le()?;
                let _byte_rate = io.read_u32_le()?;
                let _block_align = io.read_u16_le()?;
                bits_per_sample = io.read_u16_le()?;

                if chunk_size > 16 {
                    io.skip((chunk_size - 16) as i64)?;
                }
            } else if &chunk_id == b"data" {
                self.data_start = io.tell();
                self.data_size = chunk_size;
                break;
            } else {
                io.skip(chunk_size as i64)?;
            }
        }

        let codec_id = match (audio_format, bits_per_sample) {
            (1, 8) => CodecId::PcmU8,
            (1, 16) => CodecId::PcmS16le,
            (1, 24) => CodecId::PcmS24le,
            (1, 32) => CodecId::PcmS32le,
            (3, 32) => CodecId::PcmF32le,
            (3, 64) => CodecId::PcmF64le,
            _ => CodecId::PcmS16le,
        };

        self.bytes_per_sample = (bits_per_sample / 8) as usize;
        self.channels = channels as usize;
        self.bytes_read = 0;

        let stream_idx = ctx.new_stream(codec_id);
        let stream = ctx.stream_mut(stream_idx).unwrap();
        stream.time_base = Rational::new(1, sample_rate as i32);
        stream.codec_context.sample_rate = sample_rate;
        stream.codec_context.ch_layout = ChannelLayout::from_channels(channels as u32);
        stream.codec_context.sample_fmt = match codec_id {
            CodecId::PcmU8 => SampleFormat::U8,
            CodecId::PcmS16le => SampleFormat::S16,
            CodecId::PcmS24le | CodecId::PcmS32le => SampleFormat::S32,
            CodecId::PcmF32le => SampleFormat::Flt,
            CodecId::PcmF64le => SampleFormat::Dbl,
            _ => SampleFormat::S16,
        };

        ctx.io = Some(io);
        Ok(())
    }

    fn read_packet(&mut self, ctx: &mut FormatContext, pkt: &mut Packet) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;

        if self.bytes_read >= self.data_size {
            return Err(RmediaError::EndOfFile);
        }

        // Read a chunk of 1024 audio frames
        let frame_bytes = self.bytes_per_sample * self.channels;
        let read_len = (1024 * frame_bytes).min((self.data_size - self.bytes_read) as usize);

        let mut buf = vec![0u8; read_len];
        io.read_exact(&mut buf)?;
        self.bytes_read += read_len as u64;

        *pkt = Packet::from_slice(&buf);
        pkt.stream_index = 0;
        pkt.set_keyframe(true);
        pkt.pts = (self.bytes_read / frame_bytes as u64) as i64;
        pkt.dts = pkt.pts;
        Ok(())
    }

    fn seek(&mut self, ctx: &mut FormatContext, _stream_idx: i32, timestamp: i64, _flags: u32) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;
        let frame_bytes = self.bytes_per_sample * self.channels;
        let byte_offset = (timestamp as u64) * (frame_bytes as u64);
        let target = (self.data_start + byte_offset).min(self.data_start + self.data_size);
        io.seek(SeekFrom::Start(target))?;
        self.bytes_read = target - self.data_start;
        Ok(())
    }
}

/// WAV Muxer.
#[derive(Debug, Clone, Default)]
pub struct WavMuxer {
    data_size: u32,
}

impl Muxer for WavMuxer {
    fn name(&self) -> &'static str {
        "wav"
    }

    fn write_header(&mut self, ctx: &mut FormatContext) -> Result<()> {
        let stream = ctx.stream(0).ok_or_else(|| RmediaError::InvalidArgument("WAV requires 1 audio stream".into()))?;

        let sample_rate = stream.codec_context.sample_rate.max(8000);
        let channels = stream.codec_context.ch_layout.nb_channels.max(1) as u16;
        let bits_per_sample: u16 = match stream.codec_context.sample_fmt {
            SampleFormat::U8 | SampleFormat::U8P => 8,
            SampleFormat::S16 | SampleFormat::S16P => 16,
            SampleFormat::S32 | SampleFormat::S32P | SampleFormat::Flt | SampleFormat::FltP => 32,
            SampleFormat::Dbl | SampleFormat::DblP => 64,
            _ => 16,
        };

        let block_align = channels * (bits_per_sample / 8);
        let byte_rate = sample_rate * (block_align as u32);
        let is_float = matches!(stream.codec_context.sample_fmt, SampleFormat::Flt | SampleFormat::FltP | SampleFormat::Dbl | SampleFormat::DblP);
        let audio_format: u16 = if is_float { 3 } else { 1 };

        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;

        // RIFF Header
        io.write_all(b"RIFF")?;
        io.write_u32_le(0)?; // Placeholder for RIFF size
        io.write_all(b"WAVE")?;

        // fmt Chunk (16 bytes)
        io.write_all(b"fmt ")?;
        io.write_u32_le(16)?;
        io.write_u16_le(audio_format)?;
        io.write_u16_le(channels)?;
        io.write_u32_le(sample_rate)?;
        io.write_u32_le(byte_rate)?;
        io.write_u16_le(block_align)?;
        io.write_u16_le(bits_per_sample)?;

        // data Chunk Header
        io.write_all(b"data")?;
        io.write_u32_le(0)?; // Placeholder for data size

        self.data_size = 0;
        Ok(())
    }

    fn write_packet(&mut self, ctx: &mut FormatContext, pkt: &mut Packet) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;
        let data = pkt.data();
        io.write_all(data)?;
        self.data_size += data.len() as u32;
        Ok(())
    }

    fn write_trailer(&mut self, ctx: &mut FormatContext) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;

        // Patch RIFF size (file_size - 8)
        let riff_size = 36 + self.data_size;
        io.seek(SeekFrom::Start(4))?;
        io.write_u32_le(riff_size)?;

        // Patch data size
        io.seek(SeekFrom::Start(40))?;
        io.write_u32_le(self.data_size)?;
        io.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wav_mux_demux_roundtrip() {
        let mut out_ctx = FormatContext::new();
        out_ctx.io = Some(crate::io::io_context::IoContext::new_memory());
        let s_idx = out_ctx.new_stream(CodecId::PcmS16le);
        out_ctx.stream_mut(s_idx).unwrap().codec_context.sample_rate = 48000;
        out_ctx.stream_mut(s_idx).unwrap().codec_context.ch_layout = ChannelLayout::STEREO;
        out_ctx.stream_mut(s_idx).unwrap().codec_context.sample_fmt = SampleFormat::S16;

        let mut muxer = WavMuxer::default();
        muxer.write_header(&mut out_ctx).unwrap();

        let dummy_samples = vec![0x12u8; 1024];
        let mut pkt = Packet::from_slice(&dummy_samples);
        muxer.write_packet(&mut out_ctx, &mut pkt).unwrap();
        muxer.write_trailer(&mut out_ctx).unwrap();

        let bytes = out_ctx.io.unwrap().into_inner_vec().unwrap();
        assert!(bytes.len() > 44);

        let mut in_ctx = FormatContext::new();
        in_ctx.io = Some(crate::io::io_context::IoContext::from_vec(bytes));

        let mut demuxer = WavDemuxer::default();
        demuxer.read_header(&mut in_ctx).unwrap();
        assert_eq!(in_ctx.streams.len(), 1);
        assert_eq!(in_ctx.streams[0].codec_context.sample_rate, 48000);

        let mut read_pkt = Packet::new();
        demuxer.read_packet(&mut in_ctx, &mut read_pkt).unwrap();
        assert_eq!(read_pkt.size(), 1024);
    }
}

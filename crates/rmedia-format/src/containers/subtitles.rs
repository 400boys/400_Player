//! Subtitle file format demuxers and muxers (SRT, ASS, WebVTT).

use crate::format_context::{Demuxer, FormatContext, Muxer};
use rmedia_codec::codec_id::CodecId;
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::packet::Packet;
use rmedia_util::rational::Rational;

/// Subtitle text file Demuxer (SRT / WebVTT / ASS).
#[derive(Debug, Clone)]
pub struct SubtitleDemuxer {
    codec_id: CodecId,
    read_done: bool,
}

impl SubtitleDemuxer {
    pub fn new(codec_id: CodecId) -> Self {
        Self { codec_id, read_done: false }
    }
}

impl Demuxer for SubtitleDemuxer {
    fn name(&self) -> &'static str {
        match self.codec_id {
            CodecId::Srt => "srt",
            CodecId::Ass => "ass",
            CodecId::WebVtt => "webvtt",
            _ => "subtitles",
        }
    }

    fn read_header(&mut self, ctx: &mut FormatContext) -> Result<()> {
        let s_idx = ctx.new_stream(self.codec_id);
        let s = ctx.stream_mut(s_idx).unwrap();
        s.time_base = Rational::new(1, 1000);
        self.read_done = false;
        Ok(())
    }

    fn read_packet(&mut self, ctx: &mut FormatContext, pkt: &mut Packet) -> Result<()> {
        if self.read_done {
            return Err(RmediaError::EndOfFile);
        }

        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;
        let mut buf = Vec::new();
        let mut tmp = [0u8; 1024];
        while let Ok(n) = io.read(&mut tmp) {
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n]);
        }

        if buf.is_empty() {
            return Err(RmediaError::EndOfFile);
        }

        *pkt = Packet::from_slice(&buf);
        pkt.stream_index = 0;
        pkt.pts = 0;
        pkt.set_keyframe(true);
        self.read_done = true;
        Ok(())
    }

    fn seek(&mut self, _ctx: &mut FormatContext, _stream_idx: i32, _timestamp: i64, _flags: u32) -> Result<()> {
        self.read_done = false;
        Ok(())
    }
}

/// Subtitle text file Muxer.
#[derive(Debug, Clone)]
pub struct SubtitleMuxer {
    codec_id: CodecId,
}

impl SubtitleMuxer {
    pub fn new(codec_id: CodecId) -> Self {
        Self { codec_id }
    }
}

impl Muxer for SubtitleMuxer {
    fn name(&self) -> &'static str {
        match self.codec_id {
            CodecId::Srt => "srt",
            CodecId::Ass => "ass",
            CodecId::WebVtt => "webvtt",
            _ => "subtitles",
        }
    }

    fn write_header(&mut self, _ctx: &mut FormatContext) -> Result<()> {
        Ok(())
    }

    fn write_packet(&mut self, ctx: &mut FormatContext, pkt: &mut Packet) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;
        io.write_all(pkt.data())?;
        Ok(())
    }

    fn write_trailer(&mut self, ctx: &mut FormatContext) -> Result<()> {
        let io = ctx.io.as_mut().ok_or_else(|| RmediaError::Io("No IoContext".into()))?;
        io.flush()?;
        Ok(())
    }
}

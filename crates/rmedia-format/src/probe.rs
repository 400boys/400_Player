//! Automatic container format probing and demuxer/muxer discovery.

use crate::containers::*;
use crate::format_context::{Demuxer, Muxer};
use rmedia_codec::codec_id::CodecId;

/// Probe byte slice to determine container format name.
pub fn probe_input_format(buf: &[u8]) -> Option<&'static str> {
    if buf.len() >= 12 && &buf[0..4] == b"RIFF" && &buf[8..12] == b"WAVE" {
        return Some("wav");
    }
    if buf.len() >= 12 && &buf[0..4] == b"RIFF" && &buf[8..12] == b"AVI " {
        return Some("avi");
    }
    if buf.len() >= 12 && &buf[0..4] == b"FORM" && (&buf[8..12] == b"AIFF" || &buf[8..12] == b"AIFC") {
        return Some("aiff");
    }
    if buf.len() >= 4 && &buf[0..4] == b"OggS" {
        return Some("ogg");
    }
    if buf.len() >= 9 && &buf[0..9] == b"YUV4MPEG2" {
        return Some("yuv4mpegpipe");
    }
    if buf.len() >= 4 && &buf[0..4] == b"DKIF" {
        return Some("ivf");
    }
    if buf.len() >= 8 && (&buf[4..8] == b"ftyp" || &buf[4..8] == b"moov" || &buf[4..8] == b"mdat") {
        return Some("mp4");
    }
    if buf.len() >= 4 && buf[0] == 0x1A && buf[1] == 0x45 && buf[2] == 0xDF && buf[3] == 0xA3 {
        return Some("matroska");
    }
    if buf.len() >= 3 && &buf[0..3] == b"FLV" {
        return Some("flv");
    }
    if buf.len() >= 188 && buf[0] == 0x47 {
        return Some("mpegts");
    }
    if buf.len() >= 8 && &buf[0..8] == b"\x89PNG\r\n\x1a\n" {
        return Some("image2");
    }
    if buf.len() >= 2 && &buf[0..2] == b"BM" {
        return Some("image2");
    }
    if buf.len() >= 2 && buf[0] == 0xFF && buf[1] == 0xD8 {
        return Some("image2");
    }
    if buf.len() >= 6 && &buf[0..6] == b"WEBVTT" {
        return Some("webvtt");
    }
    if let Ok(text) = std::str::from_utf8(&buf[..buf.len().min(128)]) {
        if text.contains("[Script Info]") {
            return Some("ass");
        }
        if text.contains("-->") {
            return Some("srt");
        }
    }
    None
}

/// Instantiate a demuxer for the given format name.
pub fn find_demuxer(format_name: &str) -> Option<Box<dyn Demuxer>> {
    match format_name {
        "wav" => Some(Box::new(WavDemuxer::default())),
        "avi" => Some(Box::new(AviDemuxer::default())),
        "ogg" => Some(Box::new(OggDemuxer::default())),
        "aiff" => Some(Box::new(AiffDemuxer::default())),
        "yuv4mpegpipe" | "y4m" => Some(Box::new(Y4mDemuxer::default())),
        "ivf" => Some(Box::new(IvfDemuxer::default())),
        "mp4" | "mov" | "m4a" | "3gp" => Some(Box::new(Mp4Demuxer::default())),
        "matroska" | "webm" | "mkv" => Some(Box::new(MkvDemuxer::default())),
        "flv" => Some(Box::new(FlvDemuxer::default())),
        "mpegts" | "ts" => Some(Box::new(TsDemuxer::default())),
        "srt" => Some(Box::new(SubtitleDemuxer::new(CodecId::Srt))),
        "ass" | "ssa" => Some(Box::new(SubtitleDemuxer::new(CodecId::Ass))),
        "webvtt" | "vtt" => Some(Box::new(SubtitleDemuxer::new(CodecId::WebVtt))),
        _ => None,
    }
}

/// Instantiate a muxer for the given format name.
pub fn find_muxer(format_name: &str) -> Option<Box<dyn Muxer>> {
    match format_name {
        "wav" => Some(Box::new(WavMuxer::default())),
        "avi" => Some(Box::new(AviMuxer::default())),
        "ogg" => Some(Box::new(OggMuxer::default())),
        "aiff" => Some(Box::new(AiffMuxer::default())),
        "yuv4mpegpipe" | "y4m" => Some(Box::new(Y4mMuxer)),
        "ivf" => Some(Box::new(IvfMuxer::default())),
        "mp4" | "mov" | "m4a" => Some(Box::new(Mp4Muxer::default())),
        "matroska" | "webm" | "mkv" => Some(Box::new(MkvMuxer::default())),
        "flv" => Some(Box::new(FlvMuxer)),
        "mpegts" | "ts" => Some(Box::new(TsMuxer::default())),
        "srt" => Some(Box::new(SubtitleMuxer::new(CodecId::Srt))),
        "ass" | "ssa" => Some(Box::new(SubtitleMuxer::new(CodecId::Ass))),
        "webvtt" | "vtt" => Some(Box::new(SubtitleMuxer::new(CodecId::WebVtt))),
        _ => None,
    }
}

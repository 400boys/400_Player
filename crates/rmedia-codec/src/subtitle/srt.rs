//! SubRip (SRT) subtitle decoder and encoder in 100% pure Rust.

use crate::codec_id::CodecId;
use crate::context::{CodecContext, Decoder, Encoder};
use rmedia_util::buffer::BufferRef;
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::frame::Frame;
use rmedia_util::packet::Packet;

/// Subtitle cue entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtitleCue {
    pub index: u32,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

/// Parse SRT timestamp string "HH:MM:SS,mmm" into milliseconds.
pub fn parse_srt_timestamp(s: &str) -> Option<u64> {
    let parts: Vec<&str> = s.trim().split(&[':', ','][..]).collect();
    if parts.len() != 4 {
        return None;
    }
    let h: u64 = parts[0].parse().ok()?;
    let m: u64 = parts[1].parse().ok()?;
    let sec: u64 = parts[2].parse().ok()?;
    let ms: u64 = parts[3].parse().ok()?;
    Some(h * 3600_000 + m * 60_000 + sec * 1000 + ms)
}

/// Format milliseconds into SRT timestamp string "HH:MM:SS,mmm".
pub fn format_srt_timestamp(ms: u64) -> String {
    let h = ms / 3600_000;
    let rem = ms % 3600_000;
    let m = rem / 60_000;
    let rem = rem % 60_000;
    let sec = rem / 1000;
    let millis = rem % 1000;
    format!("{:02}:{:02}:{:02},{:03}", h, m, sec, millis)
}

/// Parse entire SRT document into a list of subtitle cues.
pub fn parse_srt_document(text: &str) -> Vec<SubtitleCue> {
    let mut cues = Vec::new();
    let blocks: Vec<&str> = text.split("\n\n").collect();

    for block in blocks {
        let lines: Vec<&str> = block.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
        if lines.len() < 2 {
            continue;
        }

        let (idx, time_line_idx) = if let Ok(n) = lines[0].parse::<u32>() {
            (n, 1)
        } else {
            (cues.len() as u32 + 1, 0)
        };

        if time_line_idx >= lines.len() {
            continue;
        }

        let time_line = lines[time_line_idx];
        if let Some((start_str, end_str)) = time_line.split_once("-->") {
            if let (Some(start_ms), Some(end_ms)) = (parse_srt_timestamp(start_str), parse_srt_timestamp(end_str)) {
                let text_lines = &lines[time_line_idx + 1..];
                let text = text_lines.join("\n");
                cues.push(SubtitleCue {
                    index: idx,
                    start_ms,
                    end_ms,
                    text,
                });
            }
        }
    }

    cues
}

/// Format cues into complete SRT document.
pub fn format_srt_document(cues: &[SubtitleCue]) -> String {
    let mut out = String::new();
    for (i, cue) in cues.iter().enumerate() {
        if i > 0 {
            out.push_str("\n\n");
        }
        out.push_str(&format!("{}\n", cue.index));
        out.push_str(&format!("{} --> {}\n", format_srt_timestamp(cue.start_ms), format_srt_timestamp(cue.end_ms)));
        out.push_str(&cue.text);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_srt_parsing_roundtrip() {
        let sample = "1\n00:00:01,000 --> 00:00:04,000\nHello world!\n\n2\n00:00:05,500 --> 00:00:08,250\nSecond line";
        let cues = parse_srt_document(sample);
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].start_ms, 1000);
        assert_eq!(cues[0].end_ms, 4000);
        assert_eq!(cues[0].text, "Hello world!");
        assert_eq!(cues[1].start_ms, 5500);
        assert_eq!(cues[1].end_ms, 8250);

        let formatted = format_srt_document(&cues);
        assert_eq!(formatted, sample);
    }
}

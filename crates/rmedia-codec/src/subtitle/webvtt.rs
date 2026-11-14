//! WebVTT subtitle decoder and encoder in 100% pure Rust.

use rmedia_util::error::{Result, RmediaError};
use super::srt::{format_srt_timestamp, parse_srt_timestamp, SubtitleCue};

pub const WEBVTT_MAGIC: &str = "WEBVTT";

/// Parse WebVTT timestamp (either "MM:SS.mmm" or "HH:MM:SS.mmm") into milliseconds.
pub fn parse_webvtt_timestamp(s: &str) -> Option<u64> {
    let s = s.trim().replace(',', ".");
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() == 2 {
        let m: u64 = parts[0].parse().ok()?;
        let (sec_str, ms_str) = parts[1].split_once('.')?;
        let sec: u64 = sec_str.parse().ok()?;
        let ms: u64 = ms_str.parse().ok()?;
        Some(m * 60_000 + sec * 1000 + ms)
    } else if parts.len() == 3 {
        let h: u64 = parts[0].parse().ok()?;
        let m: u64 = parts[1].parse().ok()?;
        let (sec_str, ms_str) = parts[2].split_once('.')?;
        let sec: u64 = sec_str.parse().ok()?;
        let ms: u64 = ms_str.parse().ok()?;
        Some(h * 3600_000 + m * 60_000 + sec * 1000 + ms)
    } else {
        None
    }
}

/// Format milliseconds into WebVTT timestamp "HH:MM:SS.mmm".
pub fn format_webvtt_timestamp(ms: u64) -> String {
    format_srt_timestamp(ms).replace(',', ".")
}

/// Parse entire WebVTT document.
pub fn parse_webvtt_document(text: &str) -> Result<Vec<SubtitleCue>> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() || !lines[0].trim().starts_with(WEBVTT_MAGIC) {
        return Err(RmediaError::InvalidData("Invalid WebVTT header".into()));
    }

    let mut cues = Vec::new();
    let blocks: Vec<&str> = text.split("\n\n").collect();

    for block in blocks {
        let blk_lines: Vec<&str> = block.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
        if blk_lines.is_empty() || blk_lines[0].starts_with(WEBVTT_MAGIC) {
            continue;
        }

        let time_idx = if blk_lines[0].contains("-->") { 0 } else { 1 };
        if time_idx >= blk_lines.len() {
            continue;
        }

        if let Some((start_str, end_str)) = blk_lines[time_idx].split_once("-->") {
            let clean_end = end_str.split_whitespace().next().unwrap_or("");
            if let (Some(s_ms), Some(e_ms)) = (parse_webvtt_timestamp(start_str), parse_webvtt_timestamp(clean_end)) {
                let text = blk_lines[time_idx + 1..].join("\n");
                cues.push(SubtitleCue {
                    index: cues.len() as u32 + 1,
                    start_ms: s_ms,
                    end_ms: e_ms,
                    text,
                });
            }
        }
    }

    Ok(cues)
}

/// Format cues into WebVTT document.
pub fn format_webvtt_document(cues: &[SubtitleCue]) -> String {
    let mut out = String::from("WEBVTT\n\n");
    for (i, cue) in cues.iter().enumerate() {
        if i > 0 {
            out.push_str("\n\n");
        }
        out.push_str(&format!("{}\n", cue.index));
        out.push_str(&format!("{} --> {}\n", format_webvtt_timestamp(cue.start_ms), format_webvtt_timestamp(cue.end_ms)));
        out.push_str(&cue.text);
    }
    out
}

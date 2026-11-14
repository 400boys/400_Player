//! Advanced SubStation Alpha (ASS / SSA) subtitle decoder and encoder.

use crate::codec_id::CodecId;
use crate::context::{CodecContext, Decoder, Encoder};
use rmedia_util::error::{Result, RmediaError};

/// Parsed ASS Dialogue event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssDialogue {
    pub layer: u32,
    pub start_ms: u64,
    pub end_ms: u64,
    pub style: String,
    pub name: String,
    pub margin_l: u32,
    pub margin_r: u32,
    pub margin_v: u32,
    pub effect: String,
    pub text: String,
}

/// Parse ASS timestamp "H:MM:SS.cs" into milliseconds.
pub fn parse_ass_timestamp(s: &str) -> Option<u64> {
    let parts: Vec<&str> = s.trim().split(&[':', '.'][..]).collect();
    if parts.len() != 4 {
        return None;
    }
    let h: u64 = parts[0].parse().ok()?;
    let m: u64 = parts[1].parse().ok()?;
    let sec: u64 = parts[2].parse().ok()?;
    let cs: u64 = parts[3].parse().ok()?;
    Some(h * 3600_000 + m * 60_000 + sec * 1000 + cs * 10)
}

/// Format milliseconds into ASS timestamp "H:MM:SS.cs".
pub fn format_ass_timestamp(ms: u64) -> String {
    let h = ms / 3600_000;
    let rem = ms % 3600_000;
    let m = rem / 60_000;
    let rem = rem % 60_000;
    let sec = rem / 1000;
    let cs = (rem % 1000) / 10;
    format!("{}:{:02}:{:02}.{:02}", h, m, sec, cs)
}

/// Parse ASS Dialogue line.
pub fn parse_ass_dialogue(line: &str) -> Option<AssDialogue> {
    let line = line.trim();
    if !line.starts_with("Dialogue:") {
        return None;
    }
    let content = line["Dialogue:".len()..].trim();
    let parts: Vec<&str> = content.splitn(10, ',').collect();
    if parts.len() < 10 {
        return None;
    }

    let layer = parts[0].trim().parse().unwrap_or(0);
    let start_ms = parse_ass_timestamp(parts[1])?;
    let end_ms = parse_ass_timestamp(parts[2])?;
    let style = parts[3].trim().to_string();
    let name = parts[4].trim().to_string();
    let margin_l = parts[5].trim().parse().unwrap_or(0);
    let margin_r = parts[6].trim().parse().unwrap_or(0);
    let margin_v = parts[7].trim().parse().unwrap_or(0);
    let effect = parts[8].trim().to_string();
    let text = parts[9].to_string();

    Some(AssDialogue {
        layer,
        start_ms,
        end_ms,
        style,
        name,
        margin_l,
        margin_r,
        margin_v,
        effect,
        text,
    })
}

/// Format ASS Dialogue into line.
pub fn format_ass_dialogue(d: &AssDialogue) -> String {
    format!(
        "Dialogue: {},{},{},{},{},{},{},{},{},{}",
        d.layer,
        format_ass_timestamp(d.start_ms),
        format_ass_timestamp(d.end_ms),
        d.style,
        d.name,
        d.margin_l,
        d.margin_r,
        d.margin_v,
        d.effect,
        d.text
    )
}

//! Subtitle decoders and encoders (SRT, ASS, WebVTT).

pub mod ass;
pub mod srt;
pub mod webvtt;

pub use ass::{format_ass_dialogue, parse_ass_dialogue, AssDialogue};
pub use srt::{format_srt_document, parse_srt_document, SubtitleCue};
pub use webvtt::{format_webvtt_document, parse_webvtt_document};

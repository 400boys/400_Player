//! Media description types shared by the engine, the UI and the `--info` CLI.

use serde::{Deserialize, Serialize};

/// Kind of an elementary stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StreamKind {
    Video,
    Audio,
    Subtitle,
}

/// Dynamic range flavour of a video stream.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DynamicRange {
    #[default]
    Sdr,
    /// PQ (SMPTE ST 2084) transfer function.
    Hdr10,
    /// Hybrid Log-Gamma.
    Hlg,
}

/// Description of a video stream.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct VideoStreamInfo {
    /// Index of the stream inside the container.
    pub index: usize,
    pub codec: String,
    pub profile: Option<String>,
    pub width: u32,
    pub height: u32,
    pub fps: Option<f64>,
    pub pix_fmt: String,
    pub bit_depth: u8,
    pub bit_rate: Option<u64>,
    /// Display rotation in degrees (0, 90, 180, 270) coming from container metadata.
    pub rotation: i32,
    pub dynamic_range: DynamicRange,
    pub color_space: Option<String>,
    pub language: Option<String>,
    pub title: Option<String>,
    pub is_default: bool,
    /// `true` when the stream is just a cover picture attached to an audio file.
    pub is_cover_art: bool,
}

/// Description of an audio stream.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AudioStreamInfo {
    pub index: usize,
    pub codec: String,
    pub channels: u16,
    pub channel_layout: String,
    pub sample_rate: u32,
    pub sample_fmt: String,
    pub bit_rate: Option<u64>,
    pub language: Option<String>,
    pub title: Option<String>,
    pub is_default: bool,
}

/// Description of a subtitle stream embedded in the container.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SubtitleStreamInfo {
    pub index: usize,
    pub codec: String,
    /// `true` for text formats (SubRip, ASS/SSA, WebVTT); `false` for bitmap formats (PGS, DVB, VobSub).
    pub is_text: bool,
    pub language: Option<String>,
    pub title: Option<String>,
    pub is_default: bool,
    pub is_forced: bool,
}

/// A chapter marker.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Chapter {
    pub start: f64,
    pub end: f64,
    pub title: String,
}

/// Everything the UI needs to know about an opened media.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MediaInfo {
    pub source: String,
    pub title: Option<String>,
    /// Duration in seconds; `None` for live streams.
    pub duration: Option<f64>,
    pub container: String,
    pub bit_rate: Option<u64>,
    pub size_bytes: Option<u64>,
    pub is_live: bool,
    pub is_seekable: bool,
    pub video: Vec<VideoStreamInfo>,
    pub audio: Vec<AudioStreamInfo>,
    pub subtitles: Vec<SubtitleStreamInfo>,
    pub chapters: Vec<Chapter>,
    /// Container tags in file order (`artist`, `album`, `title`, …).
    pub metadata: Vec<(String, String)>,
    /// Stream index of the video stream being played, if any.
    pub selected_video: Option<usize>,
    /// Stream index of the audio stream being played, if any.
    pub selected_audio: Option<usize>,
    /// Stream index of the embedded subtitle stream being shown, if any.
    pub selected_subtitle: Option<usize>,
}

impl MediaInfo {
    /// First video stream that is not a cover picture.
    pub fn primary_video(&self) -> Option<&VideoStreamInfo> {
        self.selected_video.and_then(|i| self.video.iter().find(|v| v.index == i)).or_else(|| self.video.iter().find(|v| !v.is_cover_art))
    }

    /// `true` when there is something to *see* (i.e. a real video stream).
    pub fn has_video(&self) -> bool {
        self.video.iter().any(|v| !v.is_cover_art)
    }

    /// Looks up a metadata tag case-insensitively.
    pub fn tag(&self, key: &str) -> Option<&str> {
        self.metadata.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.as_str()).filter(|v| !v.trim().is_empty())
    }

    /// Best human title: tag title, else `None` (callers fall back to the file name).
    pub fn display_title(&self) -> Option<String> {
        self.title.clone().or_else(|| self.tag("title").map(str::to_owned))
    }

    /// Artist tag, if present.
    pub fn artist(&self) -> Option<&str> {
        self.tag("artist").or_else(|| self.tag("album_artist")).or_else(|| self.tag("performer"))
    }

    /// Index of the chapter that contains `t`.
    pub fn chapter_at(&self, t: f64) -> Option<usize> {
        self.chapters.iter().rposition(|c| t >= c.start - 1e-6)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chapter_lookup() {
        let info = MediaInfo {
            chapters: vec![
                Chapter { start: 0.0, end: 10.0, title: "a".into() },
                Chapter { start: 10.0, end: 20.0, title: "b".into() },
                Chapter { start: 20.0, end: 30.0, title: "c".into() },
            ],
            ..Default::default()
        };
        assert_eq!(info.chapter_at(0.0), Some(0));
        assert_eq!(info.chapter_at(9.99), Some(0));
        assert_eq!(info.chapter_at(10.0), Some(1));
        assert_eq!(info.chapter_at(99.0), Some(2));
    }

    #[test]
    fn tags_are_case_insensitive() {
        let info = MediaInfo { metadata: vec![("ARTIST".into(), "Daft Punk".into()), ("title".into(), " ".into())], ..Default::default() };
        assert_eq!(info.artist(), Some("Daft Punk"));
        assert_eq!(info.tag("title"), None);
    }
}

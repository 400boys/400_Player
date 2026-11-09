//! Opening media and describing them ([`MediaInfo`]), stream selection, cover art.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use lumen400_core::lang;
use lumen400_core::media_kind;
use lumen400_core::types::{AudioStreamInfo, DynamicRange, MediaInfo, SubtitleStreamInfo, VideoStreamInfo};

use crate::container::{ContainerInput, MediaKind, StreamDesc, open_container};
use crate::error::{EngineError, Result};
use crate::frame::{RgbaImage, TransferCharacteristic};

/// Language preferences used to choose tracks automatically (ISO codes, most wanted first).
#[derive(Clone, Debug, Default)]
pub struct LangPrefs {
    pub audio: Vec<String>,
    pub subtitles: Vec<String>,
    /// Show subtitles automatically when the rules below pick one.
    pub auto_subtitles: bool,
}

impl LangPrefs {
    /// Parses the comma separated lists stored in the settings (`"fr, en"`).
    pub fn from_settings(audio: &str, subtitles: &str, auto_subtitles: bool) -> Self {
        let split = |s: &str| s.split([',', ';', ' ']).map(str::trim).filter(|x| !x.is_empty()).map(str::to_owned).collect();
        Self { audio: split(audio), subtitles: split(subtitles), auto_subtitles }
    }
}

/// Explicit track choices made by the user; `None` means "decide automatically".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StreamSelection {
    pub video: Option<usize>,
    /// `Some(None)` = no audio at all.
    pub audio: Option<Option<usize>>,
    /// `Some(None)` = subtitles off.
    pub subtitle: Option<Option<usize>>,
}

/// Turns what the user typed / dropped into a normalized path or URL.
pub(crate) fn normalize_source(src: &str) -> String {
    let s = src.trim();
    if media_kind::is_file_url(s)
        && let Some(p) = media_kind::file_url_to_path(s)
    {
        return p.to_string_lossy().into_owned();
    }
    s.to_owned()
}

/// Opens `src` using the standalone pure-Rust container parser with an abort hook (`stop`).
pub(crate) fn open_input(src: &str, stop: Arc<AtomicBool>) -> Result<ContainerInput> {
    let src = normalize_source(src);
    let is_net = media_kind::is_url(&src);
    if !is_net && !Path::new(&src).exists() {
        return Err(EngineError::Open { input: src, reason: "file not found".into() });
    }
    open_container(&src, stop)
}

/// Opens, describes and closes `source` (used by `--info` and the tests).
pub fn probe(source: &str, prefs: &LangPrefs) -> Result<MediaInfo> {
    let input = open_input(source, Arc::new(AtomicBool::new(false)))?;
    let mut info = describe(&input, source);
    select_streams(&mut info, &StreamSelection::default(), prefs);
    Ok(info)
}

// -------------------------------------------------------------------------------------------------
// Description
// -------------------------------------------------------------------------------------------------

fn clean_lang(lang: Option<&str>) -> Option<String> {
    lang.map(str::trim)
        .filter(|l| !l.is_empty() && !l.eq_ignore_ascii_case("und") && !l.eq_ignore_ascii_case("unk"))
        .map(str::to_owned)
}

fn video_info(stream: &StreamDesc) -> VideoStreamInfo {
    let dynamic_range = match stream.color_trc {
        TransferCharacteristic::SMPTE2084 => DynamicRange::Hdr10,
        TransferCharacteristic::ARIB_STD_B67 => DynamicRange::Hlg,
        _ => DynamicRange::Sdr,
    };
    VideoStreamInfo {
        index: stream.index,
        codec: stream.codec.clone(),
        profile: stream.profile.clone(),
        width: stream.width,
        height: stream.height,
        fps: stream.fps_f64(),
        pix_fmt: stream.pix_fmt.name().to_owned(),
        bit_depth: stream.bit_depth,
        bit_rate: stream.bit_rate,
        rotation: stream.rotation,
        dynamic_range,
        color_space: stream.color_space.name().map(str::to_owned),
        language: clean_lang(stream.language.as_deref()),
        title: stream.title.clone(),
        is_default: stream.is_default,
        is_cover_art: stream.is_cover_art,
    }
}

fn audio_info(stream: &StreamDesc) -> AudioStreamInfo {
    AudioStreamInfo {
        index: stream.index,
        codec: stream.codec.clone(),
        channels: stream.channels,
        channel_layout: stream.channel_layout.clone(),
        sample_rate: stream.sample_rate,
        sample_fmt: stream.sample_fmt.clone(),
        bit_rate: stream.bit_rate,
        language: clean_lang(stream.language.as_deref()),
        title: stream.title.clone(),
        is_default: stream.is_default,
    }
}

fn subtitle_info(stream: &StreamDesc) -> SubtitleStreamInfo {
    SubtitleStreamInfo {
        index: stream.index,
        codec: stream.codec.clone(),
        is_text: stream.is_text_sub,
        language: clean_lang(stream.language.as_deref()),
        title: stream.title.clone(),
        is_default: stream.is_default,
        is_forced: stream.is_forced,
    }
}

/// Describes every stream, chapter and tag of an opened container.
pub(crate) fn describe(input: &ContainerInput, source: &str) -> MediaInfo {
    let mut info = MediaInfo {
        source: source.to_owned(),
        container: input.format_name.clone(),
        duration: input.duration,
        bit_rate: input.bit_rate,
        size_bytes: input.size_bytes,
        is_live: input.duration.is_none(),
        is_seekable: input.seekable && input.duration.is_some(),
        metadata: input.metadata.clone(),
        ..Default::default()
    };
    info.title = info.tag("title").map(str::to_owned);

    for stream in &input.streams {
        match stream.kind {
            MediaKind::Video => info.video.push(video_info(stream)),
            MediaKind::Audio => info.audio.push(audio_info(stream)),
            MediaKind::Subtitle => info.subtitles.push(subtitle_info(stream)),
            MediaKind::Attachment => {}
        }
    }
    info.chapters = input.chapters.iter().filter(|c| c.end >= c.start).cloned().collect();
    info
}

// -------------------------------------------------------------------------------------------------
// Selection
// -------------------------------------------------------------------------------------------------

fn lang_rank(prefs: &[String], tag: Option<&str>) -> usize {
    tag.and_then(|t| lang::preference_rank(prefs, t)).unwrap_or(usize::MAX)
}

/// Fills `selected_video / selected_audio / selected_subtitle`, honouring explicit choices first,
/// then the language preferences, then the container's `default` flags.
pub(crate) fn select_streams(info: &mut MediaInfo, sel: &StreamSelection, prefs: &LangPrefs) {
    // --- video: user choice, else the biggest real picture (cover art only as a last resort)
    info.selected_video = sel
        .video
        .filter(|i| info.video.iter().any(|v| v.index == *i))
        .or_else(|| info.video.iter().filter(|v| !v.is_cover_art).max_by_key(|v| (v.is_default, u64::from(v.width) * u64::from(v.height))).map(|v| v.index));

    // --- audio
    info.selected_audio = match sel.audio {
        Some(None) => None,
        Some(Some(i)) if info.audio.iter().any(|a| a.index == i) => Some(i),
        _ => info
            .audio
            .iter()
            .enumerate()
            .min_by_key(|(order, a)| (lang_rank(&prefs.audio, a.language.as_deref()), !a.is_default, u32::MAX - u32::from(a.channels), *order))
            .map(|(_, a)| a.index),
    };

    // --- subtitles (both text and bitmap tracks; text is preferred when both match the same rank)
    info.selected_subtitle = match sel.subtitle {
        Some(None) => None,
        Some(Some(i)) if info.subtitles.iter().any(|s| s.index == i) => Some(i),
        _ if !prefs.auto_subtitles => None,
        _ => {
            let audio_lang = info.selected_audio.and_then(|i| info.audio.iter().find(|a| a.index == i)).and_then(|a| a.language.clone());
            let candidates: Vec<&SubtitleStreamInfo> = info.subtitles.iter().collect();
            // 1. a forced track in the audio language (foreign dialogue only), preferring text
            let forced = candidates
                .iter()
                .filter(|s| {
                    s.is_forced
                        && match (&audio_lang, &s.language) {
                            (Some(a), Some(l)) => lang::same_language(a, l),
                            (_, None) => true,
                            _ => false,
                        }
                })
                .min_by_key(|s| (!s.is_text, s.index));
            if let Some(f) = forced {
                Some(f.index)
            } else if audio_lang.as_deref().zip(prefs.subtitles.first()).is_some_and(|(a, primary)| lang::same_language(a, primary)) {
                // 2. the soundtrack is already in the user's primary language: no subtitles
                None
            } else {
                // 3. otherwise the best match of the preferences (text before bitmap on tie), else
                //    the track the container flags as default
                candidates
                    .iter()
                    .filter(|s| !s.is_forced)
                    .filter_map(|s| lang::preference_rank(&prefs.subtitles, s.language.as_deref()?).map(|r| (r, !s.is_text, s.index)))
                    .min()
                    .map(|(_, _, i)| i)
                    .or_else(|| candidates.iter().filter(|s| s.is_default && !s.is_forced).min_by_key(|s| (!s.is_text, s.index)).map(|s| s.index))
            }
        }
    };
}

// -------------------------------------------------------------------------------------------------
// Cover art
// -------------------------------------------------------------------------------------------------

/// Decodes the picture attached to an audio file (ID3 `APIC`, MP4 `covr`, FLAC picture…).
pub(crate) fn cover_art(input: &ContainerInput) -> Option<RgbaImage> {
    let bytes = input.cover_art_bytes.as_deref()?;
    let img = image::load_from_memory(bytes).ok()?;
    let rgba = img.to_rgba8();
    Some(RgbaImage { width: rgba.width(), height: rgba.height(), data: rgba.into_raw() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info_with(audio: &[(&str, bool, u16)], subs: &[(&str, bool, bool)]) -> MediaInfo {
        MediaInfo {
            video: vec![VideoStreamInfo { index: 0, width: 1920, height: 1080, ..Default::default() }],
            audio: audio
                .iter()
                .enumerate()
                .map(|(i, (l, d, c))| AudioStreamInfo { index: 1 + i, language: Some((*l).into()), is_default: *d, channels: *c, ..Default::default() })
                .collect(),
            subtitles: subs
                .iter()
                .enumerate()
                .map(|(i, (l, f, t))| SubtitleStreamInfo { index: 10 + i, language: Some((*l).into()), is_forced: *f, is_text: *t, ..Default::default() })
                .collect(),
            ..Default::default()
        }
    }

    fn prefs() -> LangPrefs {
        LangPrefs::from_settings("fr, en", "fr, en", true)
    }

    #[test]
    fn prefs_parse() {
        let p = LangPrefs::from_settings("fr, en;de", "", true);
        assert_eq!(p.audio, ["fr", "en", "de"]);
        assert!(p.subtitles.is_empty());
    }

    #[test]
    fn audio_follows_language_preferences_then_default_flag() {
        let mut i = info_with(&[("eng", true, 6), ("fre", false, 2), ("jpn", false, 2)], &[]);
        select_streams(&mut i, &StreamSelection::default(), &prefs());
        assert_eq!(i.selected_audio, Some(2), "French wins over the container default");
        let mut i = info_with(&[("jpn", false, 2), ("deu", true, 2)], &[]);
        select_streams(&mut i, &StreamSelection::default(), &prefs());
        assert_eq!(i.selected_audio, Some(2), "no preferred language → default flag");
        let mut i = info_with(&[("jpn", false, 2), ("deu", false, 6)], &[]);
        select_streams(&mut i, &StreamSelection::default(), &LangPrefs::default());
        assert_eq!(i.selected_audio, Some(2), "otherwise the richest layout");
    }

    #[test]
    fn explicit_choices_win() {
        let mut i = info_with(&[("eng", true, 6), ("fre", false, 2)], &[("fre", false, true)]);
        let sel = StreamSelection { audio: Some(Some(1)), subtitle: Some(Some(10)), video: None };
        select_streams(&mut i, &sel, &prefs());
        assert_eq!(i.selected_audio, Some(1));
        assert_eq!(i.selected_subtitle, Some(10));
        let sel = StreamSelection { audio: Some(None), subtitle: Some(None), video: None };
        select_streams(&mut i, &sel, &prefs());
        assert_eq!((i.selected_audio, i.selected_subtitle), (None, None));
    }

    #[test]
    fn subtitles_are_automatic_only_when_useful() {
        let mut i = info_with(&[("eng", true, 6)], &[("eng", false, true), ("fre", false, true), ("fre", true, true)]);
        select_streams(&mut i, &StreamSelection::default(), &prefs());
        assert_eq!(i.selected_subtitle, Some(11));
        let mut i = info_with(&[("fre", true, 2)], &[("fre", false, true), ("eng", false, true)]);
        select_streams(&mut i, &StreamSelection::default(), &prefs());
        assert_eq!(i.selected_subtitle, None);
        let mut i = info_with(&[("fre", true, 2)], &[("fre", false, true), ("fre", true, true)]);
        select_streams(&mut i, &StreamSelection::default(), &prefs());
        assert_eq!(i.selected_subtitle, Some(11));
        let mut i = info_with(&[("eng", true, 2)], &[("fre", false, false)]);
        select_streams(&mut i, &StreamSelection::default(), &prefs());
        assert_eq!(i.selected_subtitle, Some(10));
        let mut both = info_with(&[("eng", true, 2)], &[("fre", false, false), ("fre", false, true)]);
        select_streams(&mut both, &StreamSelection::default(), &prefs());
        assert_eq!(both.selected_subtitle, Some(11), "text preferred over bitmap on tie");
        let mut i = info_with(&[("jpn", true, 2)], &[("ger", false, true), ("spa", false, true)]);
        i.subtitles[1].is_default = true;
        select_streams(&mut i, &StreamSelection::default(), &prefs());
        assert_eq!(i.selected_subtitle, Some(11));
        let mut i = info_with(&[("fre", true, 2)], &[("ger", false, true)]);
        i.subtitles[0].is_default = true;
        select_streams(&mut i, &StreamSelection::default(), &prefs());
        assert_eq!(i.selected_subtitle, None);
        let mut p = prefs();
        p.auto_subtitles = false;
        let mut i = info_with(&[("eng", true, 2)], &[("fre", false, true)]);
        select_streams(&mut i, &StreamSelection::default(), &p);
        assert_eq!(i.selected_subtitle, None);
    }

    #[test]
    fn video_prefers_real_pictures_over_cover_art() {
        let mut i = MediaInfo {
            video: vec![
                VideoStreamInfo { index: 0, width: 600, height: 600, is_cover_art: true, ..Default::default() },
                VideoStreamInfo { index: 1, width: 1280, height: 720, ..Default::default() },
            ],
            ..Default::default()
        };
        select_streams(&mut i, &StreamSelection::default(), &prefs());
        assert_eq!(i.selected_video, Some(1));
        let mut i = MediaInfo { video: vec![VideoStreamInfo { index: 0, width: 600, height: 600, is_cover_art: true, ..Default::default() }], ..Default::default() };
        select_streams(&mut i, &StreamSelection::default(), &prefs());
        assert_eq!(i.selected_video, None);
    }

    #[test]
    fn sources_are_normalised() {
        assert_eq!(normalize_source("file:///home/me/a%20b.mp4"), "/home/me/a b.mp4");
        assert_eq!(normalize_source("  https://x.org/a.mp4 "), "https://x.org/a.mp4");
        assert_eq!(normalize_source("/tmp/x.mkv"), "/tmp/x.mkv");
    }

    #[test]
    fn missing_files_fail_cleanly() {
        let err = open_input("/definitely/not/here.mp4", Arc::new(AtomicBool::new(false))).err().expect("must fail");
        assert!(matches!(err, EngineError::Open { .. }), "{err}");
    }
}

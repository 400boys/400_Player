//! The media information window: what FFmpeg found in the file, stream by stream.

use egui::{Align, Context, FontId, Layout, RichText, Ui, vec2};
use media_player_400_core::time::{format_clock, format_duration_long};
use media_player_400_core::types::{DynamicRange, MediaInfo};

use crate::app::MediaPlayer400App;
use crate::i18n::{Key as T, t, t1};
use crate::theme::{Palette, fonts};
use crate::ui::{Modal, icons};
use crate::util::{channels_label, codec_display, container_display, file_name_of, format_bitrate, format_bytes};
use crate::widgets::{button, icon_text};

/// A titled group of label/value rows.
pub struct Section {
    pub title: String,
    pub icon: char,
    pub rows: Vec<(String, String)>,
}

fn yes_no(v: bool) -> String {
    t(if v { T::Yes } else { T::No }).to_owned()
}

fn trim_float(v: f64, decimals: usize) -> String {
    let s = format!("{v:.decimals$}");
    crate::util::localize_decimal(if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_owned() } else { s })
}

/// The information window's content, as plain data (so it can be shown and copied identically).
pub fn sections(m: &MediaInfo) -> Vec<Section> {
    let mut out = Vec::new();

    let mut general = vec![(t(T::InfoFile).to_owned(), file_name_of(&m.source))];
    if m.source.contains("://") || m.source.contains(['/', '\\']) {
        general.push((t(T::InfoLocation).to_owned(), m.source.clone()));
    }
    if !m.container.is_empty() {
        general.push((t(T::InfoContainer).to_owned(), container_display(&m.container)));
    }
    general.push((
        t(T::InfoDuration).to_owned(),
        match m.duration {
            Some(d) if !m.is_live => format_duration_long(d),
            _ => t(T::Live).to_owned(),
        },
    ));
    if let Some(s) = m.size_bytes {
        general.push((t(T::InfoSize).to_owned(), format_bytes(s)));
    }
    if let Some(b) = m.bit_rate {
        general.push((t(T::InfoBitrate).to_owned(), format_bitrate(b)));
    }
    general.push((t(T::InfoSeekable).to_owned(), yes_no(m.is_seekable)));
    out.push(Section { title: t(T::InfoGeneral).to_owned(), icon: icons::INFO, rows: general });

    let many_video = m.video.len() > 1;
    for (n, v) in m.video.iter().enumerate() {
        let mut rows = Vec::new();
        let codec = match &v.profile {
            Some(p) if !p.is_empty() => format!("{} ({p})", codec_display(&v.codec)),
            _ => codec_display(&v.codec),
        };
        rows.push((t(T::InfoCodec).to_owned(), codec));
        rows.push((t(T::InfoResolution).to_owned(), format!("{}×{}", v.width, v.height)));
        if let Some(fps) = v.fps.filter(|f| *f > 0.0) {
            rows.push((t(T::InfoFrameRate).to_owned(), format!("{} fps", trim_float(fps, 3))));
        }
        if !v.pix_fmt.is_empty() {
            let depth = if v.bit_depth > 0 { format!(" · {} bit", v.bit_depth) } else { String::new() };
            rows.push((t(T::InfoPixelFormat).to_owned(), format!("{}{depth}", v.pix_fmt)));
        }
        rows.push((
            t(T::InfoDynamicRange).to_owned(),
            match v.dynamic_range {
                DynamicRange::Sdr => "SDR".to_owned(),
                DynamicRange::Hdr10 => "HDR10 (PQ)".to_owned(),
                DynamicRange::Hlg => "HLG".to_owned(),
            },
        ));
        if let Some(c) = v.color_space.as_deref().filter(|c| !c.is_empty()) {
            rows.push((t(T::InfoColorSpace).to_owned(), c.to_owned()));
        }
        if let Some(b) = v.bit_rate {
            rows.push((t(T::InfoBitrate).to_owned(), format_bitrate(b)));
        }
        if v.rotation % 360 != 0 {
            rows.push((t(T::InfoRotation).to_owned(), format!("{}°", v.rotation.rem_euclid(360))));
        }
        if v.is_cover_art {
            rows.push((t(T::InfoType).to_owned(), t(T::InfoCoverArt).to_owned()));
        }
        push_language_title(&mut rows, v.language.as_deref(), v.title.as_deref());
        let title = if many_video { t1(T::InfoVideoN, n + 1) } else { t(T::InfoVideo).to_owned() };
        out.push(Section { title, icon: icons::FILM_STRIP, rows });
    }

    let many_audio = m.audio.len() > 1;
    for (n, a) in m.audio.iter().enumerate() {
        let mut rows = Vec::new();
        rows.push((t(T::InfoCodec).to_owned(), codec_display(&a.codec)));
        let layout = if a.channel_layout.is_empty() { channels_label(a.channels) } else { format!("{} ({})", channels_label(a.channels), a.channel_layout) };
        rows.push((t(T::InfoChannels).to_owned(), layout));
        if a.sample_rate > 0 {
            rows.push((t(T::InfoSampleRate).to_owned(), format!("{} Hz", a.sample_rate)));
        }
        if let Some(b) = a.bit_rate {
            rows.push((t(T::InfoBitrate).to_owned(), format_bitrate(b)));
        }
        push_language_title(&mut rows, a.language.as_deref(), a.title.as_deref());
        if a.is_default {
            rows.push((t(T::InfoDefault).to_owned(), yes_no(true)));
        }
        let title = if many_audio { t1(T::InfoAudioN, n + 1) } else { t(T::InfoAudio).to_owned() };
        out.push(Section { title, icon: icons::HEADPHONES, rows });
    }

    for (n, s) in m.subtitles.iter().enumerate() {
        let mut rows = Vec::new();
        rows.push((t(T::InfoCodec).to_owned(), codec_display(&s.codec)));
        rows.push((t(T::InfoType).to_owned(), t(if s.is_text { T::InfoText } else { T::InfoBitmap }).to_owned()));
        push_language_title(&mut rows, s.language.as_deref(), s.title.as_deref());
        if s.is_forced {
            rows.push((t(T::InfoForced).to_owned(), yes_no(true)));
        }
        out.push(Section { title: t1(T::InfoSubtitleN, n + 1), icon: icons::SUBTITLES, rows });
    }

    if !m.chapters.is_empty() {
        let rows = m
            .chapters
            .iter()
            .enumerate()
            .take(60)
            .map(|(i, c)| (format_clock(c.start), if c.title.trim().is_empty() { t1(T::ChapterN, i + 1) } else { c.title.clone() }))
            .collect();
        out.push(Section { title: format!("{} ({})", t(T::InfoChapters), m.chapters.len()), icon: icons::BOOKMARK, rows });
    }

    if !m.metadata.is_empty() {
        let rows = m.metadata.iter().filter(|(_, v)| !v.trim().is_empty()).map(|(k, v)| (capitalize(k), v.clone())).collect();
        out.push(Section { title: t(T::InfoMetadata).to_owned(), icon: icons::LIST_BULLETS, rows });
    }
    out
}

fn push_language_title(rows: &mut Vec<(String, String)>, language: Option<&str>, title: Option<&str>) {
    if let Some(l) = language.filter(|l| !l.is_empty() && !l.eq_ignore_ascii_case("und")) {
        let name = media_player_400_core::lang::display_name(l, crate::i18n::current()).map_or_else(|| l.to_owned(), str::to_owned);
        rows.push((t(T::InfoLanguage).to_owned(), name));
    }
    if let Some(ti) = title.map(str::trim).filter(|s| !s.is_empty()) {
        rows.push((t(T::InfoTitle).to_owned(), ti.to_owned()));
    }
}

fn capitalize(s: &str) -> String {
    let s = s.replace('_', " ");
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// Everything as plain text, for the clipboard.
pub fn to_text(sections: &[Section]) -> String {
    let mut out = String::new();
    for s in sections {
        out.push_str(&format!("[{}]\n", s.title));
        for (k, v) in &s.rows {
            out.push_str(&format!("{k}: {v}\n"));
        }
        out.push('\n');
    }
    out.trim_end().to_owned() + "\n"
}

impl MediaPlayer400App {
    pub fn draw_info(&mut self, ctx: &Context) {
        let p = self.pal.clone();
        let media = self.media.clone();
        let screen_h = ctx.content_rect().height();
        let mut copy: Option<String> = None;
        let close = Modal::new("info", t(T::MediaInfo), icons::INFO, 680.0).show(ctx, &p, |ui| match &media {
            None => {
                ui.add_space(24.0);
                ui.vertical_centered(|ui| {
                    ui.label(icon_text(icons::FILM_SLATE, 38.0, p.text_faint));
                    ui.add_space(8.0);
                    ui.label(RichText::new(t(T::InfoNoMedia)).font(FontId::new(15.0, fonts::medium())).color(p.text_dim));
                });
                ui.add_space(28.0);
            }
            Some(m) => {
                let secs = sections(m);
                crate::ui::scroll_body(ui, (screen_h - 230.0).clamp(200.0, 620.0), |ui| {
                    ui.spacing_mut().item_spacing.y = 10.0;
                    for (i, s) in secs.iter().enumerate() {
                        section_card(ui, &p, s, i);
                    }
                });
                ui.add_space(12.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if button(ui, &p, Some(icons::COPY), t(T::CopyAll), false).clicked() {
                        copy = Some(to_text(&secs));
                    }
                });
            }
        });
        if let Some(text) = copy {
            ctx.copy_text(text);
            self.osd(icons::COPY, t(T::CopiedToClipboard), None);
        }
        if close {
            self.windows.info = false;
        }
    }
}

fn section_card(ui: &mut Ui, p: &Palette, s: &Section, index: usize) {
    p.card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(icon_text(s.icon, 18.0, p.accent));
            ui.label(RichText::new(&s.title).font(FontId::new(14.5, fonts::semibold())).color(p.text));
        });
        ui.add_space(6.0);
        egui::Grid::new(("info-grid", index)).num_columns(2).spacing(vec2(18.0, 4.0)).min_col_width(130.0).min_row_height(20.0).show(ui, |ui| {
            for (k, v) in &s.rows {
                ui.label(RichText::new(k).font(FontId::new(13.0, fonts::medium())).color(p.text_faint));
                ui.add(egui::Label::new(RichText::new(v).font(FontId::new(13.0, fonts::medium())).color(p.text)).wrap());
                ui.end_row();
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use media_player_400_core::settings::Language;
    use media_player_400_core::types::{AudioStreamInfo, Chapter, SubtitleStreamInfo, VideoStreamInfo};

    fn sample() -> MediaInfo {
        MediaInfo {
            source: "/films/Alien (1979).mkv".into(),
            duration: Some(7020.0),
            container: "matroska,webm".into(),
            bit_rate: Some(8_000_000),
            size_bytes: Some(7_000_000_000),
            is_seekable: true,
            video: vec![VideoStreamInfo {
                index: 0,
                codec: "hevc".into(),
                profile: Some("Main 10".into()),
                width: 3840,
                height: 2160,
                fps: Some(23.976),
                pix_fmt: "yuv420p10le".into(),
                bit_depth: 10,
                dynamic_range: DynamicRange::Hdr10,
                color_space: Some("bt2020nc".into()),
                ..Default::default()
            }],
            audio: vec![AudioStreamInfo {
                index: 1,
                codec: "eac3".into(),
                channels: 6,
                channel_layout: "5.1(side)".into(),
                sample_rate: 48_000,
                language: Some("eng".into()),
                is_default: true,
                ..Default::default()
            }],
            subtitles: vec![SubtitleStreamInfo { index: 2, codec: "subrip".into(), is_text: true, language: Some("fra".into()), is_forced: true, ..Default::default() }],
            chapters: vec![Chapter { start: 0.0, end: 600.0, title: "Nostromo".into() }, Chapter { start: 600.0, end: 1200.0, title: String::new() }],
            metadata: vec![("title".into(), "Alien".into()), ("empty".into(), " ".into()), ("encoded_by".into(), "me".into())],
            ..Default::default()
        }
    }

    #[test]
    fn sections_describe_every_stream() {
        let _guard = crate::i18n::test_lock();
        crate::i18n::set_language(Language::English);
        let s = sections(&sample());
        let titles: Vec<&str> = s.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(titles, ["General", "Video", "Audio", "Subtitles 1", "Chapters (2)", "Metadata"]);
        let video = &s[1].rows;
        assert!(video.contains(&("Codec".to_owned(), "HEVC (Main 10)".to_owned())), "{video:?}");
        assert!(video.contains(&("Resolution".to_owned(), "3840×2160".to_owned())));
        assert!(video.contains(&("Frame rate".to_owned(), "23.976 fps".to_owned())));
        assert!(video.contains(&("Dynamic range".to_owned(), "HDR10 (PQ)".to_owned())));
        let audio = &s[2].rows;
        assert!(audio.contains(&("Channels".to_owned(), "5.1 (5.1(side))".to_owned())), "{audio:?}");
        assert!(audio.contains(&("Language".to_owned(), "English".to_owned())));
        assert_eq!(s[4].rows[1].1, "Chapter 2", "an untitled chapter is numbered");
        assert_eq!(s[5].rows.len(), 2, "blank tags are dropped");
        assert_eq!(s[5].rows[1].0, "Encoded by");
    }

    #[test]
    fn live_streams_say_so() {
        let _guard = crate::i18n::test_lock();
        crate::i18n::set_language(Language::English);
        let m = MediaInfo { source: "http://example.com/live".into(), is_live: true, ..Default::default() };
        let s = sections(&m);
        assert!(s[0].rows.iter().any(|(k, v)| k == "Duration" && v == "LIVE"));
    }

    #[test]
    fn text_export_lists_all_rows() {
        let _guard = crate::i18n::test_lock();
        crate::i18n::set_language(Language::English);
        let text = to_text(&sections(&sample()));
        assert!(text.starts_with("[General]\nFile: Alien (1979).mkv\n"), "{text}");
        assert!(text.contains("[Video]\nCodec: HEVC (Main 10)"));
        assert!(text.ends_with('\n') && !text.ends_with("\n\n"));
    }

    #[test]
    fn floats_are_trimmed() {
        assert_eq!(trim_float(25.0, 3), "25");
        assert_eq!(trim_float(29.97, 3), "29.97");
        assert_eq!(trim_float(23.976, 3), "23.976");
    }
}

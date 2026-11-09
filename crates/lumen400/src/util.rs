//! Small formatting helpers shared by the UI modules.

use crate::i18n::{Key as T, t, t1};

/// A readable codec name: `ac3 → AC-3`, `hevc → HEVC`, `vorbis → Vorbis`.
pub fn codec_display(codec: &str) -> String {
    let c = codec.trim().to_ascii_lowercase();
    let name = match c.as_str() {
        "h264" => "H.264",
        "hevc" | "h265" => "HEVC",
        "av1" => "AV1",
        "vp8" => "VP8",
        "vp9" => "VP9",
        "mpeg2video" => "MPEG-2",
        "mpeg4" => "MPEG-4",
        "msmpeg4v3" => "MS MPEG-4",
        "theora" => "Theora",
        "prores" => "ProRes",
        "mjpeg" => "MJPEG",
        "wmv3" | "vc1" => "VC-1",
        "aac" => "AAC",
        "ac3" => "AC-3",
        "eac3" => "E-AC-3",
        "dts" => "DTS",
        "truehd" => "TrueHD",
        "mp3" => "MP3",
        "mp2" => "MP2",
        "opus" => "Opus",
        "vorbis" => "Vorbis",
        "flac" => "FLAC",
        "alac" => "ALAC",
        "wmav2" | "wmav1" => "WMA",
        "subrip" | "srt" => "SRT",
        "ass" | "ssa" => "ASS",
        "webvtt" => "WebVTT",
        "mov_text" => "MOV text",
        "hdmv_pgs_subtitle" => "PGS",
        "dvd_subtitle" => "VobSub",
        "dvb_subtitle" => "DVB",
        other if other.starts_with("pcm_") => return "PCM".to_owned(),
        _ => return codec.to_ascii_uppercase(),
    };
    name.to_owned()
}

/// A readable container name from FFmpeg's demuxer name(s): `"mov,mp4,m4a,3gp,3g2,mj2" → "MP4 / MOV"`.
pub fn container_display(demuxer: &str) -> String {
    let d = demuxer.trim().to_ascii_lowercase();
    let first = d.split(',').next().unwrap_or("");
    let name = match first {
        "mov" => "MP4 / MOV",
        "matroska" => "Matroska / WebM",
        "avi" => "AVI",
        "mpegts" => "MPEG-TS",
        "mpeg" => "MPEG-PS",
        "flv" => "Flash Video",
        "asf" => "ASF / WMV",
        "ogg" => "Ogg",
        "wav" => "WAV",
        "flac" => "FLAC",
        "mp3" => "MP3",
        "aac" => "AAC (ADTS)",
        "ac3" => "AC-3",
        "hls" | "applehttp" => "HLS",
        "dash" => "DASH",
        "rm" => "RealMedia",
        "gif" => "GIF",
        "m4v" | "h264" => "H.264 (raw)",
        "hevc" => "HEVC (raw)",
        "aiff" => "AIFF",
        "ape" => "Monkey's Audio",
        "caf" => "Core Audio",
        "rtsp" => "RTSP",
        "sdp" => "RTP / SDP",
        _ => return demuxer.trim().to_owned(),
    };
    name.to_owned()
}

/// `2 → "stéréo"`, `6 → "5.1"`, `8 → "7.1"`, `1 → "mono"`.
pub fn channels_label(channels: u16) -> String {
    match channels {
        0 => String::new(),
        1 => t(T::Mono).to_owned(),
        2 => t(T::Stereo).to_owned(),
        3 => "2.1".to_owned(),
        4 => "4.0".to_owned(),
        5 => "5.0".to_owned(),
        6 => "5.1".to_owned(),
        7 => "6.1".to_owned(),
        8 => "7.1".to_owned(),
        n => t1(T::Channels, n),
    }
}

/// A time offset the way a delay is spoken: `+50 ms`, `-250 ms`, `+1.5 s`, `0 ms`.
pub fn format_delay(secs: f64) -> String {
    if !secs.is_finite() || secs.abs() < 0.0005 {
        return "0 ms".to_owned();
    }
    if secs.abs() < 1.0 { format!("{:+} ms", (secs * 1000.0).round() as i64) } else { format!("{secs:+.1} s") }
}

/// `1.0 → "1"`, `1.25 → "1.25"` (`"1,25"` in French), `0.5 → "0.5"`.
pub fn format_speed(speed: f64) -> String {
    let s = format!("{speed:.2}");
    localize_decimal(s.trim_end_matches('0').trim_end_matches('.').to_owned())
}

/// Uses the decimal separator of the interface language (`1.5 → "1,5"` in French).
pub fn localize_decimal(s: String) -> String {
    if crate::i18n::current() == lumen400_core::settings::Language::French { s.replace('.', ",") } else { s }
}

/// `1536 → "1.5 KiB"` (`"1,5 Kio"` in French), `5_662_240 → "5.4 MiB"`.
pub fn format_bytes(bytes: u64) -> String {
    let french = crate::i18n::current() == lumen400_core::settings::Language::French;
    let units: [&str; 5] = if french { ["o", "Kio", "Mio", "Gio", "Tio"] } else { ["B", "KiB", "MiB", "GiB", "TiB"] };
    let mut v = bytes as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit < units.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{bytes} {}", units[0]) } else { localize_decimal(format!("{v:.1}")) + " " + units[unit] }
}

/// `4_500_000 → "4.5 Mb/s"` (`"4,5 Mbit/s"` in French), `128_000 → "128 kb/s"`.
pub fn format_bitrate(bits_per_second: u64) -> String {
    let french = crate::i18n::current() == lumen400_core::settings::Language::French;
    let b = bits_per_second as f64;
    if b >= 1e6 {
        localize_decimal(format!("{:.1}", b / 1e6)) + if french { " Mbit/s" } else { " Mb/s" }
    } else {
        format!("{:.0}", b / 1e3) + if french { " kbit/s" } else { " kb/s" }
    }
}

/// The last path component of a source (file name or URL tail), for titles.
pub fn file_name_of(source: &str) -> String {
    let no_query = source.split(['?', '#']).next().unwrap_or(source);
    no_query.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().filter(|s| !s.is_empty()).unwrap_or(source).to_owned()
}

/// Shortens `s` to at most `max` characters with an ellipsis in the middle (file names keep their
/// extension visible).
pub fn ellipsize_middle(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max || max < 5 {
        return s.to_owned();
    }
    let keep = max - 1;
    let tail = keep / 3;
    let head = keep - tail;
    let mut out: String = chars[..head].iter().collect();
    out.push('…');
    out.extend(chars[chars.len() - tail..].iter());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delays() {
        assert_eq!(format_delay(0.05), "+50 ms");
        assert_eq!(format_delay(-0.25), "-250 ms");
        assert_eq!(format_delay(0.0), "0 ms");
        assert_eq!(format_delay(1.5), "+1.5 s");
        assert_eq!(format_delay(-12.0), "-12.0 s");
        assert_eq!(format_delay(f64::NAN), "0 ms");
    }

    #[test]
    fn speeds() {
        let _guard = crate::i18n::test_lock();
        crate::i18n::set_language(lumen400_core::settings::Language::English);
        assert_eq!(format_speed(1.0), "1");
        assert_eq!(format_speed(1.25), "1.25");
        assert_eq!(format_speed(0.5), "0.5");
        assert_eq!(format_speed(2.0), "2");
        assert_eq!(format_speed(1.1), "1.1");
    }

    #[test]
    fn sizes() {
        let _guard = crate::i18n::test_lock();
        crate::i18n::set_language(lumen400_core::settings::Language::English);
        assert_eq!(format_bytes(900), "900 B");
        assert_eq!(format_bytes(1536), "1.5 KiB");
        assert_eq!(format_bytes(5_662_240), "5.4 MiB");
        assert_eq!(format_bitrate(128_000), "128 kb/s");
        assert_eq!(format_bitrate(4_500_000), "4.5 Mb/s");
    }

    #[test]
    fn sizes_in_french() {
        let _guard = crate::i18n::test_lock();
        crate::i18n::set_language(lumen400_core::settings::Language::French);
        assert_eq!(format_bytes(900), "900 o");
        assert_eq!(format_bytes(1536), "1,5 Kio");
        assert_eq!(format_bytes(5_662_240), "5,4 Mio");
        assert_eq!(format_bitrate(128_000), "128 kbit/s");
        assert_eq!(format_bitrate(4_500_000), "4,5 Mbit/s");
        assert_eq!(localize_decimal("29.97".to_owned()), "29,97");
        crate::i18n::set_language(lumen400_core::settings::Language::English);
        assert_eq!(localize_decimal("29.97".to_owned()), "29.97");
    }

    #[test]
    fn names() {
        assert_eq!(file_name_of("/home/me/Films/Alien.mkv"), "Alien.mkv");
        assert_eq!(file_name_of("C:\\Videos\\clip.mp4"), "clip.mp4");
        assert_eq!(file_name_of("https://example.com/v/a.mp4?token=1#t"), "a.mp4");
        assert_eq!(file_name_of("https://example.com/"), "example.com");
        assert_eq!(file_name_of("plain"), "plain");
    }

    #[test]
    fn ellipsis() {
        assert_eq!(ellipsize_middle("short.mkv", 20), "short.mkv");
        let e = ellipsize_middle("A very long file name for a movie.2024.1080p.mkv", 24);
        assert_eq!(e.chars().count(), 24);
        assert!(e.contains('…') && e.ends_with("mkv"), "{e}");
        assert_eq!(ellipsize_middle("abcdef", 3), "abcdef", "too small to shorten");
    }
}

#[cfg(test)]
mod container_tests {
    use super::container_display;

    #[test]
    fn containers_are_named_for_humans() {
        assert_eq!(container_display("mov,mp4,m4a,3gp,3g2,mj2"), "MP4 / MOV");
        assert_eq!(container_display("matroska,webm"), "Matroska / WebM");
        assert_eq!(container_display("MPEGTS"), "MPEG-TS");
        assert_eq!(container_display("something_new"), "something_new");
        assert_eq!(container_display(""), "");
    }
}

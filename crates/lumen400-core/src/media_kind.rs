//! File classification, folder scanning and sibling-subtitle discovery.

use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};

pub const VIDEO_EXTS: &[&str] = &[
    "mp4", "mkv", "webm", "avi", "mov", "m4v", "wmv", "flv", "f4v", "ts", "m2ts", "mts", "mpg", "mpeg", "mpe", "3gp", "3g2", "ogv", "vob", "divx", "asf", "rm", "rmvb", "mxf",
    "y4m", "ivf", "mk3d", "wtv", "dvr-ms", "evo", "nut", "mj2", "qt", "m2v", "m1v", "h264", "264", "hevc", "265", "av1", "obu", "gif", "webp", "apng",
];
pub const AUDIO_EXTS: &[&str] = &[
    "mp3", "flac", "wav", "ogg", "oga", "opus", "m4a", "m4b", "aac", "wma", "aiff", "aif", "aifc", "ape", "wv", "mka", "ac3", "eac3", "dts", "mp2", "mpa", "amr", "caf", "tta",
    "tak", "dsf", "dff", "mpc", "spx", "au", "snd", "w64", "rf64", "bwf", "thd", "truehd", "mlp", "alaw", "ulaw",
];
pub const SUBTITLE_EXTS: &[&str] = &["srt", "vtt", "ass", "ssa", "sub", "sup"];
pub const PLAYLIST_EXTS: &[&str] = &["m3u", "m3u8", "pls", "xspf"];

/// Coarse classification of a path based on its extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathKind {
    Video,
    Audio,
    Subtitle,
    Playlist,
    Other,
}

fn ext_of(path: &Path) -> Option<String> {
    path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase())
}

/// Classifies `path` by extension.
pub fn classify(path: &Path) -> PathKind {
    let Some(ext) = ext_of(path) else { return PathKind::Other };
    let ext = ext.as_str();
    if VIDEO_EXTS.contains(&ext) {
        PathKind::Video
    } else if AUDIO_EXTS.contains(&ext) {
        PathKind::Audio
    } else if SUBTITLE_EXTS.contains(&ext) {
        PathKind::Subtitle
    } else if PLAYLIST_EXTS.contains(&ext) {
        PathKind::Playlist
    } else {
        PathKind::Other
    }
}

/// `true` for files the player can open directly (audio or video).
pub fn is_playable(path: &Path) -> bool {
    matches!(classify(path), PathKind::Video | PathKind::Audio)
}

/// `true` when `s` looks like `scheme://…` (excluding Windows drive letters such as `C:\`).
pub fn is_url(s: &str) -> bool {
    let s = s.trim();
    match s.find("://") {
        Some(i) if i >= 2 => s[..i].chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')),
        _ => false,
    }
}

/// `true` for plain `file://` URLs.
pub fn is_file_url(s: &str) -> bool {
    s.trim().len() > 7 && s.trim()[..7].eq_ignore_ascii_case("file://")
}

/// Converts a `file://` URL into a path (percent-decoding included).
pub fn file_url_to_path(url: &str) -> Option<PathBuf> {
    let rest = url.trim().get(7..)?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let decoded = percent_decode(rest);
    // `file:///C:/x` → `C:/x` on Windows-style paths.
    let bytes = decoded.as_bytes();
    let decoded = if bytes.len() > 2 && bytes[0] == b'/' && bytes[2] == b':' && bytes[1].is_ascii_alphabetic() { decoded[1..].to_owned() } else { decoded };
    Some(PathBuf::from(decoded))
}

/// Percent-decodes a URL component (invalid escapes are kept verbatim).
pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2]))
        {
            out.push(h << 4 | l);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Percent-encodes a filesystem path as a `file://` URL.
pub fn path_to_file_url(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    let mut out = String::from("file://");
    if !s.starts_with('/') {
        out.push('/');
    }
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b':' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Case-insensitive natural ordering (`file2` < `file10`).
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    natord::compare_ignore_case(a, b)
}

/// Lists the playable files of `dir` (optionally recursive), naturally sorted.
pub fn scan_folder(dir: &Path, recursive: bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    scan_into(dir, recursive, 0, &mut out);
    out.sort_by(|a, b| natural_cmp(&a.to_string_lossy(), &b.to_string_lossy()));
    out
}

fn scan_into(dir: &Path, recursive: bool, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for entry in rd.flatten() {
        let path = entry.path();
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_dir() {
            if recursive && depth < 8 && !entry.file_name().to_string_lossy().starts_with('.') {
                scan_into(&path, recursive, depth + 1, out);
            }
        } else if is_playable(&path) {
            out.push(path);
        }
    }
}

/// Finds subtitle files that belong to `video`: same stem (optionally followed by a language
/// suffix such as `.fr` / `.en.forced`) next to the video or inside `Subs/` / `Subtitles/`.
pub fn find_sibling_subtitles(video: &Path) -> Vec<PathBuf> {
    let Some(stem) = video.file_stem().map(|s| s.to_string_lossy().to_lowercase()) else { return Vec::new() };
    let Some(dir) = video.parent() else { return Vec::new() };
    let mut dirs = vec![dir.to_path_buf()];
    for sub in ["Subs", "subs", "Subtitles", "subtitles", "Sub", "sub"] {
        let p = dir.join(sub);
        if p.is_dir() && !dirs.iter().any(|d| d.as_os_str().eq_ignore_ascii_case(p.as_os_str())) {
            dirs.push(p);
        }
    }
    let mut out = Vec::new();
    for d in dirs {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for entry in rd.flatten() {
            let path = entry.path();
            if classify(&path) != PathKind::Subtitle {
                continue;
            }
            let Some(name) = path.file_stem().map(|s| s.to_string_lossy().to_lowercase()) else { continue };
            if name == stem || (name.starts_with(&stem) && name[stem.len()..].starts_with('.')) {
                out.push(path);
            }
        }
    }
    out.sort_by(|a, b| natural_cmp(&a.to_string_lossy(), &b.to_string_lossy()));
    out.dedup_by(|a, b| a.as_os_str().eq_ignore_ascii_case(b.as_os_str()));
    out
}

/// Extracts a language hint from a subtitle file name (`movie.fr.srt` → `fr`).
pub fn subtitle_language_hint(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_string_lossy().to_string();
    let last = stem.rsplit('.').next()?;
    if stem.contains('.') && (2..=3).contains(&last.len()) && last.chars().all(|c| c.is_ascii_alphabetic()) { Some(last.to_lowercase()) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification() {
        assert_eq!(classify(Path::new("a/b/Film.MKV")), PathKind::Video);
        assert_eq!(classify(Path::new("x.flac")), PathKind::Audio);
        assert_eq!(classify(Path::new("x.SRT")), PathKind::Subtitle);
        assert_eq!(classify(Path::new("list.m3u8")), PathKind::Playlist);
        assert_eq!(classify(Path::new("readme.txt")), PathKind::Other);
        assert_eq!(classify(Path::new("noext")), PathKind::Other);
    }

    #[test]
    fn urls() {
        assert!(is_url("https://example.com/a.mp4"));
        assert!(is_url("rtsp://cam/stream"));
        assert!(!is_url("C:\\videos\\a.mp4"));
        assert!(!is_url("/home/me/a.mp4"));
        assert!(is_file_url("file:///home/me/a.mp4"));
        assert_eq!(file_url_to_path("file:///home/me/a%20b.mp4"), Some(PathBuf::from("/home/me/a b.mp4")));
        assert_eq!(file_url_to_path("file:///C:/Users/me/a.mp4"), Some(PathBuf::from("C:/Users/me/a.mp4")));
        assert_eq!(path_to_file_url(Path::new("/home/me/a b.mp4")), "file:///home/me/a%20b.mp4");
    }

    #[test]
    fn percent() {
        assert_eq!(percent_decode("a%20b%2Fc"), "a b/c");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
        assert_eq!(percent_decode("%e2%82%ac"), "€");
    }

    #[test]
    fn natural_sorting() {
        let mut v = vec!["ep10.mkv", "ep2.mkv", "ep1.mkv"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, ["ep1.mkv", "ep2.mkv", "ep10.mkv"]);
    }

    #[test]
    fn language_hint() {
        assert_eq!(subtitle_language_hint(Path::new("Movie.fr.srt")), Some("fr".into()));
        assert_eq!(subtitle_language_hint(Path::new("Movie.srt")), None);
    }

    #[test]
    fn sibling_discovery() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-sib")
            .join(format!("lumen400-sib-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("Subs")).unwrap();
        fs::write(dir.join("Movie.mkv"), b"").unwrap();
        fs::write(dir.join("Movie.srt"), b"").unwrap();
        fs::write(dir.join("Movie.fr.srt"), b"").unwrap();
        fs::write(dir.join("Movie2.srt"), b"").unwrap();
        fs::write(dir.join("Subs/Movie.en.ass"), b"").unwrap();
        let found = find_sibling_subtitles(&dir.join("Movie.mkv"));
        let names: Vec<_> = found.iter().map(|p| p.file_name().unwrap().to_string_lossy().to_string()).collect();
        assert_eq!(names, ["Movie.fr.srt", "Movie.srt", "Movie.en.ass"]);
        let scanned = scan_folder(&dir, true);
        assert_eq!(scanned.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }
}

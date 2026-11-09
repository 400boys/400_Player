//! The play queue: ordering, shuffle, repeat and playlist file import / export.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::media_kind::{file_url_to_path, is_file_url, is_url, path_to_file_url, percent_decode};
use crate::text::{decode_bytes, xml_escape, xml_unescape};

pub type ItemId = u64;

/// One entry of the queue.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaylistItem {
    pub id: ItemId,
    /// Absolute path or URL.
    pub source: String,
    pub title: Option<String>,
    /// Duration in seconds, filled in once known.
    pub duration: Option<f64>,
}

impl PlaylistItem {
    /// `true` for network sources (anything with a scheme except `file://`).
    pub fn is_remote(&self) -> bool {
        is_url(&self.source) && !is_file_url(&self.source)
    }

    /// Local path of the item, if it lives on disk.
    pub fn local_path(&self) -> Option<PathBuf> {
        if is_file_url(&self.source) {
            file_url_to_path(&self.source)
        } else if is_url(&self.source) {
            None
        } else {
            Some(PathBuf::from(&self.source))
        }
    }

    /// Title to show in lists: explicit title, else file stem, else last URL segment.
    pub fn display_name(&self) -> String {
        if let Some(t) = self.title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
            return t.to_owned();
        }
        if let Some(p) = self.local_path()
            && let Some(stem) = p.file_stem()
        {
            return stem.to_string_lossy().into_owned();
        }
        let no_query = self.source.split(['?', '#']).next().unwrap_or(&self.source);
        let tail = no_query.trim_end_matches('/').rsplit('/').next().unwrap_or(no_query);
        let decoded = percent_decode(tail);
        if decoded.is_empty() {
            return self.source.clone();
        }
        // Hide a recognised media extension (`clip.mp4` → `clip`), keep anything else verbatim.
        match Path::new(&decoded).file_stem() {
            Some(stem) if crate::media_kind::is_playable(Path::new(&decoded)) => stem.to_string_lossy().into_owned(),
            _ => decoded,
        }
    }
}

/// What to do when the end of the queue is reached.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RepeatMode {
    #[default]
    Off,
    /// Loop the whole queue.
    All,
    /// Loop the current item.
    One,
}

impl RepeatMode {
    /// Off → All → One → Off.
    pub fn cycle(self) -> Self {
        match self {
            Self::Off => Self::All,
            Self::All => Self::One,
            Self::One => Self::Off,
        }
    }
}

/// A playlist entry as read from a file (no id yet).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImportedEntry {
    pub source: String,
    pub title: Option<String>,
    pub duration: Option<f64>,
}

/// Ordered play queue with shuffle and repeat.
#[derive(Debug, Default)]
pub struct Playlist {
    items: Vec<PlaylistItem>,
    current: Option<ItemId>,
    repeat: RepeatMode,
    shuffle: bool,
    next_id: ItemId,
    /// Items not yet played in the current shuffle cycle (the *last* element is next).
    bag: Vec<ItemId>,
    /// Items played before the current one (for "previous" while shuffling).
    history: Vec<ItemId>,
    rng: fastrand::Rng,
}

impl Playlist {
    pub fn new() -> Self {
        Self { next_id: 1, ..Default::default() }
    }

    /// Deterministic queue for tests.
    pub fn with_seed(seed: u64) -> Self {
        Self { next_id: 1, rng: fastrand::Rng::with_seed(seed), ..Default::default() }
    }

    // ---- inspection -------------------------------------------------------------------------

    pub fn items(&self) -> &[PlaylistItem] {
        &self.items
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    pub fn current_id(&self) -> Option<ItemId> {
        self.current
    }
    pub fn current(&self) -> Option<&PlaylistItem> {
        self.current.and_then(|id| self.get(id))
    }
    pub fn current_index(&self) -> Option<usize> {
        self.current.and_then(|id| self.index_of(id))
    }
    pub fn get(&self, id: ItemId) -> Option<&PlaylistItem> {
        self.items.iter().find(|i| i.id == id)
    }
    pub fn index_of(&self, id: ItemId) -> Option<usize> {
        self.items.iter().position(|i| i.id == id)
    }
    pub fn repeat(&self) -> RepeatMode {
        self.repeat
    }
    pub fn shuffle(&self) -> bool {
        self.shuffle
    }

    // ---- mutation ---------------------------------------------------------------------------

    pub fn set_repeat(&mut self, mode: RepeatMode) {
        self.repeat = mode;
    }

    pub fn set_shuffle(&mut self, on: bool) {
        if on != self.shuffle {
            self.shuffle = on;
            self.history.clear();
            self.refill_bag();
        }
    }

    /// Appends a path or URL and returns its id.
    pub fn add(&mut self, source: impl Into<String>) -> ItemId {
        self.insert(self.items.len(), ImportedEntry { source: source.into(), ..Default::default() })
    }

    /// Appends a local path.
    pub fn add_path(&mut self, path: &Path) -> ItemId {
        self.add(path.to_string_lossy().into_owned())
    }

    /// Inserts one entry at `index` (clamped).
    pub fn insert(&mut self, index: usize, entry: ImportedEntry) -> ItemId {
        let id = self.next_id;
        self.next_id += 1;
        let index = index.min(self.items.len());
        self.items.insert(index, PlaylistItem { id, source: entry.source, title: entry.title, duration: entry.duration });
        if self.shuffle && self.current != Some(id) {
            let pos = self.rng.usize(..=self.bag.len());
            self.bag.insert(pos, id);
        }
        id
    }

    /// Appends many entries, returning their ids.
    pub fn extend(&mut self, entries: impl IntoIterator<Item = ImportedEntry>) -> Vec<ItemId> {
        entries.into_iter().map(|e| self.insert(self.items.len(), e)).collect()
    }

    pub fn remove(&mut self, id: ItemId) -> bool {
        let Some(idx) = self.index_of(id) else { return false };
        self.items.remove(idx);
        self.bag.retain(|&b| b != id);
        self.history.retain(|&h| h != id);
        if self.current == Some(id) {
            self.current = None;
        }
        true
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.bag.clear();
        self.history.clear();
        self.current = None;
    }

    /// Moves the item at `from` so that it ends up at index `to`.
    pub fn move_item(&mut self, from: usize, to: usize) {
        if from >= self.items.len() || from == to {
            return;
        }
        let item = self.items.remove(from);
        self.items.insert(to.min(self.items.len()), item);
    }

    /// Natural, case-insensitive sort by display name.
    pub fn sort_by_name(&mut self) {
        self.items.sort_by(|a, b| crate::media_kind::natural_cmp(&a.display_name(), &b.display_name()));
    }

    pub fn set_duration(&mut self, id: ItemId, secs: f64) {
        if let Some(i) = self.items.iter_mut().find(|i| i.id == id) {
            i.duration = Some(secs).filter(|d| d.is_finite() && *d > 0.0);
        }
    }

    pub fn set_title(&mut self, id: ItemId, title: Option<String>) {
        if let Some(i) = self.items.iter_mut().find(|i| i.id == id) {
            i.title = title.filter(|t| !t.trim().is_empty());
        }
    }

    // ---- navigation -------------------------------------------------------------------------

    /// Makes `id` the current item (e.g. user double-click). Returns `false` if unknown.
    pub fn set_current(&mut self, id: ItemId) -> bool {
        if self.get(id).is_none() {
            return false;
        }
        if let Some(prev) = self.current.filter(|&p| p != id) {
            self.history.push(prev);
            if self.history.len() > 500 {
                self.history.remove(0);
            }
        }
        self.bag.retain(|&b| b != id);
        self.current = Some(id);
        true
    }

    /// Next item to play.
    ///
    /// `auto` is `true` when the previous item ended by itself (as opposed to the user pressing
    /// "next"): only then does [`RepeatMode::One`] replay the current item.
    pub fn advance(&mut self, auto: bool) -> Option<ItemId> {
        if self.items.is_empty() {
            return None;
        }
        if auto && self.repeat == RepeatMode::One && self.current.is_some() {
            return self.current;
        }
        let next = if self.shuffle { self.next_shuffled() } else { self.next_linear() };
        if let Some(id) = next {
            self.set_current(id);
        }
        next
    }

    /// Convenience for the "next" button.
    pub fn go_next(&mut self) -> Option<ItemId> {
        self.advance(false)
    }

    /// Previous item (shuffle-aware). Returns `None` at the very beginning.
    pub fn go_previous(&mut self) -> Option<ItemId> {
        if self.items.is_empty() {
            return None;
        }
        if self.shuffle {
            while let Some(id) = self.history.pop() {
                if self.get(id).is_some() && Some(id) != self.current {
                    if let Some(cur) = self.current {
                        self.bag.insert(0, cur);
                    }
                    self.current = Some(id);
                    return Some(id);
                }
            }
            return None;
        }
        let idx = self.current_index()?;
        let prev = if idx > 0 {
            idx - 1
        } else if self.repeat == RepeatMode::All {
            self.items.len() - 1
        } else {
            return None;
        };
        let id = self.items[prev].id;
        self.current = Some(id);
        Some(id)
    }

    /// Peeks at what a linear "next" would be, used to pre-load durations.
    pub fn peek_next_linear(&self) -> Option<ItemId> {
        let idx = self.current_index().map(|i| i + 1).unwrap_or(0);
        self.items.get(idx).map(|i| i.id)
    }

    fn next_linear(&mut self) -> Option<ItemId> {
        match self.current_index() {
            None => self.items.first().map(|i| i.id),
            Some(i) if i + 1 < self.items.len() => Some(self.items[i + 1].id),
            Some(_) if self.repeat == RepeatMode::All => self.items.first().map(|i| i.id),
            Some(_) => None,
        }
    }

    fn next_shuffled(&mut self) -> Option<ItemId> {
        if self.bag.is_empty() {
            if self.repeat == RepeatMode::All || self.current.is_none() {
                self.refill_bag();
                if self.bag.is_empty() && self.items.len() == 1 && self.repeat == RepeatMode::All {
                    return self.current.or(self.items.first().map(|i| i.id));
                }
            } else {
                return None;
            }
        }
        self.bag.pop()
    }

    fn refill_bag(&mut self) {
        self.bag = self.items.iter().map(|i| i.id).filter(|&id| Some(id) != self.current).collect();
        self.rng.shuffle(&mut self.bag);
    }

    // ---- import / export --------------------------------------------------------------------

    /// Serialises the queue as an extended M3U8 playlist.
    pub fn to_m3u8(&self) -> String {
        let mut out = String::from("#EXTM3U\n");
        for it in &self.items {
            let dur = it.duration.map(|d| d.round() as i64).unwrap_or(-1);
            out.push_str(&format!("#EXTINF:{dur},{}\n{}\n", it.display_name().replace('\n', " "), it.source));
        }
        out
    }

    /// Serialises the queue as XSPF (VLC's native playlist format).
    pub fn to_xspf(&self, title: &str) -> String {
        let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<playlist version=\"1\" xmlns=\"http://xspf.org/ns/0/\">\n");
        out.push_str(&format!("  <title>{}</title>\n  <trackList>\n", xml_escape(title)));
        for it in &self.items {
            let location = match it.local_path() {
                Some(p) if !is_url(&it.source) || is_file_url(&it.source) => path_to_file_url(&p),
                _ => it.source.clone(),
            };
            out.push_str("    <track>\n");
            out.push_str(&format!("      <location>{}</location>\n", xml_escape(&location)));
            out.push_str(&format!("      <title>{}</title>\n", xml_escape(&it.display_name())));
            if let Some(d) = it.duration {
                out.push_str(&format!("      <duration>{}</duration>\n", (d * 1000.0).round() as i64));
            }
            out.push_str("    </track>\n");
        }
        out.push_str("  </trackList>\n</playlist>\n");
        out
    }
}

// ---- parsers ------------------------------------------------------------------------------

fn normalise_source(raw: &str, base: Option<&Path>) -> String {
    let raw = raw.trim();
    if is_file_url(raw) {
        return file_url_to_path(raw).map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| raw.to_owned());
    }
    if is_url(raw) {
        return raw.to_owned();
    }
    let mut p = PathBuf::from(raw);
    if !cfg!(windows) && raw.contains('\\') && !p.exists() {
        p = PathBuf::from(raw.replace('\\', "/"));
    }
    let abs = p.is_absolute() || raw.starts_with('/') || raw.chars().nth(1) == Some(':');
    if !abs && let Some(base) = base {
        p = base.join(p);
    }
    p.to_string_lossy().into_owned()
}

/// Parses M3U / M3U8 text. Relative paths are resolved against `base_dir`.
pub fn parse_m3u(text: &str, base_dir: Option<&Path>) -> Vec<ImportedEntry> {
    let mut out = Vec::new();
    let mut pending: Option<(Option<f64>, Option<String>)> = None;
    for line in text.trim_start_matches('\u{feff}').lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("#EXTINF:") {
            let (dur, title) = rest.split_once(',').unwrap_or((rest, ""));
            let dur = dur.split_whitespace().next().and_then(|d| d.parse::<f64>().ok()).filter(|d| *d > 0.0);
            let title = Some(title.trim().to_owned()).filter(|t| !t.is_empty());
            pending = Some((dur, title));
        } else if line.starts_with('#') {
            continue;
        } else {
            let (duration, title) = pending.take().unwrap_or((None, None));
            out.push(ImportedEntry { source: normalise_source(line, base_dir), title, duration });
        }
    }
    out
}

/// Parses a PLS (`[playlist]` INI) file.
pub fn parse_pls(text: &str, base_dir: Option<&Path>) -> Vec<ImportedEntry> {
    let mut map: BTreeMap<u32, ImportedEntry> = BTreeMap::new();
    for line in text.trim_start_matches('\u{feff}').lines() {
        let Some((k, v)) = line.trim().split_once('=') else { continue };
        let k = k.trim().to_ascii_lowercase();
        let v = v.trim();
        let (field, n) = ["file", "title", "length"].iter().find_map(|f| k.strip_prefix(f).and_then(|n| n.parse::<u32>().ok()).map(|n| (*f, n))).unzip();
        let (Some(field), Some(n)) = (field, n) else { continue };
        let e = map.entry(n).or_default();
        match field {
            "file" => e.source = normalise_source(v, base_dir),
            "title" => e.title = Some(v.to_owned()).filter(|t| !t.is_empty()),
            "length" => e.duration = v.parse::<f64>().ok().filter(|d| *d > 0.0),
            _ => {}
        }
    }
    map.into_values().filter(|e| !e.source.is_empty()).collect()
}

/// Parses an XSPF document (no full XML parser needed: only `<track>` blocks are read).
pub fn parse_xspf(text: &str, base_dir: Option<&Path>) -> Vec<ImportedEntry> {
    fn tag<'a>(block: &'a str, name: &str) -> Option<&'a str> {
        let open = format!("<{name}>");
        let close = format!("</{name}>");
        let start = block.find(&open)? + open.len();
        let end = block[start..].find(&close)? + start;
        Some(block[start..end].trim())
    }
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(s) = rest.find("<track>") {
        let after = &rest[s + 7..];
        let Some(e) = after.find("</track>") else { break };
        let block = &after[..e];
        if let Some(loc) = tag(block, "location") {
            let loc = xml_unescape(loc);
            out.push(ImportedEntry {
                source: normalise_source(&loc, base_dir),
                title: tag(block, "title").map(xml_unescape).filter(|t| !t.is_empty()),
                duration: tag(block, "duration").and_then(|d| d.parse::<f64>().ok()).map(|ms| ms / 1000.0).filter(|d| *d > 0.0),
            });
        }
        rest = &after[e + 8..];
    }
    out
}

/// Reads a playlist file, picking the parser from the extension (M3U as the default).
pub fn read_playlist_file(path: &Path) -> std::io::Result<Vec<ImportedEntry>> {
    let bytes = std::fs::read(path)?;
    let text = decode_bytes(&bytes, None);
    let base = path.parent();
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    Ok(match ext.as_str() {
        "pls" => parse_pls(&text, base),
        "xspf" => parse_xspf(&text, base),
        _ => parse_m3u(&text, base),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(n: usize) -> Playlist {
        let mut p = Playlist::with_seed(42);
        for i in 0..n {
            p.add(format!("/media/{i}.mp4"));
        }
        p
    }

    #[test]
    fn linear_navigation() {
        let mut p = list(3);
        assert_eq!(p.advance(true), Some(1));
        assert_eq!(p.advance(true), Some(2));
        assert_eq!(p.advance(true), Some(3));
        assert_eq!(p.advance(true), None, "end of queue, repeat off");
        assert_eq!(p.current_id(), Some(3));
        p.set_repeat(RepeatMode::All);
        assert_eq!(p.advance(true), Some(1));
        assert_eq!(p.go_previous(), Some(3), "wraps backwards with repeat-all");
        p.set_repeat(RepeatMode::Off);
        p.set_current(1);
        assert_eq!(p.go_previous(), None);
    }

    #[test]
    fn repeat_one_only_on_auto_advance() {
        let mut p = list(3);
        p.set_current(2);
        p.set_repeat(RepeatMode::One);
        assert_eq!(p.advance(true), Some(2));
        assert_eq!(p.advance(false), Some(3), "manual next leaves the loop");
    }

    #[test]
    fn shuffle_plays_everything_once_per_cycle() {
        let mut p = list(6);
        p.set_shuffle(true);
        p.set_current(1);
        let mut seen = vec![1];
        while let Some(id) = p.advance(true) {
            seen.push(id);
        }
        seen.sort();
        assert_eq!(seen, vec![1, 2, 3, 4, 5, 6]);
        // With repeat-all a new cycle starts and never repeats the current item immediately.
        p.set_repeat(RepeatMode::All);
        let last = p.current_id();
        let next = p.advance(true);
        assert!(next.is_some());
        assert_ne!(next, last);
    }

    #[test]
    fn shuffle_previous_walks_history() {
        let mut p = list(5);
        p.set_shuffle(true);
        p.set_current(1);
        let a = p.advance(true).unwrap();
        let b = p.advance(true).unwrap();
        assert_eq!(p.go_previous(), Some(a));
        assert_eq!(p.go_previous(), Some(1));
        assert_ne!(a, b);
    }

    #[test]
    fn single_item_repeat_all_shuffled() {
        let mut p = list(1);
        p.set_shuffle(true);
        p.set_repeat(RepeatMode::All);
        p.set_current(1);
        assert_eq!(p.advance(true), Some(1));
    }

    #[test]
    fn mutation() {
        let mut p = list(4);
        p.set_current(2);
        assert!(p.remove(2));
        assert_eq!(p.current_id(), None);
        assert_eq!(p.len(), 3);
        p.move_item(0, 2);
        let order: Vec<_> = p.items().iter().map(|i| i.id).collect();
        assert_eq!(order, vec![3, 4, 1]);
        p.clear();
        assert!(p.is_empty());
    }

    #[test]
    fn display_names() {
        let it = |s: &str| PlaylistItem { id: 1, source: s.into(), title: None, duration: None };
        assert_eq!(it("/a/b/My Movie.mkv").display_name(), "My Movie");
        assert_eq!(it("https://x.org/videos/clip%20one.mp4?t=3").display_name(), "clip one");
        assert!(it("https://x.org/live/").is_remote());
        let mut t = it("/a/b.mp4");
        t.title = Some("  Custom ".into());
        assert_eq!(t.display_name(), "Custom");
    }

    #[test]
    fn m3u_roundtrip() {
        let text = "#EXTM3U\n#EXTINF:123,Artist - Song\n/music/a.mp3\n#EXTINF:-1,Stream\nhttp://radio.example/live\nrel/b.flac\n";
        let e = parse_m3u(text, Some(Path::new("/lists")));
        assert_eq!(e.len(), 3);
        assert_eq!(e[0].duration, Some(123.0));
        assert_eq!(e[0].title.as_deref(), Some("Artist - Song"));
        assert_eq!(e[1].duration, None);
        assert_eq!(e[1].source, "http://radio.example/live");
        assert_eq!(e[2].source.replace('\\', "/"), "/lists/rel/b.flac");

        let mut p = Playlist::new();
        p.extend(e);
        let out = p.to_m3u8();
        let again = parse_m3u(&out, None);
        assert_eq!(again.len(), 3);
        assert_eq!(again[0].title.as_deref(), Some("Artist - Song"));
    }

    #[test]
    fn pls_parse() {
        let text = "[playlist]\nNumberOfEntries=2\nFile2=/b.mp3\nTitle2=B\nLength2=61\nFile1=/a.mp3\nTitle1=A\nLength1=-1\nVersion=2\n";
        let e = parse_pls(text, None);
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].source, "/a.mp3");
        assert_eq!(e[0].duration, None);
        assert_eq!(e[1].title.as_deref(), Some("B"));
        assert_eq!(e[1].duration, Some(61.0));
    }

    #[test]
    fn xspf_roundtrip() {
        let mut p = Playlist::new();
        let id = p.add("/music/Rock & Roll/one two.mp3");
        p.set_title(id, Some("Rock <&> Roll".into()));
        p.set_duration(id, 215.5);
        p.add("https://example.com/a.mp4");
        let xml = p.to_xspf("Test");
        assert!(xml.contains("file:///music/Rock%20%26%20Roll/one%20two.mp3"), "{xml}");
        let back = parse_xspf(&xml, None);
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].source, "/music/Rock & Roll/one two.mp3");
        assert_eq!(back[0].title.as_deref(), Some("Rock <&> Roll"));
        assert_eq!(back[0].duration, Some(215.5));
        assert_eq!(back[1].source, "https://example.com/a.mp4");
    }
}

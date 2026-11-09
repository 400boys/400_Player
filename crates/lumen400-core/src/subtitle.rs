//! Text subtitles: SRT, WebVTT and ASS/SSA parsing, plus a time-indexed cue store.
//!
//! The parsers are deliberately forgiving (real-world subtitle files are messy) and produce a
//! small, renderer-friendly model: a cue is a list of lines, a line is a list of styled spans.
//! Positioning, karaoke, drawings and animations of ASS are intentionally dropped; alignment,
//! bold / italic / underline and explicit colours are preserved.

use std::collections::HashMap;
use std::sync::Arc;

use thiserror::Error;

/// One decoded bitmap region of an image subtitle (PGS Blu-ray, VobSub DVD, DVB).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BitmapRect {
    /// Unique identifier used as a texture cache key by the renderer.
    pub id: u64,
    /// Top-left position inside the subtitle canvas (`0 .. canvas_w`, `0 .. canvas_h`).
    pub x: i32,
    pub y: i32,
    /// Bitmap dimensions in pixels.
    pub w: u32,
    pub h: u32,
    /// Reference canvas dimensions (e.g. `1920×1080` for PGS, `720×576` for DVD/DVB).
    pub canvas_w: u32,
    pub canvas_h: u32,
    /// Straight-alpha RGBA8 pixels (`w * h * 4` bytes, row-major).
    pub rgba: Arc<Vec<u8>>,
}

impl BitmapRect {
    /// Converts an 8-bit paletted subtitle bitmap (`indices` with `stride` bytes per row and a
    /// `palette` of `0xAARRGGBB` entries) into an RGBA8 [`BitmapRect`].
    pub fn from_paletted(
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        canvas_w: u32,
        canvas_h: u32,
        indices: &[u8],
        stride: usize,
        palette: &[u32],
    ) -> Option<Self> {
        if w == 0 || h == 0 || w > 8192 || h > 8192 || stride < w as usize || palette.is_empty() {
            return None;
        }
        let needed = stride.checked_mul((h as usize).saturating_sub(1))?.checked_add(w as usize)?;
        if indices.len() < needed {
            return None;
        }
        let mut clut = [[0u8; 4]; 256];
        for (i, &c) in palette.iter().take(256).enumerate() {
            clut[i] = [(c >> 16) as u8, (c >> 8) as u8, c as u8, (c >> 24) as u8];
        }
        let mut rgba = Vec::with_capacity((w as usize) * (h as usize) * 4);
        let mut any_visible = false;
        for row in 0..h as usize {
            let src = &indices[row * stride..row * stride + w as usize];
            for &idx in src {
                let px = clut[idx as usize];
                if px[3] > 0 {
                    any_visible = true;
                }
                rgba.extend_from_slice(&px);
            }
        }
        if !any_visible {
            return None;
        }
        let cw = canvas_w.max(x.max(0) as u32 + w);
        let ch = canvas_h.max(y.max(0) as u32 + h);
        // Deterministic 64-bit FNV-1a hash so identical bitmap cues share the same GPU cache key.
        let mut h64: u64 = 0xcbf2_9ce4_8422_2325;
        for b in x.to_le_bytes().into_iter().chain(y.to_le_bytes()).chain(w.to_le_bytes()).chain(h.to_le_bytes()).chain(cw.to_le_bytes()).chain(ch.to_le_bytes()) {
            h64 ^= u64::from(b);
            h64 = h64.wrapping_mul(0x0100_0000_01b3);
        }
        for &b in &rgba {
            h64 ^= u64::from(b);
            h64 = h64.wrapping_mul(0x0100_0000_01b3);
        }
        let id = if h64 == 0 { 1 } else { h64 };
        Some(Self { id, x, y, w, h, canvas_w: cw, canvas_h: ch, rgba: Arc::new(rgba) })
    }
}

use crate::text::{decode_bytes, xml_unescape};

/// A run of text sharing the same style.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    /// Explicit colour requested by the file; `None` means "use the user's subtitle colour".
    pub color: Option<[u8; 3]>,
}

/// One visual line of a cue.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Line {
    pub spans: Vec<Span>,
}

impl Line {
    pub fn plain(&self) -> String {
        self.spans.iter().map(|s| s.text.as_str()).collect()
    }
}

/// Numpad-style alignment as used by ASS (`\an1` … `\an9`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    BottomLeft = 1,
    #[default]
    BottomCenter = 2,
    BottomRight = 3,
    MiddleLeft = 4,
    MiddleCenter = 5,
    MiddleRight = 6,
    TopLeft = 7,
    TopCenter = 8,
    TopRight = 9,
}

impl Align {
    pub fn from_numpad(n: u32) -> Option<Self> {
        Some(match n {
            1 => Self::BottomLeft,
            2 => Self::BottomCenter,
            3 => Self::BottomRight,
            4 => Self::MiddleLeft,
            5 => Self::MiddleCenter,
            6 => Self::MiddleRight,
            7 => Self::TopLeft,
            8 => Self::TopCenter,
            9 => Self::TopRight,
            _ => return None,
        })
    }

    /// Legacy SSA `\a` / `Alignment` numbering (1-3 bottom, 5-7 top, 9-11 middle).
    pub fn from_ssa_legacy(n: u32) -> Option<Self> {
        let col = match n % 4 {
            1 => 0,
            2 => 1,
            3 => 2,
            _ => return None,
        };
        let row = match n {
            1..=3 => 0,
            5..=7 => 2,
            9..=11 => 1,
            _ => return None,
        };
        Self::from_numpad(row * 3 + col + 1)
    }

    pub fn is_top(self) -> bool {
        matches!(self, Self::TopLeft | Self::TopCenter | Self::TopRight)
    }
    pub fn is_middle(self) -> bool {
        matches!(self, Self::MiddleLeft | Self::MiddleCenter | Self::MiddleRight)
    }
    /// -1 left, 0 centre, +1 right.
    pub fn horizontal(self) -> i8 {
        match self as u8 % 3 {
            1 => -1,
            2 => 0,
            _ => 1,
        }
    }
}

/// A subtitle event (either styled text lines or one or more bitmap regions).
#[derive(Clone, Debug, PartialEq)]
pub struct Cue {
    pub start: f64,
    pub end: f64,
    pub lines: Vec<Line>,
    pub align: Align,
    /// Bitmap regions for image-based subtitle tracks (PGS, VobSub, DVB); empty for text cues.
    pub bitmaps: Vec<BitmapRect>,
}

impl Cue {
    pub fn plain_text(&self) -> String {
        self.lines.iter().map(Line::plain).collect::<Vec<_>>().join("\n")
    }

    /// Cue from a plain string (used for tests and for text decoded by FFmpeg).
    pub fn from_text(start: f64, end: f64, text: &str) -> Self {
        let (lines, align) = parse_markup(text, Style::default(), false);
        Self { start, end: end.max(start), lines, align: align.unwrap_or_default(), bitmaps: Vec::new() }
    }

    /// Cue carrying one or more bitmap subtitle regions (PGS, VobSub, DVB).
    pub fn from_bitmaps(start: f64, end: f64, bitmaps: Vec<BitmapRect>) -> Self {
        Self { start, end: end.max(start), lines: Vec::new(), align: Align::default(), bitmaps }
    }
}

#[derive(Debug, Error)]
pub enum SubtitleError {
    #[error("no subtitle cues found in the file")]
    NoCues,
}

// ---------------------------------------------------------------------------------------------
// Track
// ---------------------------------------------------------------------------------------------

/// A time-ordered set of cues with fast "what is on screen at time *t*" lookup.
#[derive(Clone, Debug, Default)]
pub struct SubtitleTrack {
    cues: Vec<Cue>,
    max_len: f64,
}

impl SubtitleTrack {
    pub fn new(mut cues: Vec<Cue>) -> Self {
        cues.sort_by(|a, b| a.start.total_cmp(&b.start));
        let max_len = cues.iter().map(|c| c.end - c.start).fold(0.0, f64::max);
        Self { cues, max_len }
    }

    /// Parses subtitle bytes, sniffing the format (`ext_hint` is the lower-case file extension).
    pub fn parse(bytes: &[u8], ext_hint: Option<&str>, locale_hint: Option<&str>) -> Result<Self, SubtitleError> {
        let text = decode_bytes(bytes, locale_hint);
        let head: String = text.trim_start_matches('\u{feff}').chars().take(4096).collect();
        let cues = if head.trim_start().starts_with("WEBVTT") || ext_hint == Some("vtt") {
            parse_srt(&text)
        } else if matches!(ext_hint, Some("ass" | "ssa")) || head.contains("[Script Info]") || head.contains("[Events]") {
            parse_ass(&text)
        } else {
            parse_srt(&text)
        };
        if cues.is_empty() {
            return Err(SubtitleError::NoCues);
        }
        Ok(Self::new(cues))
    }

    pub fn cues(&self) -> &[Cue] {
        &self.cues
    }
    pub fn len(&self) -> usize {
        self.cues.len()
    }
    pub fn is_empty(&self) -> bool {
        self.cues.is_empty()
    }
    pub fn clear(&mut self) {
        self.cues.clear();
        self.max_len = 0.0;
    }

    /// Inserts a cue keeping the track ordered. Exact duplicates (same timing and content, which
    /// happen when the demuxer re-reads packets after a seek) are ignored. Returns `true` if the
    /// cue was added.
    pub fn push(&mut self, cue: Cue) -> bool {
        let pos = self.cues.partition_point(|c| c.start < cue.start - 1e-3);
        let dup = self.cues[pos..].iter().take_while(|c| (c.start - cue.start).abs() < 1e-3).any(|c| {
            if cue.bitmaps.is_empty() {
                (c.end - cue.end).abs() < 1e-3 && c.lines == cue.lines && c.bitmaps.is_empty()
            } else {
                c.bitmaps.len() == cue.bitmaps.len()
                    && c.bitmaps.iter().zip(&cue.bitmaps).all(|(a, b)| a.id == b.id && (a.x, a.y, a.w, a.h) == (b.x, b.y, b.w, b.h) && a.rgba == b.rgba)
            }
        });
        if dup {
            return false;
        }
        self.max_len = self.max_len.max(cue.end - cue.start);
        self.cues.insert(pos, cue);
        true
    }

    /// Ends any bitmap cue that is still open at `t` (PGS and DVB streams end an open display set
    /// by sending an empty subtitle packet or the next display set at `t`).
    pub fn close_bitmaps_at(&mut self, t: f64) {
        for c in &mut self.cues {
            if !c.bitmaps.is_empty() && c.start < t - 1e-3 && c.end > t {
                c.end = t;
            }
        }
    }

    /// Cues visible at media time `t` (several may overlap).
    pub fn active_at(&self, t: f64) -> Vec<&Cue> {
        let upper = self.cues.partition_point(|c| c.start <= t);
        let mut out = Vec::new();
        for c in self.cues[..upper].iter().rev() {
            if c.start < t - self.max_len {
                break;
            }
            if t < c.end {
                out.push(c);
            }
        }
        out.reverse();
        out
    }

    /// Start time of the first cue starting after `t` (used by "next subtitle" navigation).
    pub fn next_start_after(&self, t: f64) -> Option<f64> {
        self.cues.get(self.cues.partition_point(|c| c.start <= t + 1e-3)).map(|c| c.start)
    }
}

// ---------------------------------------------------------------------------------------------
// Markup (shared by SRT / VTT / ASS)
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Style {
    bold: bool,
    italic: bool,
    underline: bool,
    color: Option<[u8; 3]>,
}

struct Builder {
    lines: Vec<Line>,
    cur: Vec<Span>,
    style: Style,
}

impl Builder {
    fn new(style: Style) -> Self {
        Self { lines: Vec::new(), cur: Vec::new(), style }
    }

    fn push_str(&mut self, s: &str) {
        for c in s.chars() {
            self.push_char(c);
        }
    }

    fn push_char(&mut self, c: char) {
        let st = self.style;
        match self.cur.last_mut() {
            Some(last) if last.bold == st.bold && last.italic == st.italic && last.underline == st.underline && last.color == st.color => last.text.push(c),
            _ => self.cur.push(Span { text: c.to_string(), bold: st.bold, italic: st.italic, underline: st.underline, color: st.color }),
        }
    }

    fn newline(&mut self) {
        let spans = std::mem::take(&mut self.cur);
        self.lines.push(Line { spans });
    }

    fn finish(mut self) -> Vec<Line> {
        self.newline();
        let mut out = Vec::new();
        for mut line in self.lines {
            // Trim leading / trailing whitespace of the line.
            if let Some(first) = line.spans.first_mut() {
                first.text = first.text.trim_start().to_owned();
            }
            if let Some(last) = line.spans.last_mut() {
                last.text = last.text.trim_end().to_owned();
            }
            line.spans.retain(|s| !s.text.is_empty());
            if !line.spans.is_empty() {
                out.push(line);
            }
        }
        out
    }
}

const NAMED_COLORS: &[(&str, [u8; 3])] = &[
    ("white", [255, 255, 255]),
    ("black", [0, 0, 0]),
    ("red", [255, 0, 0]),
    ("lime", [0, 255, 0]),
    ("green", [0, 128, 0]),
    ("blue", [0, 0, 255]),
    ("yellow", [255, 255, 0]),
    ("cyan", [0, 255, 255]),
    ("aqua", [0, 255, 255]),
    ("magenta", [255, 0, 255]),
    ("fuchsia", [255, 0, 255]),
    ("orange", [255, 165, 0]),
    ("gray", [128, 128, 128]),
    ("grey", [128, 128, 128]),
    ("silver", [192, 192, 192]),
    ("pink", [255, 192, 203]),
];

fn parse_css_color(s: &str) -> Option<[u8; 3]> {
    let s = s.trim().trim_matches(['"', '\'']);
    if let Some(hex) = s.strip_prefix('#') {
        return match hex.len() {
            6 => {
                let v = u32::from_str_radix(hex, 16).ok()?;
                Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
            }
            3 => {
                let v = u32::from_str_radix(hex, 16).ok()?;
                let (r, g, b) = ((v >> 8) & 15, (v >> 4) & 15, v & 15);
                Some([(r * 17) as u8, (g * 17) as u8, (b * 17) as u8])
            }
            _ => None,
        };
    }
    let lower = s.to_ascii_lowercase();
    NAMED_COLORS.iter().find(|(n, _)| *n == lower).map(|(_, c)| *c)
}

/// ASS `&HAABBGGRR&` / `&HBBGGRR&` → RGB.
fn parse_ass_color(s: &str) -> Option<[u8; 3]> {
    let t = s.trim().trim_matches('&');
    let t = t.strip_prefix(['H', 'h']).unwrap_or(t);
    let t = t.trim_end_matches('&');
    let hex: String = t.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
    if hex.is_empty() {
        return None;
    }
    let v = u32::from_str_radix(&hex, 16).ok()?;
    Some([v as u8, (v >> 8) as u8, (v >> 16) as u8])
}

fn color_or_default(c: [u8; 3]) -> Option<[u8; 3]> {
    if c == [255, 255, 255] { None } else { Some(c) }
}

enum HtmlTag {
    Bold(bool),
    Italic(bool),
    Underline(bool),
    FontOpen(Option<[u8; 3]>),
    FontClose,
    Ignored,
}

/// Parses an HTML-ish tag at the start of `s` (which starts with `<`). Returns the tag and its
/// byte length, or `None` when the `<` is just a literal character.
fn parse_html_tag(s: &str) -> Option<(HtmlTag, usize)> {
    let end = s.char_indices().take(160).find(|&(_, c)| c == '>')?.0;
    let inner = &s[1..end];
    let first = inner.chars().next()?;
    if !(first == '/' || first.is_ascii_alphanumeric()) {
        return None;
    }
    let closing = inner.starts_with('/');
    let body = inner.trim_start_matches('/').trim();
    let name_end = body.find(|c: char| c.is_whitespace() || c == '.').unwrap_or(body.len());
    let name = body[..name_end].to_ascii_lowercase();
    let tag = match (name.as_str(), closing) {
        ("b", c) => HtmlTag::Bold(!c),
        ("i", c) | ("em", c) => HtmlTag::Italic(!c),
        ("u", c) => HtmlTag::Underline(!c),
        ("font", false) => {
            let color = body.to_ascii_lowercase().find("color=").and_then(|i| {
                let rest = &body[i + 6..];
                let val: String = if let Some(stripped) = rest.strip_prefix(['"', '\'']) {
                    stripped.chars().take_while(|c| *c != '"' && *c != '\'').collect()
                } else {
                    rest.chars().take_while(|c| !c.is_whitespace()).collect()
                };
                parse_css_color(&val)
            });
            HtmlTag::FontOpen(color)
        }
        ("font", true) => HtmlTag::FontClose,
        // WebVTT class span: <c.yellow> / <c.cyan.bg_black>
        ("c", false) => {
            let color = body[name_end..].split('.').filter(|s| !s.is_empty()).find_map(|cls| parse_css_color(cls.trim()));
            HtmlTag::FontOpen(color)
        }
        ("c", true) => HtmlTag::FontClose,
        _ => HtmlTag::Ignored,
    };
    Some((tag, end + 1))
}

/// Applies an ASS override block (the text between `{` and `}`).
fn apply_overrides(block: &str, st: &mut Style, base: Style, align: &mut Option<Align>, drawing: &mut bool) {
    let b = block.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'\\' {
            i += 1;
            continue;
        }
        i += 1;
        let start = i;
        let mut depth = 0usize;
        while i < b.len() {
            match b[i] {
                b'(' => depth += 1,
                b')' => depth = depth.saturating_sub(1),
                b'\\' if depth == 0 => break,
                _ => {}
            }
            i += 1;
        }
        let tag = block[start..i].trim();
        let num = |s: &str| s.trim().parse::<u32>().ok();
        if let Some(n) = tag.strip_prefix("an").and_then(num) {
            if let Some(a) = Align::from_numpad(n) {
                *align = Some(a);
            }
        } else if tag.starts_with("pos") || tag.starts_with("clip") || tag.starts_with("iclip") {
            // positioning / clipping: ignored
        } else if let Some(n) = tag.strip_prefix('a').and_then(num) {
            if let Some(a) = Align::from_ssa_legacy(n) {
                *align = Some(a);
            }
        } else if let Some(rest) = tag.strip_prefix('b').filter(|r| r.chars().all(|c| c.is_ascii_digit())) {
            if !rest.is_empty() {
                st.bold = rest != "0";
            }
        } else if let Some(rest) = tag.strip_prefix('i').filter(|r| r.chars().all(|c| c.is_ascii_digit())) {
            if !rest.is_empty() {
                st.italic = rest != "0";
            }
        } else if let Some(rest) = tag.strip_prefix('u').filter(|r| r.chars().all(|c| c.is_ascii_digit())) {
            if !rest.is_empty() {
                st.underline = rest != "0";
            }
        } else if let Some(rest) = tag.strip_prefix("1c").or_else(|| tag.strip_prefix('c').filter(|r| r.starts_with(['&', 'H', 'h']))) {
            st.color = parse_ass_color(rest).and_then(color_or_default);
        } else if tag == "r" || (tag.starts_with('r') && !tag.starts_with("rnd")) {
            *st = base;
        } else if let Some(rest) = tag.strip_prefix('p').filter(|r| !r.is_empty() && r.chars().all(|c| c.is_ascii_digit())) {
            *drawing = rest.parse::<u32>().unwrap_or(0) > 0;
        }
    }
}

/// Converts subtitle markup (HTML-like tags and/or ASS overrides) into lines of styled spans.
/// Returns the alignment requested by an inline override, if any.
fn parse_markup(text: &str, base: Style, ass_mode: bool) -> (Vec<Line>, Option<Align>) {
    let mut b = Builder::new(base);
    let mut align = None;
    let mut drawing = false;
    let mut color_stack: Vec<Option<[u8; 3]>> = Vec::new();
    let mut plain = String::new();
    let mut rest = text;

    macro_rules! flush {
        () => {
            if !plain.is_empty() {
                if !drawing {
                    b.push_str(&xml_unescape(&plain));
                }
                plain.clear();
            }
        };
    }

    while let Some(c) = rest.chars().next() {
        match c {
            '{' if rest[1..].starts_with('\\') => {
                if let Some(end) = rest.find('}') {
                    flush!();
                    let mut st = b.style;
                    apply_overrides(&rest[1..end], &mut st, base, &mut align, &mut drawing);
                    b.style = st;
                    rest = &rest[end + 1..];
                    continue;
                }
                plain.push(c);
                rest = &rest[1..];
            }
            '<' => match parse_html_tag(rest) {
                Some((tag, len)) => {
                    flush!();
                    match tag {
                        HtmlTag::Bold(v) => b.style.bold = v,
                        HtmlTag::Italic(v) => b.style.italic = v,
                        HtmlTag::Underline(v) => b.style.underline = v,
                        HtmlTag::FontOpen(col) => {
                            color_stack.push(b.style.color);
                            if let Some(col) = col {
                                b.style.color = color_or_default(col);
                            }
                        }
                        HtmlTag::FontClose => {
                            if let Some(prev) = color_stack.pop() {
                                b.style.color = prev;
                            }
                        }
                        HtmlTag::Ignored => {}
                    }
                    rest = &rest[len..];
                }
                None => {
                    plain.push('<');
                    rest = &rest[1..];
                }
            },
            '\\' if ass_mode => match rest[1..].chars().next() {
                Some('N') | Some('n') => {
                    flush!();
                    b.newline();
                    rest = &rest[2..];
                }
                Some('h') => {
                    plain.push('\u{a0}');
                    rest = &rest[2..];
                }
                _ => {
                    plain.push('\\');
                    rest = &rest[1..];
                }
            },
            '\n' => {
                flush!();
                b.newline();
                rest = &rest[1..];
            }
            c => {
                plain.push(c);
                rest = &rest[c.len_utf8()..];
            }
        }
    }
    flush!();
    (b.finish(), align)
}

// ---------------------------------------------------------------------------------------------
// SRT / WebVTT
// ---------------------------------------------------------------------------------------------

/// `00:01:02,345`, `01:02.345`, `1:02:03.45` → seconds.
fn parse_time(s: &str) -> Option<f64> {
    let s = s.trim();
    let (main, frac) = match s.find([',', '.']) {
        Some(i) => (&s[..i], &s[i + 1..]),
        None => (s, ""),
    };
    let parts: Vec<&str> = main.split(':').collect();
    if !(2..=3).contains(&parts.len()) {
        return None;
    }
    let mut secs = 0.0;
    for p in parts {
        secs = secs * 60.0 + p.trim().parse::<u64>().ok()? as f64;
    }
    let digits: String = frac.chars().take_while(|c| c.is_ascii_digit()).collect();
    if !digits.is_empty() && digits.len() <= 9 {
        secs += digits.parse::<u64>().ok()? as f64 / 10f64.powi(digits.len() as i32);
    }
    Some(secs)
}

/// Parses SRT and WebVTT cue blocks.
pub fn parse_srt(text: &str) -> Vec<Cue> {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = text.lines().collect();
    let mut cues = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim_start_matches('\u{feff}');
        if let Some((a, b)) = line.split_once("-->")
            && let (Some(start), Some(end)) = (parse_time(a), parse_time(b.split_whitespace().next().unwrap_or("")))
        {
            let mut j = i + 1;
            let mut body: Vec<&str> = Vec::new();
            while j < lines.len() {
                let l = lines[j];
                if l.trim().is_empty() || l.contains("-->") {
                    break;
                }
                // A bare number directly followed by a timing line is the next cue's index.
                if lines.get(j + 1).is_some_and(|n| n.contains("-->")) && l.trim().chars().all(|c| c.is_ascii_digit()) {
                    break;
                }
                body.push(l);
                j += 1;
            }
            let (lines_, align) = parse_markup(&body.join("\n"), Style::default(), false);
            if !lines_.is_empty() {
                cues.push(Cue { start, end: end.max(start), lines: lines_, align: align.unwrap_or_default(), bitmaps: Vec::new() });
            }
            i = j;
            continue;
        }
        i += 1;
    }
    cues
}

// ---------------------------------------------------------------------------------------------
// ASS / SSA
// ---------------------------------------------------------------------------------------------

/// A style from the `[V4+ Styles]` section.
#[derive(Clone, Debug, PartialEq)]
pub struct AssStyle {
    pub name: String,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub color: Option<[u8; 3]>,
    pub align: Align,
}

impl Default for AssStyle {
    fn default() -> Self {
        Self { name: "Default".into(), bold: false, italic: false, underline: false, color: None, align: Align::BottomCenter }
    }
}

/// Styles extracted from an ASS script header.
#[derive(Clone, Debug, Default)]
pub struct AssHeader {
    styles: HashMap<String, AssStyle>,
}

impl AssHeader {
    /// Parses the `[V4+ Styles]` / `[V4 Styles]` section of an ASS script (or of the extradata
    /// FFmpeg exposes for embedded ASS streams).
    pub fn parse(script: &str) -> Self {
        let mut styles = HashMap::new();
        let mut in_styles = false;
        let mut legacy = false;
        let mut cols: Vec<String> = Vec::new();
        for line in script.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                let l = line.to_ascii_lowercase();
                in_styles = l.starts_with("[v4+ styles]") || l.starts_with("[v4 styles]");
                legacy = l.starts_with("[v4 styles]");
                continue;
            }
            if !in_styles {
                continue;
            }
            if let Some(f) = line.strip_prefix("Format:") {
                cols = f.split(',').map(|c| c.trim().to_ascii_lowercase()).collect();
            } else if let Some(s) = line.strip_prefix("Style:") {
                let vals: Vec<&str> = s.splitn(cols.len().max(1), ',').map(str::trim).collect();
                let get = |name: &str| cols.iter().position(|c| c == name).and_then(|i| vals.get(i).copied());
                let Some(name) = get("name") else { continue };
                let flag = |v: Option<&str>| v.and_then(|v| v.trim().parse::<i32>().ok()).is_some_and(|v| v != 0);
                let color = get("primarycolour").and_then(|c| {
                    let t = c.trim();
                    // Old scripts store the colour as a signed decimal.
                    if let Ok(dec) = t.parse::<i64>() {
                        let v = dec as u32;
                        return Some([v as u8, (v >> 8) as u8, (v >> 16) as u8]);
                    }
                    parse_ass_color(t)
                });
                let align =
                    get("alignment").and_then(|a| a.parse::<u32>().ok()).and_then(|n| if legacy { Align::from_ssa_legacy(n) } else { Align::from_numpad(n) }).unwrap_or_default();
                styles.insert(
                    name.to_ascii_lowercase(),
                    AssStyle {
                        name: name.to_owned(),
                        bold: flag(get("bold")),
                        italic: flag(get("italic")),
                        underline: flag(get("underline")),
                        color: color.and_then(color_or_default),
                        align,
                    },
                );
            }
        }
        Self { styles }
    }

    pub fn style(&self, name: &str) -> Option<&AssStyle> {
        let n = name.trim().trim_start_matches('*').to_ascii_lowercase();
        self.styles.get(&n)
    }
}

fn ass_text_to_cue(start: f64, end: f64, style_name: &str, text: &str, header: &AssHeader) -> Option<Cue> {
    let st = header.style(style_name).cloned().unwrap_or_default();
    let base = Style { bold: st.bold, italic: st.italic, underline: st.underline, color: st.color };
    let (lines, align) = parse_markup(text, base, true);
    if lines.is_empty() {
        return None;
    }
    Some(Cue { start, end: end.max(start), lines, align: align.unwrap_or(st.align), bitmaps: Vec::new() })
}

/// Parses a complete `.ass` / `.ssa` script.
pub fn parse_ass(text: &str) -> Vec<Cue> {
    let header = AssHeader::parse(text);
    let mut in_events = false;
    let mut cols: Vec<String> = Vec::new();
    let mut cues = Vec::new();
    for line in text.trim_start_matches('\u{feff}').lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_events = line.eq_ignore_ascii_case("[events]");
            continue;
        }
        if !in_events {
            continue;
        }
        if let Some(f) = line.strip_prefix("Format:") {
            cols = f.split(',').map(|c| c.trim().to_ascii_lowercase()).collect();
        } else if let Some(d) = line.strip_prefix("Dialogue:") {
            if cols.is_empty() {
                continue;
            }
            let vals: Vec<&str> = d.trim_start().splitn(cols.len(), ',').collect();
            let get = |name: &str| cols.iter().position(|c| c == name).and_then(|i| vals.get(i).copied());
            let (Some(start), Some(end), Some(txt)) = (get("start").and_then(parse_time), get("end").and_then(parse_time), get("text")) else {
                continue;
            };
            if let Some(cue) = ass_text_to_cue(start, end, get("style").unwrap_or("Default"), txt, &header) {
                cues.push(cue);
            }
        }
    }
    cues
}

/// Converts the payload FFmpeg hands out for an embedded ASS event
/// (`ReadOrder,Layer,Style,Name,MarginL,MarginR,MarginV,Effect,Text`) into a [`Cue`].
pub fn ass_event_to_cue(payload: &str, start: f64, end: f64, header: &AssHeader) -> Option<Cue> {
    let mut it = payload.trim_end_matches(['\r', '\n']).splitn(9, ',');
    let fields: Vec<&str> = (&mut it).collect();
    if fields.len() == 9 {
        ass_text_to_cue(start, end, fields[2], fields[8], header)
    } else {
        // Not an ASS dialogue payload: treat as plain text.
        let cue = Cue::from_text(start, end, payload);
        (!cue.lines.is_empty()).then_some(cue)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRT: &str = "\u{feff}1\r\n00:00:01,000 --> 00:00:03,500\r\nHello <i>world</i>\r\nSecond &amp; line\r\n\r\n2\r\n00:00:04,000 --> 00:00:06,000\r\n<font color=\"#ff0000\">Red</font> text\r\n3\r\n00:00:07,0 --> 00:00:08,5\r\n{\\an8}Top\r\n";

    #[test]
    fn srt_basic() {
        let cues = parse_srt(SRT);
        assert_eq!(cues.len(), 3);
        assert_eq!(cues[0].start, 1.0);
        assert_eq!(cues[0].end, 3.5);
        assert_eq!(cues[0].plain_text(), "Hello world\nSecond & line");
        assert!(cues[0].lines[0].spans.iter().any(|s| s.italic && s.text == "world"));
        assert_eq!(cues[1].lines[0].spans[0].color, Some([255, 0, 0]));
        assert_eq!(cues[1].lines[0].spans[1].color, None);
        // Missing blank line between cue 2 and 3: still parsed, the "3" index is dropped.
        assert_eq!(cues[2].plain_text(), "Top");
        assert_eq!(cues[2].align, Align::TopCenter);
        assert_eq!(cues[2].start, 7.0);
    }

    #[test]
    fn webvtt() {
        let vtt = "WEBVTT\n\nNOTE comment\n\nSTYLE\n::cue { color: red }\n\nintro\n00:01.000 --> 00:02.000 align:start\n<v Bob>Hi <c.yellow>there</c></v>\n\n01:00:00.500 --> 01:00:01.000\n&lt;ok&gt;\n";
        let t = SubtitleTrack::parse(vtt.as_bytes(), Some("vtt"), None).unwrap();
        assert_eq!(t.len(), 2);
        assert_eq!(t.cues()[0].plain_text(), "Hi there");
        assert_eq!(t.cues()[0].lines[0].spans[1].color, Some([255, 255, 0]));
        assert_eq!(t.cues()[1].start, 3600.5);
        assert_eq!(t.cues()[1].plain_text(), "<ok>");
    }

    const ASS: &str = "[Script Info]\nTitle: t\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
Style: Default,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\
Style: Sign,Arial,20,&H0000FFFF,&H000000FF,&H00000000,&H00000000,-1,-1,0,0,100,100,0,0,1,2,2,8,10,10,10,1\n\
\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 0,0:00:01.00,0:00:02.50,Default,,0,0,0,,Hello, {\\i1}italic{\\i0} world\\Nsecond\n\
Dialogue: 0,0:00:03.00,0:00:04.00,Sign,,0,0,0,,Sign text\n\
Dialogue: 0,0:00:05.00,0:00:06.00,Default,,0,0,0,,{\\p1}m 0 0 l 10 10{\\p0}Visible\n\
Comment: 0,0:00:07.00,0:00:08.00,Default,,0,0,0,,ignored\n";

    #[test]
    fn ass_full_file() {
        let cues = parse_ass(ASS);
        assert_eq!(cues.len(), 3);
        assert_eq!(cues[0].plain_text(), "Hello, italic world\nsecond");
        assert_eq!(cues[0].start, 1.0);
        assert_eq!(cues[0].end, 2.5);
        assert!(cues[0].lines[0].spans.iter().any(|s| s.italic && s.text == "italic"));
        assert_eq!(cues[1].align, Align::TopCenter);
        assert!(cues[1].lines[0].spans[0].bold);
        assert_eq!(cues[1].lines[0].spans[0].color, Some([255, 255, 0]));
        assert_eq!(cues[2].plain_text(), "Visible", "vector drawings are dropped");
    }

    #[test]
    fn ass_embedded_event() {
        let header = AssHeader::parse(ASS);
        let cue = ass_event_to_cue("12,0,Sign,,0,0,0,,Hi\\Nthere, you", 10.0, 12.0, &header).unwrap();
        assert_eq!(cue.plain_text(), "Hi\nthere, you");
        assert_eq!(cue.align, Align::TopCenter);
        let plain = ass_event_to_cue("just text", 0.0, 1.0, &header).unwrap();
        assert_eq!(plain.plain_text(), "just text");
    }

    #[test]
    fn legacy_alignment() {
        assert_eq!(Align::from_ssa_legacy(2), Some(Align::BottomCenter));
        assert_eq!(Align::from_ssa_legacy(6), Some(Align::TopCenter));
        assert_eq!(Align::from_ssa_legacy(10), Some(Align::MiddleCenter));
        assert_eq!(Align::from_ssa_legacy(4), None);
        assert_eq!(Align::TopLeft.horizontal(), -1);
        assert_eq!(Align::BottomCenter.horizontal(), 0);
        assert_eq!(Align::MiddleRight.horizontal(), 1);
    }

    #[test]
    fn active_lookup_and_overlap() {
        let t = SubtitleTrack::new(vec![Cue::from_text(0.0, 10.0, "long"), Cue::from_text(2.0, 3.0, "a"), Cue::from_text(2.5, 4.0, "b"), Cue::from_text(20.0, 21.0, "later")]);
        let at = |x: f64| t.active_at(x).iter().map(|c| c.plain_text()).collect::<Vec<_>>();
        assert_eq!(at(-1.0), Vec::<String>::new());
        assert_eq!(at(1.0), ["long"]);
        assert_eq!(at(2.7), ["long", "a", "b"]);
        assert_eq!(at(3.5), ["long", "b"]);
        assert_eq!(at(15.0), Vec::<String>::new());
        assert_eq!(at(20.5), ["later"]);
        assert_eq!(t.next_start_after(2.0), Some(2.5));
        assert_eq!(t.next_start_after(30.0), None);
    }

    #[test]
    fn push_dedupes_and_orders() {
        let mut t = SubtitleTrack::default();
        assert!(t.push(Cue::from_text(5.0, 6.0, "b")));
        assert!(t.push(Cue::from_text(1.0, 2.0, "a")));
        assert!(!t.push(Cue::from_text(1.0, 2.0, "a")), "duplicate after seek");
        assert!(t.push(Cue::from_text(1.0, 2.0, "other")));
        let starts: Vec<f64> = t.cues().iter().map(|c| c.start).collect();
        assert_eq!(starts, vec![1.0, 1.0, 5.0]);
    }

    #[test]
    fn format_sniffing_and_errors() {
        assert!(SubtitleTrack::parse(ASS.as_bytes(), None, None).is_ok());
        assert!(SubtitleTrack::parse(SRT.as_bytes(), None, None).is_ok());
        assert!(matches!(SubtitleTrack::parse(b"hello", None, None), Err(SubtitleError::NoCues)));
    }

    #[test]
    fn literal_angle_brackets() {
        let c = Cue::from_text(0.0, 1.0, "if a < b and c > d");
        assert_eq!(c.plain_text(), "if a < b and c > d");
        let c = Cue::from_text(0.0, 1.0, "<b>bold</b> <unknown>x</unknown>");
        assert_eq!(c.plain_text(), "bold x");
    }

    #[test]
    fn bitmap_paletted_conversion_and_clear_packets() {
        let palette = [0x0000_0000, 0xFFFF_2040];
        let indices = [0u8, 1, 1, 0];
        let bm = BitmapRect::from_paletted(100, 800, 2, 2, 1920, 1080, &indices, 2, &palette).expect("visible bitmap");
        assert_eq!((bm.w, bm.h, bm.canvas_w, bm.canvas_h), (2, 2, 1920, 1080));
        assert_eq!(&bm.rgba[0..4], &[0, 0, 0, 0]);
        assert_eq!(&bm.rgba[4..8], &[0xFF, 0x20, 0x40, 0xFF]);
        // Fully transparent bitmaps are ignored.
        assert!(BitmapRect::from_paletted(0, 0, 2, 2, 720, 576, &[0, 0, 0, 0], 2, &palette).is_none());

        let mut track = SubtitleTrack::default();
        assert!(track.push(Cue::from_bitmaps(1.0, 13.0, vec![bm.clone()])));
        assert!(!track.push(Cue::from_bitmaps(1.0, 13.0, vec![bm])), "deduplicated on seek");
        assert_eq!(track.active_at(5.0).len(), 1);
        // A PGS clear packet at t = 3.2 s ends the open bitmap cue.
        track.close_bitmaps_at(3.2);
        assert_eq!(track.active_at(2.5).len(), 1);
        assert!(track.active_at(3.5).is_empty());
    }
}

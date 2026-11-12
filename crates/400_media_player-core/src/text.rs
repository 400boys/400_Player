//! Text decoding helpers (subtitle files and playlists come in every encoding imaginable).

use chardetng::{EncodingDetector, Iso2022JpDetection, Utf8Detection};
use encoding_rs::{Encoding, UTF_8, UTF_16BE, UTF_16LE, WINDOWS_1252};

/// Decodes `bytes` to a `String`:
/// 1. honours a BOM (UTF-8 / UTF-16 LE / UTF-16 BE),
/// 2. accepts valid UTF-8 as is,
/// 3. otherwise guesses the legacy encoding (Windows-125x, GBK, Shift-JIS, EUC-KR, …).
///
/// `locale_hint` is an optional top-level-domain-like hint (`"fr"`, `"ja"`, `"ru"`, …) that
/// improves the guess for short files.
pub fn decode_bytes(bytes: &[u8], locale_hint: Option<&str>) -> String {
    if let Some((enc, bom_len)) = Encoding::for_bom(bytes) {
        let (s, _) = enc.decode_without_bom_handling(&bytes[bom_len..]);
        return s.into_owned();
    }
    // UTF-16 without BOM: lots of NULs in the first bytes.
    if bytes.len() >= 4 {
        let head = &bytes[..bytes.len().min(64)];
        let nuls_even = head.iter().step_by(2).filter(|b| **b == 0).count();
        let nuls_odd = head.iter().skip(1).step_by(2).filter(|b| **b == 0).count();
        let half = head.len() / 2;
        if nuls_odd * 2 > half {
            return UTF_16LE.decode_without_bom_handling(bytes).0.into_owned();
        }
        if nuls_even * 2 > half {
            return UTF_16BE.decode_without_bom_handling(bytes).0.into_owned();
        }
    }
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_owned();
    }
    let mut det = EncodingDetector::new(Iso2022JpDetection::Deny);
    det.feed(bytes, true);
    let tld = locale_hint.map(|h| h.as_bytes());
    let guess = det.guess(tld, Utf8Detection::Deny);
    // Never "guess" UTF-8 for bytes that are not valid UTF-8; Windows-1252 is the safe fallback.
    let enc = if guess == UTF_8 { WINDOWS_1252 } else { guess };
    enc.decode_without_bom_handling(bytes).0.into_owned()
}

/// Escapes the five XML special characters.
pub fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

/// Decodes XML / HTML entities (`&amp;`, `&#233;`, `&#xE9;`, `&nbsp;`, …). Unknown entities are
/// kept verbatim.
pub fn xml_unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        match rest.find(';').filter(|&j| j <= 10) {
            Some(j) => {
                let name = &rest[1..j];
                let rep: Option<String> = match name {
                    "amp" => Some("&".into()),
                    "lt" => Some("<".into()),
                    "gt" => Some(">".into()),
                    "quot" => Some("\"".into()),
                    "apos" => Some("'".into()),
                    "nbsp" => Some("\u{a0}".into()),
                    "lrm" => Some("\u{200e}".into()),
                    "rlm" => Some("\u{200f}".into()),
                    n if n.starts_with("#x") || n.starts_with("#X") => u32::from_str_radix(&n[2..], 16).ok().and_then(char::from_u32).map(|c| c.to_string()),
                    n if n.starts_with('#') => n[1..].parse::<u32>().ok().and_then(char::from_u32).map(|c| c.to_string()),
                    _ => None,
                };
                match rep {
                    Some(r) => {
                        out.push_str(&r);
                        rest = &rest[j + 1..];
                    }
                    None => {
                        out.push('&');
                        rest = &rest[1..];
                    }
                }
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_and_bom() {
        assert_eq!(decode_bytes("héllo".as_bytes(), None), "héllo");
        let mut b = vec![0xEF, 0xBB, 0xBF];
        b.extend_from_slice("ça".as_bytes());
        assert_eq!(decode_bytes(&b, None), "ça");
    }

    #[test]
    fn utf16() {
        let mut le = vec![0xFF, 0xFE];
        for u in "été".encode_utf16() {
            le.extend_from_slice(&u.to_le_bytes());
        }
        assert_eq!(decode_bytes(&le, None), "été");
        let mut be = vec![0xFE, 0xFF];
        for u in "été".encode_utf16() {
            be.extend_from_slice(&u.to_be_bytes());
        }
        assert_eq!(decode_bytes(&be, None), "été");
        // No BOM.
        let mut raw = Vec::new();
        for u in "Hello world".encode_utf16() {
            raw.extend_from_slice(&u.to_le_bytes());
        }
        assert_eq!(decode_bytes(&raw, None), "Hello world");
    }

    #[test]
    fn legacy_latin1() {
        // "Café déjà vu" in Windows-1252.
        let b = b"Caf\xe9 d\xe9j\xe0 vu, c'est tr\xe8s \xe7a";
        let s = decode_bytes(b, Some("fr"));
        assert!(s.contains("Café"), "{s}");
        assert!(s.contains("très"), "{s}");
    }

    #[test]
    fn entities() {
        assert_eq!(xml_unescape("a &amp; b &lt;c&gt; &#233; &#xe9; &nbsp;|"), "a & b <c> é é \u{a0}|");
        assert_eq!(xml_unescape("AT&T rocks"), "AT&T rocks");
        assert_eq!(xml_unescape("&unknown; x"), "&unknown; x");
        assert_eq!(xml_escape("<a href=\"x\">&'"), "&lt;a href=&quot;x&quot;&gt;&amp;&apos;");
    }
}

//! Language codes: normalisation between ISO 639-1 (`fr`), 639-2/B (`fre`) and 639-2/T (`fra`),
//! plus localised display names for the track menus.

use crate::settings::Language;

/// `(639-1, 639-2/T, 639-2/B, english name, french name, native name)`
const TABLE: &[(&str, &str, &str, &str, &str, &str)] = &[
    ("ar", "ara", "ara", "Arabic", "Arabe", "العربية"),
    ("bg", "bul", "bul", "Bulgarian", "Bulgare", "Български"),
    ("ca", "cat", "cat", "Catalan", "Catalan", "Català"),
    ("cs", "ces", "cze", "Czech", "Tchèque", "Čeština"),
    ("da", "dan", "dan", "Danish", "Danois", "Dansk"),
    ("de", "deu", "ger", "German", "Allemand", "Deutsch"),
    ("el", "ell", "gre", "Greek", "Grec", "Ελληνικά"),
    ("en", "eng", "eng", "English", "Anglais", "English"),
    ("es", "spa", "spa", "Spanish", "Espagnol", "Español"),
    ("et", "est", "est", "Estonian", "Estonien", "Eesti"),
    ("fa", "fas", "per", "Persian", "Persan", "فارسی"),
    ("fi", "fin", "fin", "Finnish", "Finnois", "Suomi"),
    ("fr", "fra", "fre", "French", "Français", "Français"),
    ("he", "heb", "heb", "Hebrew", "Hébreu", "עברית"),
    ("hi", "hin", "hin", "Hindi", "Hindi", "हिन्दी"),
    ("hr", "hrv", "hrv", "Croatian", "Croate", "Hrvatski"),
    ("hu", "hun", "hun", "Hungarian", "Hongrois", "Magyar"),
    ("id", "ind", "ind", "Indonesian", "Indonésien", "Bahasa Indonesia"),
    ("is", "isl", "ice", "Icelandic", "Islandais", "Íslenska"),
    ("it", "ita", "ita", "Italian", "Italien", "Italiano"),
    ("ja", "jpn", "jpn", "Japanese", "Japonais", "日本語"),
    ("ko", "kor", "kor", "Korean", "Coréen", "한국어"),
    ("lt", "lit", "lit", "Lithuanian", "Lituanien", "Lietuvių"),
    ("lv", "lav", "lav", "Latvian", "Letton", "Latviešu"),
    ("nl", "nld", "dut", "Dutch", "Néerlandais", "Nederlands"),
    ("no", "nor", "nor", "Norwegian", "Norvégien", "Norsk"),
    ("pl", "pol", "pol", "Polish", "Polonais", "Polski"),
    ("pt", "por", "por", "Portuguese", "Portugais", "Português"),
    ("ro", "ron", "rum", "Romanian", "Roumain", "Română"),
    ("ru", "rus", "rus", "Russian", "Russe", "Русский"),
    ("sk", "slk", "slo", "Slovak", "Slovaque", "Slovenčina"),
    ("sl", "slv", "slv", "Slovenian", "Slovène", "Slovenščina"),
    ("sr", "srp", "srp", "Serbian", "Serbe", "Српски"),
    ("sv", "swe", "swe", "Swedish", "Suédois", "Svenska"),
    ("th", "tha", "tha", "Thai", "Thaï", "ไทย"),
    ("tr", "tur", "tur", "Turkish", "Turc", "Türkçe"),
    ("uk", "ukr", "ukr", "Ukrainian", "Ukrainien", "Українська"),
    ("vi", "vie", "vie", "Vietnamese", "Vietnamien", "Tiếng Việt"),
    ("zh", "zho", "chi", "Chinese", "Chinois", "中文"),
];

fn find(code: &str) -> Option<&'static (&'static str, &'static str, &'static str, &'static str, &'static str, &'static str)> {
    let c = code.trim().to_ascii_lowercase();
    // Region / script suffixes: `fr-CA`, `pt_BR`, `zh-Hans`.
    let base = c.split(['-', '_']).next().unwrap_or("");
    if base.is_empty() {
        return None;
    }
    TABLE.iter().find(|e| e.0 == base || e.1 == base || e.2 == base)
}

/// Canonical two-letter code (`"fre"` → `"fr"`), `None` when unknown.
pub fn canonical(code: &str) -> Option<&'static str> {
    find(code).map(|e| e.0)
}

/// `true` when both codes denote the same language (`"fr"` ≈ `"fra"` ≈ `"fre"` ≈ `"fr-CA"`).
pub fn same_language(a: &str, b: &str) -> bool {
    match (find(a), find(b)) {
        (Some(x), Some(y)) => x.0 == y.0,
        _ => a.trim().eq_ignore_ascii_case(b.trim()) && !a.trim().is_empty(),
    }
}

/// Position of `tag` in the ordered preference list (`0` = most wanted), `None` if absent.
pub fn preference_rank(prefs: &[String], tag: &str) -> Option<usize> {
    prefs.iter().position(|p| same_language(p, tag))
}

/// Human readable language name, `None` for unknown / undetermined codes (`und`, `zxx`, …).
pub fn display_name(code: &str, ui: Language) -> Option<&'static str> {
    let e = find(code)?;
    Some(match ui {
        Language::French => e.4,
        _ => e.3,
    })
}

/// Name of the language in the language itself.
pub fn native_name(code: &str) -> Option<&'static str> {
    find(code).map(|e| e.5)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_unified() {
        assert_eq!(canonical("fre"), Some("fr"));
        assert_eq!(canonical("fra"), Some("fr"));
        assert_eq!(canonical("FR"), Some("fr"));
        assert_eq!(canonical("pt-BR"), Some("pt"));
        assert_eq!(canonical("zh_Hans"), Some("zh"));
        assert_eq!(canonical("und"), None);
        assert!(same_language("fr", "fre"));
        assert!(same_language("ger", "deu"));
        assert!(!same_language("fr", "en"));
        assert!(!same_language("", ""));
    }

    #[test]
    fn ranks() {
        let prefs = vec!["fr".to_string(), "en".to_string()];
        assert_eq!(preference_rank(&prefs, "eng"), Some(1));
        assert_eq!(preference_rank(&prefs, "fra"), Some(0));
        assert_eq!(preference_rank(&prefs, "jpn"), None);
    }

    #[test]
    fn names() {
        assert_eq!(display_name("fra", Language::French), Some("Français"));
        assert_eq!(display_name("fra", Language::English), Some("French"));
        assert_eq!(display_name("jpn", Language::French), Some("Japonais"));
        assert_eq!(native_name("ja"), Some("日本語"));
        assert_eq!(display_name("zzz", Language::English), None);
    }
}

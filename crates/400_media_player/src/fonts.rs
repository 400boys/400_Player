//! Embedded fonts: Inter (UI), JetBrains Mono (timecodes, statistics) and Phosphor (icons).
//!
//! All three are subsetted at build-asset time (see `tools/gen_assets.py`) and licensed under the
//! SIL OFL / MIT licences — see `assets/fonts/LICENSE-*`. Scripts they do not cover (CJK, Arabic,
//! Devanagari…) fall back to the operating system's fonts, loaded on demand by eframe.

use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily};

static INTER_REGULAR: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");
static INTER_MEDIUM: &[u8] = include_bytes!("../../../assets/fonts/Inter-Medium.ttf");
static INTER_SEMIBOLD: &[u8] = include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf");
static INTER_BOLD: &[u8] = include_bytes!("../../../assets/fonts/Inter-Bold.ttf");
static JETBRAINS_MONO: &[u8] = include_bytes!("../../../assets/fonts/JetBrainsMono-Medium.ttf");
static PHOSPHOR_REGULAR: &[u8] = include_bytes!("../../../assets/fonts/Phosphor-regular.ttf");
static PHOSPHOR_BOLD: &[u8] = include_bytes!("../../../assets/fonts/Phosphor-bold.ttf");
static PHOSPHOR_FILL: &[u8] = include_bytes!("../../../assets/fonts/Phosphor-fill.ttf");

/// Builds the font set. Separate from [`install`] so it can be tested without a context.
pub fn definitions() -> FontDefinitions {
    // Keep egui's defaults as a last resort (they cover symbols and emoji the subsets do not).
    let mut fonts = FontDefinitions::default();
    let mut add = |name: &str, bytes: &'static [u8]| {
        fonts.font_data.insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    };
    add("Inter", INTER_REGULAR);
    add("Inter-Medium", INTER_MEDIUM);
    add("Inter-SemiBold", INTER_SEMIBOLD);
    add("Inter-Bold", INTER_BOLD);
    add("JetBrainsMono", JETBRAINS_MONO);
    add("Phosphor", PHOSPHOR_REGULAR);
    add("Phosphor-Bold", PHOSPHOR_BOLD);
    add("Phosphor-Fill", PHOSPHOR_FILL);

    // Body text: Inter, then the icon font (so a glyph can be embedded in a string), then defaults.
    let prop = fonts.families.entry(FontFamily::Proportional).or_default();
    prop.insert(0, "Phosphor".to_owned());
    prop.insert(0, "Inter".to_owned());

    let mono = fonts.families.entry(FontFamily::Monospace).or_default();
    mono.insert(0, "JetBrainsMono".to_owned());

    // Weights are separate families because the subsets are static fonts.
    let named = |first: &str| vec![first.to_owned(), "Inter".to_owned(), "Phosphor".to_owned()];
    fonts.families.insert(FontFamily::Name("inter-medium".into()), named("Inter-Medium"));
    fonts.families.insert(FontFamily::Name("inter-semibold".into()), named("Inter-SemiBold"));
    fonts.families.insert(FontFamily::Name("inter-bold".into()), named("Inter-Bold"));
    fonts.families.insert(FontFamily::Name("phosphor".into()), vec!["Phosphor".to_owned()]);
    fonts.families.insert(FontFamily::Name("phosphor-bold".into()), vec!["Phosphor-Bold".to_owned()]);
    fonts.families.insert(FontFamily::Name("phosphor-fill".into()), vec!["Phosphor-Fill".to_owned()]);
    fonts
}

/// Registers the fonts with egui.
pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(definitions());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_family_resolves_to_registered_fonts() {
        let d = definitions();
        for (family, names) in &d.families {
            assert!(!names.is_empty(), "{family:?} is empty");
            for n in names {
                assert!(d.font_data.contains_key(n), "{family:?} refers to the unknown font {n}");
            }
        }
        for f in ["inter-medium", "inter-semibold", "inter-bold", "phosphor", "phosphor-bold", "phosphor-fill"] {
            assert!(d.families.contains_key(&FontFamily::Name(f.into())), "{f}");
        }
    }

    #[test]
    fn embedded_fonts_are_valid_and_small() {
        let total = [INTER_REGULAR, INTER_MEDIUM, INTER_SEMIBOLD, INTER_BOLD, JETBRAINS_MONO, PHOSPHOR_REGULAR, PHOSPHOR_BOLD, PHOSPHOR_FILL]
            .iter()
            .map(|b| {
                assert!(b.len() > 1000);
                assert!(matches!(&b[..4], [0, 1, 0, 0] | b"OTTO" | b"true"), "not a TrueType/OpenType file");
                b.len()
            })
            .sum::<usize>();
        assert!(total < 2_000_000, "fonts weigh {total} bytes");
    }
}

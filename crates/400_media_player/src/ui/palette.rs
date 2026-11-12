//! The command palette (Ctrl+K): every action, the recent files and the quick settings behind a
//! single search box.

use egui::{Align2, Color32, Context, CornerRadius, FontId, Id, Key, Margin, Order, Rect, Sense, Stroke, TextEdit, pos2, vec2};
use media_player_400_core::settings::{Accent, Language, ThemeMode};
use media_player_400_core::time::format_clock;

use crate::actions::{Action, DEFS, Group};
use crate::app::MediaPlayer400App;
use crate::control::SPEEDS;
use crate::i18n::{Key as T, t, t1, tf};
use crate::theme::{fade, fonts, mix};
use crate::ui::icons;
use crate::util::{file_name_of, format_speed};
use crate::widgets::kbd;

/// What choosing an entry does.
#[derive(Clone, Debug, PartialEq)]
pub enum EntryKind {
    Action(Action),
    Recent(String),
    Theme(ThemeMode),
    Accent(Accent),
    Language(Language),
    Speed(f64),
}

/// A line of the palette.
#[derive(Clone, Debug)]
pub struct Entry {
    pub kind: EntryKind,
    pub title: String,
    pub detail: Option<String>,
    pub icon: char,
    pub shortcut: Option<String>,
    /// Lower-cased, accent-free text the query is matched against.
    haystack: String,
}

impl Entry {
    fn new(kind: EntryKind, title: String, detail: Option<String>, icon: char, shortcut: Option<String>) -> Self {
        let haystack = fold(&format!("{} {} {}", title, detail.as_deref().unwrap_or(""), shortcut.as_deref().unwrap_or("")));
        Self { kind, title, detail, icon, shortcut, haystack }
    }
}

/// Lower-case and strip the accents of the Latin letters we ship translations for.
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars().flat_map(char::to_lowercase) {
        match c {
            'à' | 'â' | 'ä' | 'á' | 'ã' | 'å' => out.push('a'),
            'é' | 'è' | 'ê' | 'ë' => out.push('e'),
            'î' | 'ï' | 'í' | 'ì' => out.push('i'),
            'ô' | 'ö' | 'ó' | 'ò' | 'õ' => out.push('o'),
            'ù' | 'û' | 'ü' | 'ú' => out.push('u'),
            'ç' => out.push('c'),
            'ñ' => out.push('n'),
            'ÿ' => out.push('y'),
            'œ' => out.push_str("oe"),
            'æ' => out.push_str("ae"),
            '\u{a0}' | '\u{202f}' => out.push(' '),
            _ => out.push(c),
        }
    }
    out
}

/// Relevance of `entry` for `query` (`None` = no match). Every word of the query must match.
pub fn score(entry: &Entry, query: &str) -> Option<i32> {
    let q = fold(query);
    let mut total = 0;
    for word in q.split_whitespace() {
        total += score_word(&entry.haystack, &fold(&entry.title), word)?;
    }
    Some(total)
}

fn score_word(haystack: &str, title: &str, word: &str) -> Option<i32> {
    if title.starts_with(word) {
        return Some(100);
    }
    if title.split(|c: char| !c.is_alphanumeric()).any(|w| w.starts_with(word)) {
        return Some(80);
    }
    if let Some(pos) = title.find(word) {
        return Some(60 - (pos as i32).min(30));
    }
    if haystack.contains(word) {
        return Some(30);
    }
    // Letters in order, anywhere in the title ("pbp" → "Piste audio… suivante" is too loose; ask 3+).
    if word.chars().count() >= 3 {
        let mut it = title.chars();
        if word.chars().all(|c| it.any(|t| t == c)) {
            return Some(10);
        }
    }
    None
}

impl MediaPlayer400App {
    /// Builds the list of everything the palette can do right now.
    fn build_palette(&self) -> Vec<Entry> {
        let has_media = self.source.is_some();
        let has_video = self.media.as_ref().is_some_and(|m| m.has_video());
        let mut out: Vec<Entry> = Vec::new();

        for def in DEFS.iter().filter(|d| d.in_palette) {
            let relevant = match def.group {
                Group::Files | Group::Window => true,
                _ => has_media || matches!(def.action, Action::OpenEqualizer | Action::ToggleFullscreen | Action::ToggleNightMode | Action::CycleDeinterlace),
            };
            if !relevant || (!has_video && matches!(def.group, Group::Picture) && !matches!(def.action, Action::ToggleFullscreen | Action::CycleDeinterlace)) {
                continue;
            }
            let icon = match def.group {
                Group::Playback => icons::PLAY_CIRCLE,
                Group::Navigation => icons::CLOCK,
                Group::Audio => icons::HEADPHONES,
                Group::Subtitles => icons::SUBTITLES,
                Group::Picture => icons::MONITOR,
                Group::Files => icons::FOLDER_OPEN,
                Group::Window => icons::COMMAND,
            };
            out.push(Entry::new(
                EntryKind::Action(def.action),
                def.action.label(&self.settings),
                Some(def.group.label().to_owned()),
                icon,
                Some(def.action.shortcuts_label()).filter(|s| !s.is_empty()),
            ));
        }

        if has_media {
            for s in SPEEDS {
                out.push(Entry::new(EntryKind::Speed(s), t1(T::SpeedValue, format_speed(s)), Some(t(T::GroupPlayback).to_owned()), icons::GAUGE, None));
            }
        }

        let recent_label = t(T::Recent);
        for e in self.history.recent(8) {
            let name = e.title.clone().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| file_name_of(&e.source));
            let detail = if e.position > 10.0 { format!("{recent_label} · {}", tf(T::ResumeAt, &[&format_clock(e.position)])) } else { recent_label.to_owned() };
            out.push(Entry::new(EntryKind::Recent(e.source.clone()), name, Some(detail), icons::CLOCK, None));
        }

        let look = t(T::Appearance);
        for (mode, key) in [(ThemeMode::Dark, T::ThemeDark), (ThemeMode::Oled, T::ThemeOled), (ThemeMode::Light, T::ThemeLight), (ThemeMode::System, T::ThemeSystem)] {
            out.push(Entry::new(EntryKind::Theme(mode), t1(T::SetTheme, t(key)), Some(look.to_owned()), icons::PALETTE, None));
        }
        for a in Accent::ALL {
            out.push(Entry::new(EntryKind::Accent(a), t1(T::SetAccent, accent_name(a)), Some(look.to_owned()), icons::PALETTE, None));
        }
        for (lang, label) in [(Language::Auto, t(T::Automatic)), (Language::French, "Français"), (Language::English, "English")] {
            out.push(Entry::new(EntryKind::Language(lang), t1(T::SetLanguage, label), Some(look.to_owned()), icons::TRANSLATE, None));
        }
        out
    }

    pub fn draw_palette(&mut self, ctx: &Context) {
        let Some(mut state) = self.windows.palette.take() else { return };
        let p = self.pal.clone();
        if state.first_frame {
            self.palette_cache = self.build_palette();
        }
        let screen = ctx.content_rect();

        // Filter.
        let query = state.query.trim().to_owned();
        let mut results: Vec<(i32, usize)> = if query.is_empty() {
            // Without a query: the recent files first, then the commands in table order.
            let mut v: Vec<(i32, usize)> = self
                .palette_cache
                .iter()
                .enumerate()
                .filter(|(_, e)| !matches!(e.kind, EntryKind::Theme(_) | EntryKind::Accent(_) | EntryKind::Language(_) | EntryKind::Speed(_)))
                .map(|(i, e)| (if matches!(e.kind, EntryKind::Recent(_)) { 1000 } else { 0 }, i))
                .collect();
            v.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            v
        } else {
            let mut v: Vec<(i32, usize)> = self.palette_cache.iter().enumerate().filter_map(|(i, e)| score(e, &query).map(|s| (s, i))).collect();
            v.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            v
        };
        results.truncate(60);

        // Keyboard navigation (the text field keeps the focus).
        let (down, up, enter, page_down, page_up) =
            ctx.input(|i| (i.key_pressed(Key::ArrowDown), i.key_pressed(Key::ArrowUp), i.key_pressed(Key::Enter), i.key_pressed(Key::PageDown), i.key_pressed(Key::PageUp)));
        let n = results.len();
        let mut moved = false;
        if n > 0 {
            if down {
                state.selected = (state.selected + 1) % n;
                moved = true;
            }
            if up {
                state.selected = (state.selected + n - 1) % n;
                moved = true;
            }
            if page_down {
                state.selected = (state.selected + 6).min(n - 1);
                moved = true;
            }
            if page_up {
                state.selected = state.selected.saturating_sub(6);
                moved = true;
            }
        }
        state.selected = state.selected.min(n.saturating_sub(1));

        // Scrim.
        let mut close = false;
        egui::Area::new(Id::unique("palette-scrim")).order(Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
            let r = ui.allocate_response(screen.size(), Sense::click());
            ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(140));
            if r.clicked() {
                close = true;
            }
        });

        let width = (screen.width() - 32.0).clamp(300.0, 620.0);
        let top = (screen.height() * 0.13).max(40.0);
        let mut chosen: Option<usize> = None;
        let mut hovered: Option<usize> = None;
        let mut query_changed = false;

        egui::Area::new(Id::unique("palette")).order(Order::Foreground).pivot(Align2::CENTER_TOP).fixed_pos(pos2(screen.center().x, top)).constrain(true).show(ctx, |ui| {
            p.window_frame().inner_margin(Margin::same(10)).show(ui, |ui| {
                ui.set_width(width - 22.0);
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);

                // Search box.
                ui.horizontal(|ui| {
                    ui.set_min_height(46.0);
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(icons::SEARCH.to_string()).font(FontId::new(20.0, fonts::icons_bold())).color(p.accent));
                    ui.add_space(8.0);
                    let edit = TextEdit::singleline(&mut state.query)
                        .hint_text(egui::RichText::new(t(T::PaletteHint)).color(p.text_faint))
                        .font(FontId::new(17.0, fonts::medium()))
                        .frame(egui::Frame::NONE)
                        .desired_width(ui.available_width() - 56.0)
                        .margin(Margin::symmetric(4, 0))
                        .min_size(vec2(0.0, 46.0))
                        .align(Align2::LEFT_CENTER);
                    let resp = ui.add(edit);
                    if state.first_frame {
                        resp.request_focus();
                    }
                    query_changed = resp.changed();
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        kbd(ui, &p, "Esc");
                    });
                });
                let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
                ui.painter().rect_filled(line, 0.0, p.border);
                ui.add_space(6.0);

                if results.is_empty() {
                    ui.add_space(26.0);
                    ui.vertical_centered(|ui| {
                        ui.label(egui::RichText::new(icons::SEARCH.to_string()).font(FontId::new(30.0, fonts::icons())).color(p.text_faint));
                        ui.add_space(6.0);
                        ui.label(egui::RichText::new(t(T::PaletteNoResult)).font(FontId::new(14.5, fonts::medium())).color(p.text_dim));
                    });
                    ui.add_space(30.0);
                } else {
                    let max_h = (screen.height() - top - 120.0).clamp(160.0, 420.0);
                    ui.allocate_ui_with_layout(vec2(ui.available_width(), max_h), egui::Layout::top_down(egui::Align::Min), |ui| {
                        egui::ScrollArea::vertical().max_height(max_h).auto_shrink([false, true]).show_rows(ui, 46.0, results.len(), |ui, range| {
                            for row in range {
                                let (score_, idx) = results[row];
                                let _ = score_;
                                let entry = &self.palette_cache[idx];
                                let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), Sense::click());
                                let selected = row == state.selected;
                                if selected {
                                    if moved {
                                        ui.scroll_to_rect(rect, None);
                                    }
                                    ui.painter().rect_filled(rect.shrink2(vec2(0.0, 1.0)), CornerRadius::same(10), p.accent_faint);
                                    ui.painter().rect_filled(Rect::from_min_size(rect.min + vec2(0.0, 12.0), vec2(3.0, rect.height() - 24.0)), CornerRadius::same(2), p.accent);
                                }
                                if resp.hovered() && ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
                                    hovered = Some(row);
                                }
                                let icon_color = if selected { p.accent } else { p.text_dim };
                                ui.painter().text(
                                    pos2(rect.left() + 26.0, rect.center().y),
                                    Align2::CENTER_CENTER,
                                    entry.icon.to_string(),
                                    FontId::new(19.0, fonts::icons()),
                                    icon_color,
                                );
                                let title_y = if entry.detail.is_some() { rect.center().y - 8.0 } else { rect.center().y };
                                ui.painter().text(
                                    pos2(rect.left() + 52.0, title_y),
                                    Align2::LEFT_CENTER,
                                    &entry.title,
                                    FontId::new(14.5, fonts::medium()),
                                    if selected { p.text } else { mix(p.text, p.text_dim, 0.25) },
                                );
                                if let Some(d) = &entry.detail {
                                    ui.painter().text(pos2(rect.left() + 52.0, rect.center().y + 10.0), Align2::LEFT_CENTER, d, FontId::new(11.5, fonts::medium()), p.text_faint);
                                }
                                if let Some(s) = &entry.shortcut {
                                    let font = FontId::new(11.5, fonts::mono());
                                    let g = ui.painter().layout_no_wrap(s.clone(), font, Color32::PLACEHOLDER);
                                    let size = vec2((g.size().x + 14.0).max(24.0), 22.0);
                                    let r = Rect::from_center_size(pos2(rect.right() - 12.0 - size.x * 0.5, rect.center().y), size);
                                    ui.painter().rect_filled(r, CornerRadius::same(6), p.sunken);
                                    ui.painter().rect_stroke(r, CornerRadius::same(6), Stroke::new(1.0, p.border_strong), egui::StrokeKind::Inside);
                                    ui.painter().galley(r.center() - g.size() * 0.5, g, p.text_dim);
                                }
                                if resp.clicked() {
                                    chosen = Some(row);
                                }
                            }
                        })
                    });
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(format!("↑↓  {}     ↵  {}", t(T::PaletteNavigate), t(T::PaletteRun)))
                            .font(FontId::new(11.5, fonts::medium()))
                            .color(fade(p.text_faint, 0.95)),
                    );
                });
            });
        });

        if let Some(row) = hovered {
            state.selected = row;
        }
        if query_changed {
            state.selected = 0;
        }
        if enter && !results.is_empty() && chosen.is_none() {
            chosen = Some(state.selected);
        }
        state.first_frame = false;

        match chosen {
            Some(row) => {
                if let Some(&(_, idx)) = results.get(row) {
                    let kind = self.palette_cache[idx].kind.clone();
                    self.run_entry(kind);
                }
                // The palette closes unless the entry opened it again.
            }
            None if close => {}
            None => self.windows.palette = Some(state),
        }
    }

    fn run_entry(&mut self, kind: EntryKind) {
        match kind {
            EntryKind::Action(a) => self.run(a),
            EntryKind::Recent(src) => self.open_inputs(vec![src], false),
            EntryKind::Speed(s) => self.set_speed(s),
            EntryKind::Theme(m) => {
                self.settings.theme = m;
                self.apply_look();
                self.touch_settings();
            }
            EntryKind::Accent(a) => {
                self.settings.accent = a;
                self.apply_look();
                self.touch_settings();
            }
            EntryKind::Language(l) => {
                self.settings.language = l;
                self.apply_look();
                self.touch_settings();
            }
        }
    }
}

/// Localised name of an accent colour.
pub fn accent_name(a: Accent) -> &'static str {
    t(match a {
        Accent::Violet => T::AccentViolet,
        Accent::Sky => T::AccentSky,
        Accent::Emerald => T::AccentEmerald,
        Accent::Rose => T::AccentRose,
        Accent::Amber => T::AccentAmber,
        Accent::Orange => T::AccentOrange,
        Accent::Fuchsia => T::AccentFuchsia,
        Accent::Cyan => T::AccentCyan,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(title: &str, detail: &str) -> Entry {
        Entry::new(EntryKind::Action(Action::Stop), title.to_owned(), Some(detail.to_owned()), icons::STOP, None)
    }

    #[test]
    fn folding() {
        assert_eq!(fold("Égaliseur"), "egaliseur");
        assert_eq!(fold("Œuvre Çà"), "oeuvre ca");
        assert_eq!(fold("Vitesse\u{a0}: 2"), "vitesse : 2");
    }

    #[test]
    fn matching_and_ranking() {
        let e = entry("Égaliseur", "Audio");
        assert!(score(&e, "egal").unwrap() >= 100, "accents are ignored and a prefix wins");
        assert!(score(&e, "ÉGA").is_some());
        assert!(score(&e, "audio").is_some(), "the detail is searched too");
        assert!(score(&e, "xyz").is_none());
        let a = entry("Plein écran", "Image");
        let b = entry("Basculer le plein écran", "Image");
        assert!(score(&a, "plein").unwrap() > score(&b, "plein").unwrap(), "a title that starts with the word ranks higher");
        assert!(score(&b, "plein ecran").is_some(), "every word has to match");
        assert!(score(&b, "plein volume").is_none());
        assert!(score(&e, "eqz").is_none(), "a loose subsequence needs every letter in order");
    }

    #[test]
    fn every_accent_has_a_name() {
        let _guard = crate::i18n::test_lock();
        for a in Accent::ALL {
            assert!(!accent_name(a).is_empty());
        }
    }
}

//! The screen shown when nothing is loaded: logo, the three ways to open something, and the
//! recently played media with their resume points.

use egui::{Align2, Color32, CornerRadius, FontId, Id, Rect, RichText, Sense, Stroke, StrokeKind, Ui, UiBuilder, pos2, vec2};
use lumen400_core::media_kind::{PathKind, classify, is_url};
use lumen400_core::playlist::PlaylistItem;
use lumen400_core::settings::HistoryEntry;
use lumen400_core::time::format_clock;

use crate::actions::Action;
use crate::app::LumenApp;
use crate::i18n::{Key as T, t, tf};
use crate::theme::{self, Palette, fade, fonts, mix};
use crate::ui::icons;
use crate::util::ellipsize_middle;
use crate::widgets::{self, IconButton, button, button_width, icon_text, kbd};

/// What one card of the "recent" list asks for.
enum RecentAction {
    Play(String),
    Restart(String),
    Remove(String),
}

impl LumenApp {
    pub fn draw_welcome(&mut self, ui: &mut Ui, stage: Rect, now: f64) {
        if self.settings.advanced_mode {
            self.draw_advanced_welcome(ui, stage, now);
            return;
        }

        let p = self.pal.clone();
        let painter = ui.painter_at(stage);
        let appear = ui.ctx().animate_bool_with_time(Id::unique("welcome-appear"), true, 0.9);
        paint_aurora(&painter, stage, &p, appear);

        let recents: Vec<HistoryEntry> = self.history.recent(8).to_vec();
        let col_w = 640.0f32.min(stage.width() - 48.0).max(260.0);
        // How many recent items fit under the header.
        let header_h = 330.0;
        let card_h = 60.0;
        let fit = (((stage.height() - header_h - 70.0) / card_h).floor() as isize).clamp(0, recents.len() as isize) as usize;
        let list_h = if fit > 0 { 40.0 + fit as f32 * card_h } else { 0.0 };
        let total = header_h + list_h + 40.0;
        let top = (stage.top() + (stage.height() - total) * 0.5).max(stage.top() + 28.0);
        let col = Rect::from_min_size(pos2(stage.center().x - col_w * 0.5, top), vec2(col_w, (stage.bottom() - top).max(100.0)));

        let mut action: Option<RecentAction> = None;
        let mut open_actions: Vec<Action> = Vec::new();
        let mut clear_history = false;
        ui.scope_builder(UiBuilder::new().max_rect(col).layout(egui::Layout::top_down(egui::Align::Min)), |ui| {
            ui.set_opacity(appear);
            ui.spacing_mut().item_spacing = vec2(10.0, 10.0);

            // --- logo and name ------------------------------------------------------------------
            let (logo, _) = ui.allocate_exact_size(vec2(col_w, 82.0), Sense::hover());
            paint_logo(ui, Rect::from_center_size(logo.center(), vec2(78.0, 78.0)), &p);

            let name_font = FontId::new(46.0, fonts::bold());
            let mut job = egui::text::LayoutJob::default();
            job.append("400_", 0.0, egui::TextFormat { font_id: name_font.clone(), color: p.text, ..Default::default() });
            job.append("Player", 2.0, egui::TextFormat { font_id: name_font, color: p.accent, ..Default::default() });
            let galley = ui.painter().layout_job(job);
            let (name_rect, _) = ui.allocate_exact_size(vec2(col_w, galley.size().y + 2.0), Sense::hover());
            ui.painter().galley(pos2(name_rect.center().x - galley.size().x * 0.5, name_rect.top()), galley, p.text);

            let (tag, _) = ui.allocate_exact_size(vec2(col_w, 22.0), Sense::hover());
            ui.painter().text(tag.center(), Align2::CENTER_CENTER, t(T::Tagline), FontId::new(15.0, fonts::medium()), p.text_dim);
            ui.add_space(10.0);

            // --- the four ways in (including Mode Avancé) ----------------------------------------
            let labels = [
                (icons::FOLDER_OPEN, t(T::OpenFile), true, Some(Action::OpenFile)),
                (icons::FILM_STRIP, t(T::OpenFolderShort), false, Some(Action::OpenFolder)),
                (icons::LINK, t(T::OpenUrlShort), false, Some(Action::OpenUrl)),
                (icons::SPARKLE, "Mode Avancé", false, None),
            ];
            let gap = 10.0;
            let widths: Vec<f32> = labels.iter().map(|(i, l, ..)| button_width(ui, l, true) + if *i == icons::FOLDER_OPEN { 8.0 } else { 0.0 }).collect();
            let row_w: f32 = widths.iter().sum::<f32>() + gap * (labels.len() as f32 - 1.0);
            let (row, _) = ui.allocate_exact_size(vec2(col_w, 40.0), Sense::hover());
            let mut x = row.center().x - row_w * 0.5;
            for (i, (icon, label, primary, act)) in labels.iter().enumerate() {
                let r = Rect::from_min_size(pos2(x, row.top()), vec2(widths[i], 40.0));
                let resp = ui.scope_builder(UiBuilder::new().max_rect(r), |ui| button(ui, &p, Some(*icon), label, *primary)).inner;
                if resp.clicked() {
                    if let Some(a) = act {
                        open_actions.push(*a);
                    } else {
                        self.settings.advanced_mode = true;
                        self.save_settings_now();
                    }
                }
                x += widths[i] + gap;
            }

            let (hint, _) = ui.allocate_exact_size(vec2(col_w, 24.0), Sense::hover());
            let hint_color = fade(p.text_faint, 0.95);
            ui.painter().text(hint.center(), Align2::CENTER_CENTER, format!("{}  {}", icons::DOWNLOAD, t(T::DropHint)), FontId::new(13.0, fonts::medium()), hint_color);

            // --- recent -------------------------------------------------------------------------
            if fit > 0 {
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    widgets::section_title(ui, &p, t(T::Recent));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let r = ui.add(egui::Label::new(RichText::new(t(T::ClearHistory)).size(12.0).color(p.text_faint)).sense(Sense::click()));
                        if r.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if r.clicked() {
                            clear_history = true;
                        }
                    });
                });
                for entry in recents.iter().take(fit) {
                    if let Some(a) = recent_card(ui, &p, entry, col_w) {
                        action = Some(a);
                    }
                }
            }
        });

        // Footer: the three shortcuts worth knowing.
        let foot = Rect::from_center_size(pos2(stage.center().x, stage.bottom() - 28.0), vec2(col_w, 24.0));
        ui.scope_builder(UiBuilder::new().max_rect(foot).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
            ui.set_opacity(appear * 0.9);
            let items = [(Action::OpenFile, T::OpenFiles), (Action::OpenPalette, T::CommandPalette), (Action::ShowShortcuts, T::Shortcuts)];
            let mut total_w = 0.0;
            let mut parts = Vec::new();
            for (a, k) in items {
                let key = a.shortcut_label().unwrap_or_default();
                let label = t(k);
                let w = ui.painter().layout_no_wrap(label.to_owned(), FontId::new(12.5, fonts::medium()), p.text_faint).size().x
                    + ui.painter().layout_no_wrap(key.clone(), FontId::new(11.5, fonts::mono()), p.text_faint).size().x
                    + 12.0
                    + 8.0
                    + 22.0;
                total_w += w;
                parts.push((key, label, w));
            }
            ui.add_space(((col_w - total_w) * 0.5).max(0.0));
            for (key, label, _) in parts {
                kbd(ui, &p, &key);
                ui.label(RichText::new(label).size(12.5).color(p.text_faint));
                ui.add_space(14.0);
            }
        });

        for a in open_actions {
            self.run(a);
        }
        if clear_history {
            self.history.clear();
            self.history_dirty = true;
        }
        match action {
            Some(RecentAction::Play(src)) => self.open_inputs(vec![src], false),
            Some(RecentAction::Restart(src)) => {
                self.playlist.clear();
                let id = self.playlist.add(src.clone());
                self.playlist.set_current(id);
                self.play_source(&src, Some(0.0));
            }
            Some(RecentAction::Remove(src)) => {
                self.history.remove(&src);
                self.history_dirty = true;
            }
            None => {}
        }
        let _ = now;
    }
}

/// Soft coloured lights behind the welcome content.
fn paint_aurora(painter: &egui::Painter, area: Rect, p: &Palette, appear: f32) {
    let (w, h) = (area.width(), area.height());
    let big = w.max(h);
    let a = p.accent_glass;
    let warm = mix(a, Color32::from_rgb(232, 121, 249), 0.55);
    let cool = mix(a, Color32::from_rgb(56, 189, 248), 0.6);
    let k = if p.dark { 1.0 } else { 0.55 };
    let lights = [
        (pos2(area.left() + w * 0.22, area.top() + h * 0.18), big * 0.62, fade(a, 0.30 * k * appear)),
        (pos2(area.left() + w * 0.86, area.top() + h * 0.62), big * 0.55, fade(warm, 0.20 * k * appear)),
        (pos2(area.left() + w * 0.45, area.top() + h * 1.08), big * 0.60, fade(cool, 0.20 * k * appear)),
    ];
    for (c, r, col) in lights {
        theme::paint_glow(painter, c, r, col);
    }
}

/// The 400Player brand mark.
pub fn paint_logo(ui: &mut Ui, rect: Rect, p: &Palette) {
    let tex = ui.ctx().load_texture(
        "400player-logo",
        egui::ColorImage::from_rgba_unmultiplied(
            [128, 128],
            include_bytes!("../../../../assets/logo_bg_128.rgba"),
        ),
        egui::TextureOptions::LINEAR,
    );
    theme::paint_glow(ui.painter(), rect.center(), rect.width() * 1.5, fade(p.accent_glass, 0.28));
    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    ui.painter().image(tex.id(), rect, uv, Color32::WHITE);
}

/// One recent item; returns what the user clicked.
fn recent_card(ui: &mut Ui, p: &Palette, e: &HistoryEntry, width: f32) -> Option<RecentAction> {
    let height = 56.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    let hover = ui.ctx().animate_bool_with_time(resp.id.with("hover"), resp.hovered(), 0.12);
    let cr = CornerRadius::same(theme::radius::CARD);
    ui.painter().rect_filled(rect, cr, mix(fade(p.raised, 0.78), p.hover, hover));
    ui.painter().rect_stroke(rect, cr, Stroke::new(1.0, mix(p.border, p.accent, hover * 0.55)), StrokeKind::Inside);

    let is_remote = is_url(&e.source) && !lumen400_core::media_kind::is_file_url(&e.source);
    let path = std::path::Path::new(&e.source);
    let icon = if is_remote {
        icons::GLOBE
    } else if classify(path) == PathKind::Audio {
        icons::MUSIC_NOTES
    } else {
        icons::FILM_STRIP
    };
    // Icon tile.
    let tile = Rect::from_min_size(rect.left_top() + vec2(10.0, 10.0), vec2(36.0, 36.0));
    ui.painter().rect_filled(tile, CornerRadius::same(10), p.accent_faint);
    ui.painter().text(tile.center(), Align2::CENTER_CENTER, icon.to_string(), FontId::new(19.0, fonts::icons()), p.accent);

    let item = PlaylistItem { id: 0, source: e.source.clone(), title: e.title.clone(), duration: None };
    let name = ellipsize_middle(&item.display_name(), ((width - 150.0) / 7.6) as usize);
    let tx = tile.right() + 12.0;
    ui.painter().text(pos2(tx, rect.top() + 11.0), Align2::LEFT_TOP, name, FontId::new(14.5, fonts::medium()), p.text);
    let meta = if e.duration > 0.0 {
        if e.position >= 10.0 && e.progress() < 0.97 {
            format!("{} / {}  ·  {}", format_clock(e.position), format_clock(e.duration), tf(T::WatchedPercent, &[&((e.progress() * 100.0).round() as i32)]))
        } else {
            format_clock(e.duration)
        }
    } else if is_remote {
        t(T::NetworkStream).to_owned()
    } else {
        String::new()
    };
    ui.painter().text(pos2(tx, rect.top() + 32.0), Align2::LEFT_TOP, meta, FontId::new(12.0, fonts::medium()), p.text_dim);

    // Watched progress along the bottom edge.
    if e.progress() > 0.01 && e.progress() < 0.97 {
        let bar = Rect::from_min_max(pos2(rect.left() + 14.0, rect.bottom() - 4.0), pos2(rect.right() - 14.0, rect.bottom() - 2.0));
        ui.painter().rect_filled(bar, CornerRadius::same(2), fade(p.text, 0.08));
        let filled = Rect::from_min_max(bar.min, pos2(bar.left() + bar.width() * e.progress(), bar.bottom()));
        ui.painter().rect_filled(filled, CornerRadius::same(2), p.accent);
    }

    let mut result = None;
    if hover > 0.01 {
        // Restart and remove appear on hover, over the right edge.
        let mut x = rect.right() - 12.0 - 30.0;
        let tint = hover;
        let remove = Rect::from_min_size(pos2(x, rect.center().y - 15.0), vec2(30.0, 30.0));
        let r = ui.interact(remove, resp.id.with("remove"), Sense::click());
        ui.painter().text(
            remove.center(),
            Align2::CENTER_CENTER,
            icons::X.to_string(),
            FontId::new(15.0, fonts::icons()),
            fade(if r.hovered() { p.danger } else { p.text_dim }, tint),
        );
        if r.clicked() {
            result = Some(RecentAction::Remove(e.source.clone()));
        }
        let r = r.on_hover_text(t(T::RemoveFromList));
        let _ = r;
        if e.position >= 10.0 && e.progress() < 0.97 {
            x -= 34.0;
            let restart = Rect::from_min_size(pos2(x, rect.center().y - 15.0), vec2(30.0, 30.0));
            let r = ui.interact(restart, resp.id.with("restart"), Sense::click());
            ui.painter().text(
                restart.center(),
                Align2::CENTER_CENTER,
                icons::ARROW_CCW.to_string(),
                FontId::new(16.0, fonts::icons()),
                fade(if r.hovered() { p.accent } else { p.text_dim }, tint),
            );
            if r.clicked() {
                result = Some(RecentAction::Restart(e.source.clone()));
            }
            let _ = r.on_hover_text(t(T::PlayFromStart));
        }
    }
    if result.is_none() && resp.clicked() {
        result = Some(RecentAction::Play(e.source.clone()));
    }
    let _ = (IconButton::new(icons::X), icon_text(icons::X, 1.0, Color32::WHITE));
    result
}

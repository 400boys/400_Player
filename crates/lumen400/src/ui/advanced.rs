//! The Advanced Studio & Library Dashboard mode for 400_Player.
//!
//! Inspired by the modern HTML 400Player interface, providing:
//! - A 212px sidebar with 400Player branding, navigation (Accueil, Vidéos, Musique, Favoris, etc.)
//! - A top header with search, real-time Skin/Theme switcher (Menthe, Iris, Ambre, Rose, etc.)
//! - "Mode Simple" switch button to toggle back to the default minimalist welcome screen
//! - Hero banner "VOTRE NOUVEL ESPACE MÉDIA — Votre cinéma. Votre rythme."
//! - "Reprendre le fil" section with progress bars for recently watched media
//! - Full interactive media cards grid with instant playback and favorites

use std::path::Path;
use egui::{
    Align, Align2, Color32, CornerRadius, FontId, Layout, Margin, Rect, RichText, Sense,
    Stroke, StrokeKind, Ui, UiBuilder, pos2, vec2,
};
use lumen400_core::media_kind::{PathKind, classify};
use lumen400_core::settings::{Accent, HistoryEntry, ThemeMode};
use lumen400_core::time::format_clock;

use crate::actions::Action;
use crate::app::LumenApp;
use crate::theme::{Palette, fade, fonts};
use crate::ui::icons;
use crate::util::ellipsize_middle;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AdvancedTab {
    #[default]
    Home,
    Videos,
    Music,
    Favorites,
}

impl LumenApp {
    /// Renders the complete Advanced Studio & Library view over `stage`.
    pub fn draw_advanced_welcome(&mut self, ui: &mut Ui, stage: Rect, now: f64) {
        let p = self.pal.clone();
        let _ = now;

        // Background panel fill
        ui.painter().rect_filled(stage, 0.0, p.panel);

        const SIDEBAR_W: f32 = 212.0;
        let sidebar_rect = Rect::from_min_size(stage.min, vec2(SIDEBAR_W, stage.height()));
        let content_rect = Rect::from_min_size(
            pos2(stage.left() + SIDEBAR_W, stage.top()),
            vec2(stage.width() - SIDEBAR_W, stage.height()),
        );

        let mut action_open: Option<Action> = None;
        let mut play_source: Option<String> = None;
        let mut toggle_fav: Option<String> = None;

        // =========================================================================================
        // 1. LEFT SIDEBAR (212px)
        // =========================================================================================
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(sidebar_rect)
                .layout(Layout::top_down(Align::Min)),
            |ui| {
                ui.painter().rect_filled(sidebar_rect, 0.0, p.panel);
                ui.painter().line_segment(
                    [pos2(sidebar_rect.right(), sidebar_rect.top()), pos2(sidebar_rect.right(), sidebar_rect.bottom())],
                    Stroke::new(1.0, p.border),
                );

                ui.add_space(14.0);
                // Brand Header with 400Player logo emblem
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    let (mark_rect, mark_resp) = ui.allocate_exact_size(vec2(34.0, 34.0), Sense::click());
                    paint_emblem_logo(ui, mark_rect);
                    if mark_resp.clicked() {
                        self.advanced_tab = AdvancedTab::Home;
                    }
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            ui.label(RichText::new("400_").font(FontId::new(20.0, fonts::bold())).color(p.text));
                            ui.label(RichText::new("Player").font(FontId::new(20.0, fonts::bold())).color(p.accent));
                        });
                        ui.label(
                            RichText::new("LE SENS DU MÉDIA")
                                .font(FontId::new(8.5, fonts::bold()))
                                .color(p.text_faint),
                        );
                    });
                });

                ui.add_space(24.0);
                // Section: ESPACE PERSONNEL
                ui.horizontal(|ui| {
                    ui.add_space(18.0);
                    ui.label(
                        RichText::new("ESPACE PERSONNEL")
                            .font(FontId::new(9.5, fonts::bold()))
                            .color(p.text_dim),
                    );
                });
                ui.add_space(6.0);

                let nav_items = [
                    (AdvancedTab::Home, "Accueil", icons::FOLDER_OPEN),
                    (AdvancedTab::Videos, "Vidéos", icons::FILM_STRIP),
                    (AdvancedTab::Music, "Musique", icons::MUSIC_NOTES),
                    (AdvancedTab::Favorites, "Favoris", icons::BOOKMARK),
                ];

                for (tab, label, icon) in nav_items {
                    let active = self.advanced_tab == tab;
                    if sidebar_nav_button(ui, &p, label, icon, active, sidebar_rect.width() - 24.0) {
                        self.advanced_tab = tab;
                        self.advanced_search.clear();
                    }
                    ui.add_space(4.0);
                }

                ui.add_space(20.0);
                // Section: LECTURE
                ui.horizontal(|ui| {
                    ui.add_space(18.0);
                    ui.label(
                        RichText::new("LECTURE")
                            .font(FontId::new(9.5, fonts::bold()))
                            .color(p.text_dim),
                    );
                });
                ui.add_space(6.0);

                if sidebar_nav_button(ui, &p, "À l’écran", icons::MONITOR_PLAY, false, sidebar_rect.width() - 24.0) {
                    if let Some(recent) = self.history.recent(1).first() {
                        play_source = Some(recent.source.clone());
                    } else {
                        action_open = Some(Action::OpenFile);
                    }
                }
                ui.add_space(4.0);
                if sidebar_nav_button(ui, &p, "File d’attente", icons::LIST, self.windows.playlist, sidebar_rect.width() - 24.0) {
                    self.windows.playlist = !self.windows.playlist;
                }
                ui.add_space(4.0);
                if sidebar_nav_button(ui, &p, "Flux réseau", icons::GLOBE, false, sidebar_rect.width() - 24.0) {
                    action_open = Some(Action::OpenUrl);
                }

                // Push bottom area to the bottom
                let available_h = ui.available_height();
                if available_h > 150.0 {
                    ui.add_space(available_h - 150.0);
                }

                // Collection Card
                let card_w = sidebar_rect.width() - 28.0;
                let (card_r, _) = ui.allocate_exact_size(vec2(card_w, 76.0), Sense::hover());
                let card_rect = Rect::from_min_size(pos2(sidebar_rect.left() + 14.0, card_r.top()), vec2(card_w, 76.0));
                ui.painter().rect_filled(card_rect, CornerRadius::same(10), p.raised);
                ui.painter().rect_stroke(card_rect, CornerRadius::same(10), Stroke::new(1.0, p.border), StrokeKind::Inside);
                
                ui.painter().text(
                    pos2(card_rect.left() + 10.0, card_rect.top() + 8.0),
                    Align2::LEFT_TOP,
                    "Votre collection, ici.",
                    FontId::new(11.5, fonts::bold()),
                    p.text,
                );
                ui.painter().text(
                    pos2(card_rect.left() + 10.0, card_rect.top() + 24.0),
                    Align2::LEFT_TOP,
                    "Ajoutez vos dossiers pour retrouver\ntous vos médias.",
                    FontId::new(10.0, fonts::medium()),
                    p.text_dim,
                );

                let btn_add_r = Rect::from_min_size(pos2(card_rect.left() + 10.0, card_rect.top() + 50.0), vec2(card_w - 20.0, 20.0));
                let add_resp = ui.allocate_rect(btn_add_r, Sense::click());
                let add_hover = add_resp.hovered();
                ui.painter().rect_filled(btn_add_r, CornerRadius::same(5), if add_hover { p.hover } else { p.raised });
                ui.painter().rect_stroke(btn_add_r, CornerRadius::same(5), Stroke::new(1.0, p.border), StrokeKind::Inside);
                ui.painter().text(
                    btn_add_r.center(),
                    Align2::CENTER_CENTER,
                    "+  Ajouter un dossier",
                    FontId::new(10.5, fonts::medium()),
                    if add_hover { p.accent } else { p.text },
                );
                if add_resp.clicked() {
                    action_open = Some(Action::OpenFolder);
                }

                ui.add_space(8.0);
                if sidebar_nav_button(ui, &p, "Préférences", icons::GEAR, self.windows.settings.is_some(), card_w) {
                    action_open = Some(Action::OpenSettings);
                }

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.add_space(18.0);
                    ui.label(RichText::new("400Player v0.1.0").font(FontId::new(10.0, fonts::medium())).color(p.text_faint));
                });
            },
        );

        // =========================================================================================
        // 2. RIGHT MAIN CONTENT AREA
        // =========================================================================================
        const HEADER_H: f32 = 64.0;
        let header_rect = Rect::from_min_size(content_rect.min, vec2(content_rect.width(), HEADER_H));
        let scroll_rect = Rect::from_min_size(
            pos2(content_rect.left(), content_rect.top() + HEADER_H),
            vec2(content_rect.width(), content_rect.height() - HEADER_H),
        );

        // ---------------- TOP HEADER ----------------
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(header_rect)
                .layout(Layout::left_to_right(Align::Center)),
            |ui| {
                ui.painter().rect_filled(header_rect, 0.0, p.panel);
                ui.painter().line_segment(
                    [pos2(header_rect.left(), header_rect.bottom()), pos2(header_rect.right(), header_rect.bottom())],
                    Stroke::new(1.0, p.border),
                );

                ui.add_space(24.0);
                let title = match self.advanced_tab {
                    AdvancedTab::Home => "Bienvenue chez vous",
                    AdvancedTab::Videos => "Vos vidéos",
                    AdvancedTab::Music => "Votre musique",
                    AdvancedTab::Favorites => "Vos favoris",
                };
                ui.label(RichText::new(title).font(FontId::new(19.0, fonts::bold())).color(p.text));

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add_space(24.0);

                    // Button: Ouvrir un fichier (primary accent button)
                    let btn_open = ui.add(
                        egui::Button::new(
                            RichText::new(format!("{}  Ouvrir un fichier", icons::FOLDER_OPEN))
                                .font(FontId::new(12.5, fonts::semibold()))
                                .color(Color32::from_rgb(13, 17, 22)),
                        )
                        .fill(p.accent)
                        .corner_radius(CornerRadius::same(7))
                    );
                    if btn_open.clicked() {
                        action_open = Some(Action::OpenFile);
                    }

                    ui.add_space(8.0);

                    // Button: Mode Simple (switch back to default interface)
                    let btn_simple = ui.add(
                        egui::Button::new(
                            RichText::new(format!("{}  Mode Simple", icons::ARROWS_IN))
                                .font(FontId::new(12.0, fonts::medium()))
                                .color(p.text),
                        )
                        .fill(p.raised)
                        .stroke(Stroke::new(1.0, p.border))
                        .corner_radius(CornerRadius::same(7))
                    );
                    if btn_simple.on_hover_text("Revenir à l'écran d'accueil épuré standard").clicked() {
                        self.settings.advanced_mode = false;
                    }

                    ui.add_space(10.0);

                    // Quick Skin / Accent Switcher
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Skins:").font(FontId::new(11.5, fonts::medium())).color(p.text_dim));
                        
                        let skins = [
                            ("Menthe", Accent::Cyan, Color32::from_rgb(61, 230, 201)),
                            ("Iris", Accent::Violet, Color32::from_rgb(160, 149, 255)),
                            ("Ambre", Accent::Amber, Color32::from_rgb(255, 170, 43)),
                            ("Rose", Accent::Rose, Color32::from_rgb(251, 113, 133)),
                        ];

                        for (name, acc, col) in skins {
                            let is_curr = self.settings.accent == acc;
                            let pill = ui.add(
                                egui::Button::new(
                                    RichText::new(name)
                                        .font(FontId::new(11.5, fonts::medium()))
                                        .color(if is_curr { col } else { p.text_dim }),
                                )
                                .fill(if is_curr { fade(col, 0.18) } else { p.raised })
                                .stroke(Stroke::new(1.0, if is_curr { col } else { p.border }))
                                .corner_radius(CornerRadius::same(6))
                            );
                            if pill.on_hover_text(format!("Appliquer le skin {name}")).clicked() {
                                self.settings.accent = acc;
                                self.apply_look();
                                self.save_settings_now();
                            }
                        }

                        // Theme toggle (Dark / Light)
                        let is_dark = self.settings.theme != ThemeMode::Light;
                        let theme_icon = if is_dark { "\u{e4a8}" } else { "\u{e4a6}" };
                        let theme_btn = ui.add(
                            egui::Button::new(RichText::new(theme_icon).font(FontId::new(13.0, fonts::icons())).color(p.text_dim))
                                .fill(p.raised)
                                .stroke(Stroke::new(1.0, p.border))
                                .corner_radius(CornerRadius::same(6))
                        );
                        if theme_btn.on_hover_text(if is_dark { "Passer au thème clair" } else { "Passer au thème sombre" }).clicked() {
                            self.settings.theme = if is_dark { ThemeMode::Light } else { ThemeMode::Dark };
                            self.apply_look();
                            self.save_settings_now();
                        }
                    });

                    ui.add_space(10.0);

                    // Search Input
                    ui.add(
                        egui::TextEdit::singleline(&mut self.advanced_search)
                            .hint_text("Rechercher dans la bibliothèque")
                            .desired_width(210.0)
                            .margin(Margin::symmetric(10, 6)),
                    );
                });
            },
        );

        // ---------------- SCROLLABLE MAIN CONTENT ----------------
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(scroll_rect)
                .layout(Layout::top_down(Align::Min)),
            |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add_space(20.0);

                        // --- HERO BANNER (Only on Home when search is empty) ---
                        if self.advanced_tab == AdvancedTab::Home && self.advanced_search.trim().is_empty() {
                            ui.horizontal(|ui| {
                                ui.add_space(24.0);
                                let banner_w = (ui.available_width() - 24.0).max(300.0);
                                let banner_h = 190.0;
                                let (b_rect, _) = ui.allocate_exact_size(vec2(banner_w, banner_h), Sense::hover());
                                
                                // Gradient banner background
                                let bg_teal = Color32::from_rgb(23, 43, 46);
                                ui.painter().rect_filled(b_rect, CornerRadius::same(16), bg_teal);
                                ui.painter().rect_stroke(b_rect, CornerRadius::same(16), Stroke::new(1.0, fade(p.accent, 0.35)), StrokeKind::Inside);

                                // Concentric circles decoration on the right
                                let center_circ = pos2(b_rect.right() - 110.0, b_rect.center().y);
                                for r in [35.0, 50.0, 68.0, 88.0, 110.0, 135.0] {
                                    ui.painter().circle_stroke(center_circ, r, Stroke::new(1.0, fade(p.accent, 0.12)));
                                }
                                // Right emblem logo
                                let emblem_rect = Rect::from_center_size(center_circ, vec2(88.0, 88.0));
                                paint_emblem_logo(ui, emblem_rect);

                                // Left text and buttons inside banner
                                let left_box = Rect::from_min_size(
                                    pos2(b_rect.left() + 28.0, b_rect.top() + 22.0),
                                    vec2(b_rect.width() * 0.62, b_rect.height() - 44.0),
                                );
                                ui.painter().text(
                                    left_box.left_top(),
                                    Align2::LEFT_TOP,
                                    "VOTRE NOUVEL ESPACE MÉDIA",
                                    FontId::new(10.0, fonts::bold()),
                                    p.accent,
                                );
                                ui.painter().text(
                                    pos2(left_box.left(), left_box.top() + 18.0),
                                    Align2::LEFT_TOP,
                                    "Votre cinéma.\nVotre rythme.",
                                    FontId::new(26.0, fonts::bold()),
                                    Color32::from_rgb(234, 240, 246),
                                );
                                ui.painter().text(
                                    pos2(left_box.left(), left_box.top() + 84.0),
                                    Align2::LEFT_TOP,
                                    "Films, musique et instants à revivre avec aperçus instantanés.",
                                    FontId::new(12.5, fonts::medium()),
                                    Color32::from_rgb(166, 193, 191),
                                );

                                // Banner action buttons
                                let btn1_rect = Rect::from_min_size(pos2(left_box.left(), left_box.top() + 112.0), vec2(165.0, 32.0));
                                let resp1 = ui.allocate_rect(btn1_rect, Sense::click());
                                ui.painter().rect_filled(btn1_rect, CornerRadius::same(7), if resp1.hovered() { p.accent_glass } else { p.accent });
                                ui.painter().text(
                                    btn1_rect.center(),
                                    Align2::CENTER_CENTER,
                                    format!("{}  Explorer mes fichiers", icons::FOLDER_OPEN),
                                    FontId::new(11.5, fonts::semibold()),
                                    Color32::from_rgb(13, 17, 22),
                                );
                                if resp1.clicked() {
                                    action_open = Some(Action::OpenFile);
                                }

                                let btn2_rect = Rect::from_min_size(pos2(left_box.left() + 175.0, left_box.top() + 112.0), vec2(150.0, 32.0));
                                let resp2 = ui.allocate_rect(btn2_rect, Sense::click());
                                ui.painter().rect_filled(btn2_rect, CornerRadius::same(7), if resp2.hovered() { p.hover } else { Color32::from_black_alpha(80) });
                                ui.painter().rect_stroke(btn2_rect, CornerRadius::same(7), Stroke::new(1.0, Color32::from_white_alpha(30)), StrokeKind::Inside);
                                ui.painter().text(
                                    btn2_rect.center(),
                                    Align2::CENTER_CENTER,
                                    "+  Ajouter un dossier",
                                    FontId::new(11.5, fonts::medium()),
                                    Color32::from_rgb(234, 240, 246),
                                );
                                if resp2.clicked() {
                                    action_open = Some(Action::OpenFolder);
                                }
                            });
                            ui.add_space(24.0);
                        }

                        // --- SECTION: REPRENDRE LE FIL (Recent Resumable Media) ---
                        let recents = self.history.recent(12).to_vec();
                        let resumable: Vec<&HistoryEntry> = recents
                            .iter()
                            .filter(|e| e.progress() > 0.02 && e.progress() < 0.98)
                            .collect();

                        if (self.advanced_tab == AdvancedTab::Home) && !resumable.is_empty() && self.advanced_search.trim().is_empty() {
                            ui.horizontal(|ui| {
                                ui.add_space(24.0);
                                ui.label(RichText::new("Reprendre le fil").font(FontId::new(18.0, fonts::bold())).color(p.text));
                                ui.add_space(6.0);
                                ui.label(RichText::new("· Là où vous vous êtes arrêté").font(FontId::new(12.0, fonts::medium())).color(p.text_dim));
                            });
                            ui.add_space(10.0);

                            ui.horizontal(|ui| {
                                ui.add_space(24.0);
                                let avail_w = ui.available_width() - 24.0;
                                let cols = if avail_w > 900.0 { 3 } else if avail_w > 600.0 { 2 } else { 1 };
                                let card_w = ((avail_w - 14.0 * (cols as f32 - 1.0)) / cols as f32).max(180.0);

                                for chunk in resumable.chunks(cols) {
                                    ui.horizontal(|ui| {
                                        for entry in chunk {
                                            if let Some(src) = render_resume_card(ui, &p, entry, card_w) {
                                                play_source = Some(src);
                                            }
                                            ui.add_space(14.0);
                                        }
                                    });
                                    ui.add_space(10.0);
                                }
                            });
                            ui.add_space(20.0);
                        }

                        // --- SECTION: VOTRE BIBLIOTHÈQUE / GRILLE DES MÉDIAS ---
                        let all_entries = self.history.entries().to_vec();
                        let search_term = self.advanced_search.trim().to_lowercase();

                        let filtered: Vec<&HistoryEntry> = all_entries
                            .iter()
                            .filter(|e| {
                                if !search_term.is_empty() {
                                    let match_src = e.source.to_lowercase().contains(&search_term);
                                    let match_title = e.title.as_ref().is_some_and(|t| t.to_lowercase().contains(&search_term));
                                    if !match_src && !match_title {
                                        return false;
                                    }
                                }
                                let is_audio = classify(Path::new(&e.source)) == PathKind::Audio;
                                match self.advanced_tab {
                                    AdvancedTab::Home => true,
                                    AdvancedTab::Videos => !is_audio,
                                    AdvancedTab::Music => is_audio,
                                    AdvancedTab::Favorites => self.advanced_favorites.contains(&e.source),
                                }
                            })
                            .collect();

                        ui.horizontal(|ui| {
                            ui.add_space(24.0);
                            let sec_title = match self.advanced_tab {
                                AdvancedTab::Home => "Votre bibliothèque",
                                AdvancedTab::Videos => "Toutes vos vidéos",
                                AdvancedTab::Music => "Votre musique",
                                AdvancedTab::Favorites => "Vos favoris",
                            };
                            ui.label(RichText::new(sec_title).font(FontId::new(18.0, fonts::bold())).color(p.text));
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new(format!("{} média(s)", filtered.len()))
                                    .font(FontId::new(12.0, fonts::medium()))
                                    .color(p.text_dim),
                            );
                        });
                        ui.add_space(12.0);

                        if filtered.is_empty() {
                            ui.horizontal(|ui| {
                                ui.add_space(24.0);
                                let empty_w = (ui.available_width() - 24.0).max(300.0);
                                let (empty_r, _) = ui.allocate_exact_size(vec2(empty_w, 140.0), Sense::hover());
                                ui.painter().rect_filled(empty_r, CornerRadius::same(12), p.raised);
                                ui.painter().rect_stroke(empty_r, CornerRadius::same(12), Stroke::new(1.0, p.border), StrokeKind::Inside);

                                ui.painter().text(
                                    pos2(empty_r.center().x, empty_r.center().y - 20.0),
                                    Align2::CENTER_CENTER,
                                    "Faites place à vos médias.",
                                    FontId::new(17.0, fonts::bold()),
                                    p.text,
                                );
                                ui.painter().text(
                                    pos2(empty_r.center().x, empty_r.center().y + 8.0),
                                    Align2::CENTER_CENTER,
                                    "Glissez des fichiers ici, ou cliquez sur « Ouvrir un fichier » pour commencer.",
                                    FontId::new(12.5, fonts::medium()),
                                    p.text_dim,
                                );
                            });
                        } else {
                            // Responsive grid for library items
                            let avail_w = (ui.available_width() - 48.0).max(300.0);
                            let cols = if avail_w > 1100.0 { 4 } else if avail_w > 800.0 { 3 } else if avail_w > 500.0 { 2 } else { 1 };
                            let card_w = ((avail_w - 14.0 * (cols as f32 - 1.0)) / cols as f32).max(180.0);

                            for chunk in filtered.chunks(cols) {
                                ui.horizontal(|ui| {
                                    ui.add_space(24.0);
                                    for entry in chunk {
                                        let is_fav = self.advanced_favorites.contains(&entry.source);
                                        let (played, fav_click) = render_media_card(ui, &p, entry, card_w, is_fav);
                                        if let Some(src) = played {
                                            play_source = Some(src);
                                        }
                                        if let Some(src) = fav_click {
                                            toggle_fav = Some(src);
                                        }
                                        ui.add_space(14.0);
                                    }
                                });
                                ui.add_space(14.0);
                            }
                        }

                        ui.add_space(40.0);
                    });
            },
        );

        // Actions dispatch
        if let Some(act) = action_open {
            self.run(act);
        }
        if let Some(src) = play_source {
            self.open_inputs(vec![src], false);
        }
        if let Some(src) = toggle_fav {
            if self.advanced_favorites.contains(&src) {
                self.advanced_favorites.remove(&src);
            } else {
                self.advanced_favorites.insert(src);
            }
        }
    }
}

/// Renders a single navigation item in the left sidebar.
fn sidebar_nav_button(ui: &mut Ui, p: &Palette, label: &str, icon: char, active: bool, width: f32) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 39.0), Sense::click());
    let hover = resp.hovered();

    if active {
        ui.painter().rect_filled(rect, CornerRadius::same(8), fade(p.accent, 0.16));
        // Vertical active accent indicator bar on the left edge
        let ind = Rect::from_min_size(pos2(rect.left(), rect.center().y - 9.5), vec2(3.5, 19.0));
        ui.painter().rect_filled(ind, CornerRadius::same(2), p.accent);
    } else if hover {
        ui.painter().rect_filled(rect, CornerRadius::same(8), p.hover);
    }

    let text_col = if active { p.accent } else if hover { p.text } else { p.text_dim };
    ui.painter().text(
        pos2(rect.left() + 14.0, rect.center().y),
        Align2::LEFT_CENTER,
        icon.to_string(),
        FontId::new(16.0, fonts::icons()),
        text_col,
    );
    ui.painter().text(
        pos2(rect.left() + 40.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::new(13.0, if active { fonts::semibold() } else { fonts::medium() }),
        text_col,
    );

    resp.clicked()
}

/// Renders the official 400Player logo emblem (cyan 4 + orange play).
fn paint_emblem_logo(ui: &mut Ui, rect: Rect) {
    let tex = ui.ctx().load_texture(
        "400player-emblem-nobg",
        egui::ColorImage::from_rgba_unmultiplied(
            [128, 128],
            include_bytes!("../../../../assets/logo_nobg_128.rgba"),
        ),
        egui::TextureOptions::LINEAR,
    );
    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    ui.painter().image(tex.id(), rect, uv, Color32::WHITE);
}

/// Renders a card in the "Reprendre le fil" section.
fn render_resume_card(ui: &mut Ui, p: &Palette, entry: &HistoryEntry, width: f32) -> Option<String> {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 68.0), Sense::click());
    let hover = resp.hovered();

    ui.painter().rect_filled(rect, CornerRadius::same(10), if hover { p.hover } else { p.raised });
    ui.painter().rect_stroke(rect, CornerRadius::same(10), Stroke::new(1.0, if hover { p.accent } else { p.border }), StrokeKind::Inside);

    let is_audio = classify(Path::new(&entry.source)) == PathKind::Audio;
    let icon = if is_audio { icons::MUSIC_NOTES } else { icons::FILM_STRIP };

    // Thumbnail tile
    let tile_r = Rect::from_min_size(pos2(rect.left() + 10.0, rect.top() + 10.0), vec2(48.0, 48.0));
    ui.painter().rect_filled(tile_r, CornerRadius::same(8), p.accent_faint);
    ui.painter().text(tile_r.center(), Align2::CENTER_CENTER, icon.to_string(), FontId::new(20.0, fonts::icons()), p.accent);

    let tx = tile_r.right() + 12.0;
    let title = entry.title.as_deref().unwrap_or_else(|| {
        Path::new(&entry.source)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&entry.source)
    });
    let short_title = ellipsize_middle(title, ((width - 80.0) / 8.0) as usize);

    ui.painter().text(
        pos2(tx, rect.top() + 12.0),
        Align2::LEFT_TOP,
        short_title,
        FontId::new(13.0, fonts::semibold()),
        if hover { p.accent } else { p.text },
    );

    let meta = if entry.duration > 0.0 {
        format!(
            "{} / {}  ·  {:.0}% vu",
            format_clock(entry.position),
            format_clock(entry.duration),
            entry.progress() * 100.0
        )
    } else {
        format_clock(entry.position)
    };
    ui.painter().text(
        pos2(tx, rect.top() + 32.0),
        Align2::LEFT_TOP,
        meta,
        FontId::new(11.0, fonts::medium()),
        p.text_dim,
    );

    // Bottom progress bar
    if entry.progress() > 0.01 {
        let p_bg = Rect::from_min_max(pos2(rect.left() + 10.0, rect.bottom() - 4.0), pos2(rect.right() - 10.0, rect.bottom() - 2.0));
        ui.painter().rect_filled(p_bg, CornerRadius::same(1), fade(p.text, 0.08));
        let p_fg = Rect::from_min_max(p_bg.min, pos2(p_bg.left() + p_bg.width() * entry.progress(), p_bg.bottom()));
        ui.painter().rect_filled(p_fg, CornerRadius::same(1), p.accent);
    }

    if resp.clicked() {
        Some(entry.source.clone())
    } else {
        None
    }
}

/// Renders a media card in the library grid.
fn render_media_card(ui: &mut Ui, p: &Palette, entry: &HistoryEntry, width: f32, is_fav: bool) -> (Option<String>, Option<String>) {
    let height = 156.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    let hover = resp.hovered();

    ui.painter().rect_filled(rect, CornerRadius::same(12), if hover { p.hover } else { p.raised });
    ui.painter().rect_stroke(rect, CornerRadius::same(12), Stroke::new(1.0, if hover { p.accent } else { p.border }), StrokeKind::Inside);

    // Top media preview container
    let thumb_h = 96.0;
    let thumb_rect = Rect::from_min_size(rect.left_top(), vec2(width, thumb_h));
    let is_audio = classify(Path::new(&entry.source)) == PathKind::Audio;
    let thumb_bg = if is_audio { Color32::from_rgb(20, 28, 38) } else { Color32::from_rgb(18, 24, 32) };
    ui.painter().rect_filled(thumb_rect, CornerRadius { nw: 12, ne: 12, sw: 0, se: 0 }, thumb_bg);

    // Center icon or play button
    let center_icon = if hover { icons::PLAY } else if is_audio { icons::MUSIC_NOTES } else { icons::FILM_STRIP };
    ui.painter().text(
        thumb_rect.center(),
        Align2::CENTER_CENTER,
        center_icon.to_string(),
        FontId::new(if hover { 26.0 } else { 22.0 }, fonts::icons()),
        if hover { p.accent } else { fade(p.text_dim, 0.8) },
    );

    // Duration badge on bottom right of thumbnail
    if entry.duration > 0.0 {
        let dur_str = format_clock(entry.duration);
        let badge_r = Rect::from_min_size(pos2(thumb_rect.right() - 48.0, thumb_rect.bottom() - 20.0), vec2(42.0, 16.0));
        ui.painter().rect_filled(badge_r, CornerRadius::same(4), Color32::from_black_alpha(180));
        ui.painter().text(badge_r.center(), Align2::CENTER_CENTER, dur_str, FontId::new(9.5, fonts::mono()), Color32::WHITE);
    }

    // Favorite toggle button on top right
    let fav_r = Rect::from_min_size(pos2(thumb_rect.right() - 26.0, thumb_rect.top() + 6.0), vec2(20.0, 20.0));
    let fav_resp = ui.allocate_rect(fav_r, Sense::click());
    let fav_hover = fav_resp.hovered();
    let fav_col = if is_fav { Color32::from_rgb(255, 170, 43) } else if fav_hover { p.text } else { p.text_faint };
    ui.painter().text(fav_r.center(), Align2::CENTER_CENTER, icons::BOOKMARK.to_string(), FontId::new(13.0, fonts::icons()), fav_col);

    // Bottom info (Title and subtitle)
    let title = entry.title.as_deref().unwrap_or_else(|| {
        Path::new(&entry.source)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&entry.source)
    });
    let short_title = ellipsize_middle(title, ((width - 24.0) / 7.6) as usize);

    ui.painter().text(
        pos2(rect.left() + 10.0, thumb_rect.bottom() + 10.0),
        Align2::LEFT_TOP,
        short_title,
        FontId::new(12.5, fonts::semibold()),
        if hover { p.accent } else { p.text },
    );

    let sub_text = if is_audio { "Fichier audio" } else { "Fichier vidéo" };
    ui.painter().text(
        pos2(rect.left() + 10.0, thumb_rect.bottom() + 28.0),
        Align2::LEFT_TOP,
        sub_text,
        FontId::new(10.5, fonts::medium()),
        p.text_dim,
    );

    let mut clicked_play = None;
    let mut clicked_fav = None;

    if fav_resp.clicked() {
        clicked_fav = Some(entry.source.clone());
    } else if resp.clicked() {
        clicked_play = Some(entry.source.clone());
    }

    (clicked_play, clicked_fav)
}

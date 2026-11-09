//! The bar at the top of the window: title, quick buttons and — when the window is borderless —
//! the minimise / maximise / close controls and the drag area.

use egui::text::LayoutJob;
use egui::{Align2, Color32, Context, CornerRadius, FontId, Id, Order, Rect, Sense, Ui, ViewportCommand, pos2, vec2};

use crate::actions::Action;
use crate::app::LumenApp;
use crate::i18n::{Key as T, t};
use crate::theme::{self, Palette, fonts, mix};
use crate::ui::icons;
use crate::widgets::IconButton;

const BAR_HEIGHT: f32 = 56.0;

impl LumenApp {
    pub fn draw_topbar(&mut self, ctx: &Context, stage: Rect, controls: f32) {
        let p = self.pal.clone();
        let has_media = self.source.is_some();
        // Over a picture (or the dark theme's welcome screen) the controls are light on dark glass.
        let on_dark = has_media || p.dark;
        let custom = self.settings.custom_titlebar && !self.fullscreen;
        let bar = Rect::from_min_size(stage.min, vec2(stage.width(), BAR_HEIGHT));

        let mut actions: Vec<Action> = Vec::new();
        let mut window_cmd: Vec<ViewportCommand> = Vec::new();
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));

        egui::Area::new(Id::unique("topbar")).order(Order::Middle).fixed_pos(bar.min).interactable(controls > 0.3).show(ctx, |ui| {
            ui.set_opacity(controls.max(if has_media { 0.0 } else { 1.0 }));

            // A soft shade so the title stays readable over bright pictures. It is painted by the area
            // itself: a bare layer painter would be drawn on top of every area of the same order.
            if has_media {
                let shade = Rect::from_min_size(stage.min, vec2(stage.width(), BAR_HEIGHT + 72.0));
                theme::paint_vgradient(ui.painter(), shade, Color32::from_black_alpha(178), Color32::TRANSPARENT);
            }

            // The empty part of the bar drags the borderless window.
            let drag = ui.allocate_rect(bar, Sense::click_and_drag());
            if custom {
                if drag.drag_started_by(egui::PointerButton::Primary) {
                    window_cmd.push(ViewportCommand::StartDrag);
                }
                if drag.double_clicked() {
                    window_cmd.push(ViewportCommand::Maximized(!maximized));
                }
            }

            // --- right side: buttons ---------------------------------------------------------------
            let mut right = bar.right() - 10.0;
            let button_rect = |right: f32, w: f32| Rect::from_min_size(pos2(right - w, bar.top() + (BAR_HEIGHT - 36.0) * 0.5), vec2(w, 36.0));

            if custom {
                for (icon, kind) in [
                    (icons::WIN_CLOSE, WinButton::Close),
                    (if maximized { icons::WIN_RESTORE } else { icons::WIN_MAX }, WinButton::Maximize),
                    (icons::WIN_MIN, WinButton::Minimize),
                ] {
                    let r = button_rect(right, 44.0);
                    right = r.left();
                    let tip = match kind {
                        WinButton::Close => t(T::Close),
                        WinButton::Maximize => t(if maximized { T::Restore } else { T::Maximize }),
                        WinButton::Minimize => t(T::Minimize),
                    };
                    if window_button(ui, &p, r, icon, kind, tip, on_dark) {
                        window_cmd.push(match kind {
                            WinButton::Close => ViewportCommand::Close,
                            WinButton::Maximize => ViewportCommand::Maximized(!maximized),
                            WinButton::Minimize => ViewportCommand::Minimized(true),
                        });
                    }
                }
                right -= 8.0;
            }

            let mut quick = |ui: &mut Ui, icon: char, tip: &str, shortcut: Option<String>, active: bool, action: Action, right: &mut f32| {
                let r = button_rect(*right, 36.0);
                *right = r.left() - 2.0;
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r));
                let button = IconButton::new(icon).size(36.0).active(active).tip(tip).shortcut(shortcut);
                let button = if on_dark { button.glass() } else { button };
                if button.show(&mut child, &p).clicked() {
                    actions.push(action);
                }
            };

            if self.fullscreen {
                quick(ui, icons::ARROWS_IN, t(T::ExitFullscreen), Action::ToggleFullscreen.shortcut_label(), false, Action::ToggleFullscreen, &mut right);
            }
            quick(ui, icons::GEAR, t(T::Settings), Action::OpenSettings.shortcut_label(), self.windows.settings.is_some(), Action::OpenSettings, &mut right);
            quick(
                ui,
                icons::COMMAND,
                t(T::CommandPalette),
                Some(crate::actions::Binding::Key(egui::Key::K, crate::actions::Mods::CTRL).label()),
                self.windows.palette.is_some(),
                Action::OpenPalette,
                &mut right,
            );
            if has_media {
                quick(ui, icons::INFO, t(T::MediaInfo), Action::ToggleInfo.shortcut_label(), self.windows.info, Action::ToggleInfo, &mut right);
            }

            // --- left side: title -----------------------------------------------------------------------
            if has_media {
                let max_w = (right - bar.left() - 36.0).max(80.0);
                let title = self.display_title();
                let painter = ui.painter();
                let pos = pos2(bar.left() + 20.0, bar.top() + 14.0);
                let shadow = single_line(painter, &title, FontId::new(15.0, fonts::semibold()), Color32::from_black_alpha(170), max_w);
                let galley = single_line(painter, &title, FontId::new(15.0, fonts::semibold()), p.glass_text, max_w);
                painter.galley(pos + vec2(0.0, 1.0), shadow, Color32::PLACEHOLDER);
                painter.galley(pos, galley.clone(), Color32::PLACEHOLDER);

                let sub = self.title_detail();
                if !sub.is_empty() {
                    let y = pos.y + galley.size().y + 2.0;
                    let shadow = single_line(painter, &sub, FontId::new(12.5, fonts::medium()), Color32::from_black_alpha(170), max_w);
                    let g = single_line(painter, &sub, FontId::new(12.5, fonts::medium()), p.glass_dim, max_w);
                    painter.galley(pos2(pos.x, y + 1.0), shadow, Color32::PLACEHOLDER);
                    painter.galley(pos2(pos.x, y), g, Color32::PLACEHOLDER);
                }
            } else if custom {
                // The welcome screen has no title: a small brand mark in the corner of a borderless window.
                let painter = ui.painter();
                let (dim, strong) = if on_dark { (p.glass_dim, p.glass_text) } else { (p.text_dim, p.text) };
                painter.circle_filled(pos2(bar.left() + 24.0, bar.center().y), 5.0, if on_dark { p.accent_glass } else { p.accent });
                painter.text(pos2(bar.left() + 38.0, bar.center().y), Align2::LEFT_CENTER, "400_Player", FontId::new(13.5, fonts::semibold()), mix(dim, strong, 0.6));
            }
        });

        for cmd in window_cmd {
            ctx.send_viewport_cmd(cmd);
        }
        self.dispatch(actions);
    }

    /// Second line under the title: position in the queue, artist.
    fn title_detail(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.playlist.len() > 1
            && let Some(i) = self.playlist.current_index()
        {
            parts.push(format!("{} / {}", i + 1, self.playlist.len()));
        }
        if let Some(m) = &self.media {
            if let Some(artist) = m.artist() {
                parts.push(artist.to_owned());
            }
            if let Some(i) = m.chapter_at(self.snap.position)
                && m.chapters.len() > 1
            {
                let c = &m.chapters[i];
                parts.push(if c.title.trim().is_empty() { crate::i18n::t1(T::ChapterN, i + 1) } else { c.title.clone() });
            }
        }
        parts.join("  ·  ")
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WinButton {
    Minimize,
    Maximize,
    Close,
}

/// A window control of the borderless title bar. `true` when clicked.
fn window_button(ui: &mut Ui, p: &Palette, rect: Rect, icon: char, kind: WinButton, tip: &str, on_dark: bool) -> bool {
    let resp = ui.interact(rect, Id::unique(("win-button", icon as u32)), Sense::click());
    let hover = ui.ctx().animate_bool_with_time(resp.id.with("h"), resp.hovered(), 0.1);
    let pressed = resp.is_pointer_button_down_on();
    let radius = CornerRadius::same(10);
    let inner = rect.shrink2(vec2(2.0, 0.0));
    if hover > 0.0 {
        let base = if kind == WinButton::Close {
            Color32::from_rgb(232, 52, 74)
        } else if on_dark {
            Color32::WHITE
        } else {
            Color32::BLACK
        };
        let a = if kind == WinButton::Close { 255.0 * hover } else { 30.0 * hover + if pressed { 14.0 } else { 0.0 } };
        ui.painter().rect_filled(inner, radius, base.gamma_multiply(a / 255.0));
    }
    let (dim, strong) = if on_dark { (p.glass_dim, p.glass_text) } else { (p.text_dim, p.text) };
    let fg = if kind == WinButton::Close && hover > 0.5 { Color32::WHITE } else { mix(dim, strong, hover) };
    ui.painter().text(inner.center(), Align2::CENTER_CENTER, icon.to_string(), FontId::new(15.0, fonts::icons()), fg);
    resp.on_hover_ui(|ui| crate::widgets::tooltip(ui, p, tip, None)).clicked()
}

/// Lays `text` out on one line, ending in an ellipsis when it does not fit.
fn single_line(painter: &egui::Painter, text: &str, font: FontId, color: Color32, max_width: f32) -> std::sync::Arc<egui::Galley> {
    let mut job = LayoutJob::simple(text.to_owned(), font, color, max_width);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    painter.layout_job(job)
}

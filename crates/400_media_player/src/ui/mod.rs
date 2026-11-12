//! The interface: one `draw` per frame that lays out the stage, the floating controls and the
//! windows, plus the keyboard and mouse handling that is not tied to a widget.

pub mod icons_gen;
pub use icons_gen as icons;

pub mod dialogs;
pub mod dock;
pub mod equalizer;
pub mod info;
pub mod menu;
pub mod osd;
pub mod palette;
pub mod playlist;
pub mod settings;
pub mod shortcuts;
pub mod stage;
pub mod topbar;
pub mod welcome;
pub mod advanced;

use egui::{Align2, Color32, Context, CursorIcon, Event, Id, Key, Order, Pos2, Rect, RichText, Sense, Ui, Vec2, pos2, vec2};

use crate::actions::{self, Action};
use crate::app::MediaPlayer400App;
use crate::i18n::{Key as T, t};
use crate::state::{MenuState, Page};
use crate::theme::{Palette, fade, fonts, radius};
use crate::widgets::{IconButton, icon_text};

/// Width of the playlist drawer.
pub const DRAWER_WIDTH: f32 = 360.0;

impl MediaPlayer400App {
    /// One frame of interface.
    pub fn draw(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let full = ui.max_rect();
        let now = ctx.input(|i| i.time);

        self.handle_input(&ctx, now);

        // The stage is the window minus the playlist drawer, which pushes the picture aside.
        let drawer_t = ctx.animate_bool_with_time(Id::unique("drawer-open"), self.windows.playlist, 0.2);
        let drawer_w = DRAWER_WIDTH.min(full.width() * 0.62) * drawer_t * (2.0 - drawer_t);
        let stage = Rect::from_min_max(full.min, pos2(full.max.x - drawer_w, full.max.y));
        self.stage_rect = stage;

        // Behind a picture it is always dark; the welcome screen follows the theme.
        let backdrop = if self.source.is_none() && !self.pal.dark { self.pal.panel } else { self.pal.video_bg };
        ui.painter().rect_filled(full, 0.0, backdrop);

        let controls = self.controls_alpha(&ctx, now);
        if self.source.is_none() {
            self.draw_welcome(ui, stage, now);
        } else {
            self.draw_stage(ui, stage, controls, now);
        }

        self.draw_osd(&ctx, stage, now);
        self.draw_stats(&ctx, stage);
        self.draw_toasts(&ctx, stage, now);
        let show_topbar = if self.source.is_some() {
            controls > 0.01
        } else {
            !self.settings.advanced_mode
        };
        if show_topbar {
            self.draw_topbar(&ctx, stage, controls);
        }
        if self.source.is_some() && controls > 0.01 && !matches!(self.snap.state, media_player_400_engine::PlayState::Failed) {
            self.draw_dock(&ctx, stage, controls);
        } else {
            self.dock_rect = None;
        }
        self.draw_drawer(&ctx, full, drawer_w);
        self.draw_windows(&ctx);
        self.draw_menu(&ctx, now);
        self.draw_palette(&ctx);
        self.draw_dialogs(&ctx);
        self.draw_drop_overlay(&ctx, full);
        self.draw_resize_handles(&ctx, full);

        // A click that only closed a menu stays ignored until the button is released.
        if !ctx.input(|i| i.pointer.any_down()) {
            self.swallow_click = false;
        }
    }

    /// How visible the floating controls are, `0.0 ..= 1.0` (animated).
    fn controls_alpha(&mut self, ctx: &Context, now: f64) -> f32 {
        let want = self.controls_wanted(ctx, now);
        let t = ctx.animate_bool_with_time(Id::unique("controls-visible"), want, if want { 0.12 } else { 0.35 });
        t * t * (3.0 - 2.0 * t)
    }

    fn controls_wanted(&self, ctx: &Context, now: f64) -> bool {
        if self.source.is_none() {
            return true;
        }
        let audio_only = self.media.as_ref().is_some_and(|m| !m.has_video());
        if audio_only || self.snap.state != media_player_400_engine::PlayState::Playing {
            return true;
        }
        if self.windows.any_window() || self.windows.playlist || self.menu.is_some() || self.seek_drag.is_some() {
            return true;
        }
        if now - self.activity_at < f64::from(self.settings.controls_timeout) {
            return true;
        }
        // Resting on the dock keeps it.
        match (self.dock_rect, ctx.pointer_latest_pos()) {
            (Some(r), Some(p)) => r.expand(10.0).contains(p),
            _ => false,
        }
    }

    // =============================================================================================
    // Input
    // =============================================================================================

    fn handle_input(&mut self, ctx: &Context, now: f64) {
        let (events, moved, hovered, dropped, modifiers) = ctx
            .input(|i| (i.events.clone(), i.pointer.delta() != Vec2::ZERO || i.pointer.any_pressed(), !i.raw.hovered_files.is_empty(), i.raw.dropped_files.clone(), i.modifiers));
        self.drop_hover = hovered;

        // Mouse wheel: `(notches, ctrl held)`; a notch is one line of a wheel or ~40 points of a trackpad.
        let wheel: Vec<(f32, bool)> = events
            .iter()
            .filter_map(|e| match e {
                Event::MouseWheel { unit, delta, modifiers, .. } => {
                    let notches = match unit {
                        egui::MouseWheelUnit::Line => delta.y,
                        egui::MouseWheelUnit::Page => delta.y * 3.0,
                        egui::MouseWheelUnit::Point => delta.y / 40.0,
                    };
                    Some((notches, modifiers.command))
                }
                _ => None,
            })
            .collect();

        let typing = ctx.egui_wants_keyboard_input();
        let key_activity = events.iter().any(|e| matches!(e, Event::Key { pressed: true, .. }));
        if moved || key_activity || !wheel.is_empty() {
            self.wake_controls();
        }

        // Escape closes the topmost layer, then returns to the list if playing, or leaves full screen.
        let escape = events.iter().any(|e| matches!(e, Event::Key { key: Key::Escape, pressed: true, repeat: false, .. }));
        if escape && !self.dismiss_top_layer() {
            if self.source.is_some() {
                if self.fullscreen {
                    self.set_fullscreen(false);
                }
                self.stop();
            } else if self.fullscreen {
                self.set_fullscreen(false);
            }
        }

        // The palette shortcut works even while typing in the palette itself (it toggles it).
        let palette_toggle = events
            .iter()
            .any(|e| actions::Binding::Key(Key::K, actions::Mods::CTRL).matches(e, false) || actions::Binding::Key(Key::P, actions::Mods::CTRL_SHIFT).matches(e, false));
        if palette_toggle {
            self.run(Action::OpenPalette);
        } else if !typing && !self.windows.any_modal() {
            for a in actions::actions_from_events(&events) {
                self.run(a);
            }
            for e in &events {
                if let Some(n) = actions::percent_key(e) {
                    self.seek_percent(n);
                }
            }
        }

        // Mouse wheel over the picture: volume, or zoom with Ctrl.
        if !wheel.is_empty() && !ctx.is_pointer_over_egui() && self.source.is_some() && !self.windows.any_modal() {
            for (notches, ctrl) in wheel {
                if ctrl {
                    self.zoom_by(1.1f32.powf(notches));
                } else {
                    self.scroll_acc += notches;
                    while self.scroll_acc.abs() >= 1.0 {
                        let dir = self.scroll_acc.signum();
                        self.scroll_acc -= dir;
                        self.change_volume(dir);
                    }
                }
            }
        }

        // Files dropped on the window: replace the queue, or add to it with Shift held.
        if !dropped.is_empty() {
            let inputs: Vec<String> = dropped.iter().map(|f| f.path().to_string_lossy().into_owned()).filter(|p| !p.is_empty()).collect();
            if !inputs.is_empty() {
                if inputs.len() == 1 && std::path::Path::new(&inputs[0]).is_dir() {
                    self.open_folder_path(std::path::Path::new(&inputs[0]), modifiers.shift);
                } else {
                    self.open_inputs(inputs, modifiers.shift);
                }
            }
        }
        let _ = now;
    }

    /// Opens the drill-down menu at `pos`.
    pub fn open_menu(&mut self, pos: Pos2, above: bool, page: Page) {
        let now = self.now();
        let stack = if page == Page::Root { vec![Page::Root] } else { vec![Page::Root, page] };
        self.menu = Some(MenuState { pos, above, stack, opened_at: now });
    }

    // =============================================================================================
    // Layers
    // =============================================================================================

    fn draw_drop_overlay(&mut self, ctx: &Context, full: Rect) {
        let shown = ctx.animate_bool_with_time(Id::unique("drop-overlay"), self.drop_hover, 0.15);
        if shown <= 0.01 {
            return;
        }
        let p = self.pal.clone();
        let painter = ctx.layer_painter(egui::LayerId::new(Order::Tooltip, Id::unique("drop-overlay")));
        painter.rect_filled(full, 0.0, fade(Color32::from_black_alpha(170), shown));
        let inner = full.shrink(18.0);
        painter.rect_stroke(inner, egui::CornerRadius::same(radius::WINDOW), egui::Stroke::new(2.0, fade(p.accent_glass, shown)), egui::StrokeKind::Inside);
        painter.rect_filled(inner, egui::CornerRadius::same(radius::WINDOW), fade(p.accent_glass.gamma_multiply(0.10), shown));
        let c = full.center();
        painter.text(c - vec2(0.0, 26.0), Align2::CENTER_CENTER, icons::DOWNLOAD.to_string(), egui::FontId::new(54.0, fonts::icons()), fade(p.accent_glass, shown));
        painter.text(c + vec2(0.0, 30.0), Align2::CENTER_CENTER, t_drop(), egui::FontId::new(22.0, fonts::semibold()), fade(Color32::WHITE, shown));
        painter.text(c + vec2(0.0, 62.0), Align2::CENTER_CENTER, t(T::DropAddHint), egui::FontId::new(13.5, fonts::medium()), fade(Color32::from_white_alpha(170), shown));
    }

    /// Resize grips for the borderless window (custom title bar).
    fn draw_resize_handles(&mut self, ctx: &Context, full: Rect) {
        if !self.settings.custom_titlebar || self.fullscreen {
            return;
        }
        use egui::ResizeDirection as D;
        let m = 6.0;
        let c = 14.0;
        let zones: [(Rect, D, CursorIcon); 8] = [
            (Rect::from_min_max(full.left_top() + vec2(c, 0.0), pos2(full.right() - c, full.top() + m)), D::North, CursorIcon::ResizeNorth),
            (Rect::from_min_max(pos2(full.left() + c, full.bottom() - m), full.right_bottom() - vec2(c, 0.0)), D::South, CursorIcon::ResizeSouth),
            (Rect::from_min_max(full.left_top() + vec2(0.0, c), pos2(full.left() + m, full.bottom() - c)), D::West, CursorIcon::ResizeWest),
            (Rect::from_min_max(pos2(full.right() - m, full.top() + c), full.right_bottom() - vec2(0.0, c)), D::East, CursorIcon::ResizeEast),
            (Rect::from_min_size(full.left_top(), vec2(c, c)), D::NorthWest, CursorIcon::ResizeNorthWest),
            (Rect::from_min_size(pos2(full.right() - c, full.top()), vec2(c, c)), D::NorthEast, CursorIcon::ResizeNorthEast),
            (Rect::from_min_size(pos2(full.left(), full.bottom() - c), vec2(c, c)), D::SouthWest, CursorIcon::ResizeSouthWest),
            (Rect::from_min_size(full.right_bottom() - vec2(c, c), vec2(c, c)), D::SouthEast, CursorIcon::ResizeSouthEast),
        ];
        egui::Area::new(Id::unique("resize-handles")).order(Order::Foreground).fixed_pos(full.min).interactable(true).show(ctx, |ui| {
            for (i, (rect, dir, cursor)) in zones.into_iter().enumerate() {
                let r = ui.interact(rect, Id::unique(("resize", i)), Sense::drag());
                if r.hovered() || r.dragged() {
                    ctx.set_cursor_icon(cursor);
                }
                if r.drag_started() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(dir));
                }
            }
        });
    }

    /// The auxiliary windows that are open.
    fn draw_windows(&mut self, ctx: &Context) {
        if self.windows.settings.is_some() {
            self.draw_settings(ctx);
        }
        if self.windows.equalizer {
            self.draw_equalizer(ctx);
        }
        if self.windows.info {
            self.draw_info(ctx);
        }
        if self.windows.shortcuts {
            self.draw_shortcuts(ctx);
        }
    }
}

fn t_drop() -> &'static str {
    t(T::DropToPlay)
}

// =================================================================================================
// Modal window helper
// =================================================================================================

/// A centred window over a dimmed backdrop, with a title row and a close button.
pub struct Modal<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub icon: char,
    pub width: f32,
    /// Dim the interface behind the window and close on a click outside.
    pub scrim: bool,
    /// Height kept free at the bottom of the screen (the control dock) when centring the window.
    pub reserve_bottom: f32,
}

impl<'a> Modal<'a> {
    pub fn new(id: &'a str, title: &'a str, icon: char, width: f32) -> Self {
        Self { id, title, icon, width, scrim: true, reserve_bottom: 0.0 }
    }

    pub fn no_scrim(mut self) -> Self {
        self.scrim = false;
        self
    }

    /// Centres the window above the bottom `height` pixels, so it does not cover the dock.
    pub fn keep_bottom_free(mut self, height: f32) -> Self {
        self.reserve_bottom = height;
        self
    }

    /// Shows the window; returns `true` when the user asked to close it.
    pub fn show(self, ctx: &Context, p: &Palette, body: impl FnOnce(&mut Ui)) -> bool {
        let screen = ctx.content_rect();
        let mut close = false;
        let appear = ctx.animate_bool_with_time(Id::unique((self.id, "appear")), true, 0.14);
        if self.scrim {
            egui::Area::new(Id::unique((self.id, "scrim"))).order(Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
                let r = ui.allocate_response(screen.size(), Sense::click());
                ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha((150.0 * appear) as u8));
                if r.clicked() {
                    close = true;
                }
            });
        }
        let max_w = (screen.width() - 32.0).max(280.0);
        let width = self.width.min(max_w);
        let free = Rect::from_min_max(screen.min, pos2(screen.right(), (screen.bottom() - self.reserve_bottom).max(screen.top() + 240.0)));
        let pivot_pos = free.center() + vec2(0.0, (1.0 - appear) * 10.0);
        egui::Area::new(Id::unique((self.id, "window"))).order(Order::Foreground).pivot(Align2::CENTER_CENTER).fixed_pos(pivot_pos).constrain(true).show(ctx, |ui| {
            ui.set_opacity(appear);
            p.window_frame().show(ui, |ui| {
                ui.set_width(width - 36.0);
                ui.horizontal(|ui| {
                    ui.label(icon_text(self.icon, 22.0, p.accent));
                    ui.add_space(4.0);
                    ui.label(RichText::new(self.title).font(egui::FontId::new(19.0, fonts::semibold())).color(p.text));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if IconButton::new(icons::X).size(32.0).tip(t(T::Close)).show(ui, p).clicked() {
                            close = true;
                        }
                    });
                });
                ui.add_space(8.0);
                body(ui);
            });
        });
        close
    }
}

/// A vertical scroll area that grows up to `max_height`, whatever size its window had last frame.
///
/// egui sizes an area from its previous frame, so a scroll area that is the last thing in a window
/// would stay as small as the very first layout pass left it. Asking for the room explicitly (the
/// space not used is given back) avoids that.
pub fn scroll_body<R>(ui: &mut Ui, max_height: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let width = ui.available_width();
    ui.allocate_ui_with_layout(vec2(width, max_height), egui::Layout::top_down(egui::Align::Min), |ui| {
        egui::ScrollArea::vertical().max_height(max_height).auto_shrink([false, true]).show(ui, add).inner
    })
    .inner
}

//! The keyboard-shortcut cheat sheet, generated from the same table that drives the shortcuts.

use egui::{Align, Context, FontId, Layout, RichText, Sense, Ui, vec2};

use crate::actions::{Action, DEFS, Group};
use crate::app::LumenApp;
use crate::i18n::{Key as T, t};
use crate::theme::{Palette, fade, fonts};
use crate::ui::{Modal, icons};
use crate::widgets::{kbd, section_title};

/// Mouse gestures: (gesture, what it does).
const GESTURES: [(T, T); 7] = [
    (T::GestureClick, T::GestureClickDoes),
    (T::GestureDoubleClick, T::GestureDoubleClickDoes),
    (T::GestureWheel, T::GestureWheelDoes),
    (T::GestureCtrlWheel, T::GestureCtrlWheelDoes),
    (T::GestureRightClick, T::GestureRightClickDoes),
    (T::GestureDrop, T::GestureDropDoes),
    (T::GestureShiftDrop, T::GestureShiftDropDoes),
];

impl LumenApp {
    pub fn draw_shortcuts(&mut self, ctx: &Context) {
        let p = self.pal.clone();
        let settings = self.settings.clone();
        let screen_h = ctx.content_rect().height();
        let wide = ctx.content_rect().width() >= 860.0;
        let close = Modal::new("shortcuts", t(T::Shortcuts), icons::KEYBOARD, 860.0).show(ctx, &p, |ui| {
            ui.label(RichText::new(t(T::ShortcutsHint)).font(FontId::new(13.0, fonts::medium())).color(p.text_dim));
            ui.add_space(6.0);
            crate::ui::scroll_body(ui, (screen_h - 200.0).clamp(220.0, 640.0), |ui| {
                // Keeps the key badges clear of the floating scroll bar.
                egui::Frame::NONE.inner_margin(egui::Margin { right: 14, ..Default::default() }).show(ui, |ui| {
                    let groups = Group::ALL;
                    if wide {
                        ui.columns(2, |cols| {
                            for (i, g) in groups.iter().enumerate() {
                                // Left: playback, navigation, audio. Right: the rest and the mouse.
                                let col = if i < 3 { 0 } else { 1 };
                                group(&mut cols[col], &p, &settings, *g);
                            }
                            gestures(&mut cols[1], &p);
                        });
                    } else {
                        for g in groups {
                            group(ui, &p, &settings, g);
                        }
                        gestures(ui, &p);
                    }
                });
            });
        });
        if close {
            self.windows.shortcuts = false;
        }
    }
}

fn group(ui: &mut Ui, p: &Palette, settings: &lumen400_core::settings::Settings, g: Group) {
    section_title(ui, p, g.label());
    for def in DEFS.iter().filter(|d| d.group == g && d.in_palette) {
        row(ui, p, &def.action.label(settings), &def.labels());
    }
    // The palette shortcut is not listed as a palette entry itself.
    if g == Group::Window {
        row(ui, p, t(T::CommandPalette), &Action::OpenPalette.def().labels());
    }
    ui.add_space(6.0);
}

fn gestures(ui: &mut Ui, p: &Palette) {
    section_title(ui, p, t(T::Mouse));
    for (gesture, does) in GESTURES {
        row(ui, p, t(does), &[t(gesture).to_owned()]);
    }
}

/// `label ........ [key] or [key]`
fn row(ui: &mut Ui, p: &Palette, label: &str, keys: &[String]) {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing = vec2(5.0, 0.0);
        ui.spacing_mut().interact_size.y = 26.0;
        ui.horizontal(|ui| {
            ui.set_min_height(30.0);
            ui.label(RichText::new(label).font(FontId::new(13.5, fonts::medium())).color(p.text));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                for (i, k) in keys.iter().enumerate().rev() {
                    kbd(ui, p, k);
                    if i > 0 {
                        ui.label(RichText::new(t(T::Or)).font(FontId::new(11.5, fonts::medium())).color(p.text_faint));
                    }
                }
            });
        });
        let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
        ui.painter().rect_filled(line, 0.0, fade(p.border, 0.7));
    });
}

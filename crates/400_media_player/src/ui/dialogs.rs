//! Small modal questions: "open a network address" and "jump to a time".

use egui::{Align, Context, FontId, Key, Layout, RichText, TextEdit, Ui};

use crate::app::MediaPlayer400App;
use crate::i18n::{Key as T, t};
use crate::state::Dialog;
use crate::theme::fonts;
use crate::ui::{Modal, icons};
use crate::widgets::button;

impl MediaPlayer400App {
    pub fn draw_dialogs(&mut self, ctx: &Context) {
        let Some(dialog) = self.windows.dialog.take() else { return };
        let p = self.pal.clone();
        match dialog {
            Dialog::OpenUrl { mut text, append, mut first_frame } => {
                let mut submit = false;
                let mut cancelled = false;
                let close = Modal::new("dialog-url", t(T::OpenUrlTitle), icons::LINK, 540.0).show(ctx, &p, |ui| {
                    ui.label(RichText::new(t(T::OpenUrlHint)).font(FontId::new(13.0, fonts::medium())).color(p.text_dim));
                    ui.add_space(8.0);
                    let resp = text_field(ui, &mut text, t(T::OpenUrlHint), &mut first_frame);
                    ui.add_space(14.0);
                    let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    submit = enter && !text.trim().is_empty();
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        ui.add_enabled_ui(!text.trim().is_empty(), |ui| {
                            if button(ui, &p, None, t(T::OpenUrlConfirm), true).clicked() {
                                submit = true;
                            }
                        });
                        if button(ui, &p, None, t(T::Cancel), false).clicked() {
                            cancelled = true;
                        }
                    });
                });
                if submit && !text.trim().is_empty() {
                    self.open_inputs(vec![text.trim().to_owned()], append);
                } else if !close && !cancelled {
                    self.windows.dialog = Some(Dialog::OpenUrl { text, append, first_frame });
                }
            }
            Dialog::JumpToTime { mut text, mut error, mut first_frame } => {
                let mut submit = false;
                let mut cancelled = false;
                let close = Modal::new("dialog-jump", t(T::JumpToTime), icons::CLOCK, 420.0).show(ctx, &p, |ui| {
                    ui.label(RichText::new(t(T::JumpToTimeHint)).font(FontId::new(13.0, fonts::medium())).color(p.text_dim));
                    ui.add_space(8.0);
                    let resp = text_field(ui, &mut text, "1:23:45", &mut first_frame);
                    if resp.changed() {
                        error = false;
                    }
                    if error {
                        ui.add_space(6.0);
                        ui.label(RichText::new(t(T::JumpInvalid)).font(FontId::new(12.5, fonts::medium())).color(p.danger));
                    }
                    ui.add_space(14.0);
                    let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    submit = enter;
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        if button(ui, &p, None, t(T::Go), true).clicked() {
                            submit = true;
                        }
                        if button(ui, &p, None, t(T::Cancel), false).clicked() {
                            cancelled = true;
                        }
                    });
                });
                if submit && !text.trim().is_empty() {
                    if self.jump_to_text(&text) {
                        return;
                    }
                    error = true;
                    first_frame = true;
                }
                if !close && !cancelled {
                    self.windows.dialog = Some(Dialog::JumpToTime { text, error, first_frame });
                }
            }
        }
    }
}

/// A single-line text field that takes the keyboard focus the first time it is shown.
fn text_field(ui: &mut Ui, text: &mut String, hint: &str, first_frame: &mut bool) -> egui::Response {
    let resp = ui.add(
        TextEdit::singleline(text)
            .hint_text(RichText::new(hint).color(ui.visuals().weak_text_color()))
            .font(FontId::new(16.0, fonts::medium()))
            .desired_width(f32::INFINITY)
            .margin(egui::Margin::symmetric(12, 10)),
    );
    if *first_frame {
        resp.request_focus();
        *first_frame = false;
    }
    resp
}

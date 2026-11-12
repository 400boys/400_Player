//! The 10-band graphic equaliser window.

use egui::{Align, Align2, Color32, Context, CornerRadius, FontId, Layout, Pos2, RichText, Sense, Stroke, Ui, pos2, vec2};
use media_player_400_core::dsp::eq::{EQ_BANDS, EQ_FREQS, EQ_MAX_DB, EqSettings, PRESETS, eq_response_db};
use media_player_400_core::settings::Language;

use crate::app::MediaPlayer400App;
use crate::i18n::{self, Key as T, t};
use crate::theme::{Palette, fade, fonts, mix};
use crate::ui::{Modal, icons};
use crate::widgets::{ThinSlider, button, toggle_switch};

const CUSTOM: &str = "Custom";

/// The name of a preset in the interface language.
pub fn preset_display(name: &str) -> String {
    if i18n::current() != Language::French {
        return if name == CUSTOM { t(T::EqCustom).to_owned() } else { name.to_owned() };
    }
    match name {
        "Flat" => "Plat",
        "Bass Boost" => "Basses renforcées",
        "Treble Boost" => "Aigus renforcés",
        "Vocal" => "Voix",
        "Loudness" => "Loudness",
        "Classical" => "Classique",
        "Full Bass" => "Basses maximales",
        "Full Treble" => "Aigus maximaux",
        "Headphones" => "Casque",
        "Live" => "Concert",
        "Party" => "Fête",
        "Soft" => "Doux",
        CUSTOM => return t(T::EqCustom).to_owned(),
        other => other,
    }
    .to_owned()
}

/// `31.25 → "31"`, `1000 → "1k"`, `16000 → "16k"`.
pub fn freq_label(hz: f32) -> String {
    if hz >= 1000.0 {
        let k = hz / 1000.0;
        if (k - k.round()).abs() < 0.01 { format!("{}k", k.round() as i32) } else { format!("{k:.1}k") }
    } else {
        format!("{}", hz.round() as i32)
    }
}

impl MediaPlayer400App {
    pub fn draw_equalizer(&mut self, ctx: &Context) {
        let p = self.pal.clone();
        let mut eq = self.settings.eq.clone();
        let close = Modal::new("equalizer", t(T::Equalizer), icons::EQUALIZER, 760.0).no_scrim().keep_bottom_free(150.0).show(ctx, &p, |ui| {
            eq_body(ui, &p, &mut eq);
        });
        if eq != self.settings.eq {
            self.settings.eq = eq;
            self.player.set_equalizer(&self.settings.eq);
            self.touch_settings();
        }
        if close {
            self.windows.equalizer = false;
        }
    }
}

fn eq_body(ui: &mut Ui, p: &Palette, eq: &mut EqSettings) {
    // --- header: switch, preset, reset ---------------------------------------------------------------------
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        toggle_switch(ui, p, &mut eq.enabled);
        ui.label(RichText::new(t(T::EqEnable)).font(FontId::new(14.5, fonts::medium())).color(p.text));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if button(ui, p, Some(icons::ARROW_CCW), t(T::Reset), false).clicked() {
                *eq = EqSettings { enabled: eq.enabled, ..EqSettings::default() };
            }
            egui::ComboBox::from_id_salt("eq-preset").selected_text(preset_display(&eq.preset)).width(190.0).show_ui(ui, |ui| {
                for pr in PRESETS {
                    let selected = eq.preset.eq_ignore_ascii_case(pr.name);
                    if ui.selectable_label(selected, preset_display(pr.name)).clicked()
                        && let Some(s) = EqSettings::from_preset(pr.name)
                    {
                        *eq = s;
                    }
                }
            });
        });
    });
    ui.add_space(12.0);

    // --- response curve ---------------------------------------------------------------------------------------
    response_graph(ui, p, eq);
    ui.add_space(10.0);

    // --- sliders -------------------------------------------------------------------------------------------------
    let dim = if eq.enabled { 1.0 } else { 0.62 };
    ui.scope(|ui| {
        ui.set_opacity(dim);
        let slider_len = 168.0;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let total = ui.available_width();
            let preamp_w = 70.0;
            let band_w = ((total - preamp_w - 18.0) / EQ_BANDS as f32).floor();

            // Preamp.
            ui.allocate_ui(vec2(preamp_w, slider_len + 60.0), |ui| {
                ui.vertical_centered(|ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    gain_label(ui, p, eq.preamp_db);
                    let mut v = eq.preamp_db.clamp(-12.0, 12.0);
                    let r = ThinSlider::new(&mut v, -12.0..=12.0).vertical().length(slider_len).origin(0.0).detent(0.0).step(0.5).marks(&[0.0]).show(ui, p);
                    if r.changed() {
                        eq.preamp_db = v;
                    }
                    ui.label(RichText::new(t(T::EqPreamp)).font(FontId::new(11.5, fonts::medium())).color(p.text_faint));
                });
            });
            // Divider.
            let (div, _) = ui.allocate_exact_size(vec2(18.0, slider_len + 50.0), Sense::hover());
            ui.painter().line_segment([pos2(div.center().x, div.top() + 6.0), pos2(div.center().x, div.bottom() - 6.0)], Stroke::new(1.0, p.border));

            for (i, f) in EQ_FREQS.iter().enumerate() {
                ui.allocate_ui(vec2(band_w, slider_len + 60.0), |ui| {
                    ui.vertical_centered(|ui| {
                        ui.spacing_mut().item_spacing.y = 4.0;
                        gain_label(ui, p, eq.gains_db[i]);
                        let mut v = eq.gains_db[i];
                        let r = ThinSlider::new(&mut v, -EQ_MAX_DB..=EQ_MAX_DB).vertical().length(slider_len).origin(0.0).detent(0.0).step(0.5).marks(&[0.0]).show(ui, p);
                        if r.changed() {
                            eq.gains_db[i] = v;
                            eq.preset = CUSTOM.to_owned();
                            if !eq.enabled && !eq.is_flat() {
                                eq.enabled = true;
                            }
                        }
                        ui.label(RichText::new(freq_label(*f)).font(FontId::new(11.5, fonts::mono())).color(p.text_faint));
                    });
                });
            }
        });
    });

    // --- footer ----------------------------------------------------------------------------------------------------------
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(t(T::EqHint)).font(FontId::new(12.0, fonts::medium())).color(p.text_faint));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if button(ui, p, Some(icons::MAGIC_WAND), t(T::EqAutoPreamp), false).clicked() {
                eq.preamp_db = eq.auto_preamp();
            }
        });
    });
}

fn gain_label(ui: &mut Ui, p: &Palette, db: f32) {
    let text = if db.abs() < 0.05 { "0".to_owned() } else { format!("{db:+.1}") };
    let color = if db.abs() < 0.05 { p.text_faint } else { p.accent };
    ui.label(RichText::new(text).font(FontId::new(11.5, fonts::mono())).color(color));
}

/// The combined frequency response of the bands, 20 Hz – 20 kHz on a log axis.
fn response_graph(ui: &mut Ui, p: &Palette, eq: &EqSettings) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 110.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::same(12), p.sunken);
    painter.rect_stroke(rect, CornerRadius::same(12), Stroke::new(1.0, p.border), egui::StrokeKind::Inside);
    let plot = rect.shrink2(vec2(34.0, 12.0));
    let range_db = 15.0_f64;
    let y_of = |db: f64| plot.center().y - (db / range_db) as f32 * plot.height() * 0.5;

    // Grid: 0 dB, ±6, ±12 and the octave bands.
    for db in [-12.0, -6.0, 0.0, 6.0, 12.0] {
        let y = y_of(db);
        let strong = db == 0.0;
        painter.line_segment([pos2(plot.left(), y), pos2(plot.right(), y)], Stroke::new(1.0, if strong { p.border_strong } else { fade(p.border, 0.7) }));
        painter.text(pos2(rect.left() + 8.0, y), Align2::LEFT_CENTER, format!("{db:+}").replace("+0", "0"), FontId::new(10.0, fonts::mono()), p.text_faint);
    }
    let x_of = |hz: f64| plot.left() + ((hz / 20.0).ln() / (20000.0_f64 / 20.0).ln()) as f32 * plot.width();
    for f in EQ_FREQS {
        let x = x_of(f64::from(f));
        painter.line_segment([pos2(x, plot.top()), pos2(x, plot.bottom())], Stroke::new(1.0, fade(p.border, 0.45)));
    }

    // Curve (preamp shifts it as a whole).
    let n = 180;
    let active = eq.enabled;
    let mut pts: Vec<Pos2> = Vec::with_capacity(n + 1);
    for i in 0..=n {
        let hz = 20.0 * (20000.0_f64 / 20.0).powf(i as f64 / n as f64);
        let db = (eq_response_db(eq, 48000.0, hz) + f64::from(eq.preamp_db)).clamp(-range_db, range_db);
        pts.push(pos2(plot.left() + plot.width() * i as f32 / n as f32, y_of(db)));
    }
    let color = if active { p.accent } else { p.text_faint };
    // Fill between the curve and the 0 dB line.
    let zero = y_of(0.0);
    let mut mesh = egui::Mesh::default();
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let base = mesh.vertices.len() as u32;
        let top = fade(color, if active { 0.34 } else { 0.12 });
        let flat = fade(color, 0.0);
        for (pt, c) in [(a, top), (b, top), (pos2(b.x, zero), flat), (pos2(a.x, zero), flat)] {
            mesh.vertices.push(egui::epaint::Vertex { pos: pt, uv: egui::epaint::WHITE_UV, color: c });
        }
        mesh.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    painter.add(egui::Shape::mesh(mesh));
    painter.add(egui::Shape::line(pts, Stroke::new(2.2, color)));

    // Band markers.
    for (i, f) in EQ_FREQS.iter().enumerate() {
        let x = x_of(f64::from(*f));
        let db = (eq_response_db(eq, 48000.0, f64::from(*f)) + f64::from(eq.preamp_db)).clamp(-range_db, range_db);
        if eq.gains_db[i].abs() >= 0.05 {
            painter.circle_filled(pos2(x, y_of(db)), 3.5, mix(color, Color32::WHITE, 0.35));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frequency_labels() {
        assert_eq!(freq_label(31.25), "31");
        assert_eq!(freq_label(62.5), "63");
        assert_eq!(freq_label(125.0), "125");
        assert_eq!(freq_label(1000.0), "1k");
        assert_eq!(freq_label(16000.0), "16k");
    }

    #[test]
    fn preset_names_follow_the_language() {
        let _guard = i18n::test_lock();
        i18n::set_language(Language::French);
        assert_eq!(preset_display("Flat"), "Plat");
        assert_eq!(preset_display("Rock"), "Rock");
        assert_eq!(preset_display(CUSTOM), t(T::EqCustom));
        i18n::set_language(Language::English);
        assert_eq!(preset_display("Flat"), "Flat");
    }

    #[test]
    fn every_preset_has_a_display_name() {
        let _guard = i18n::test_lock();
        for l in [Language::French, Language::English] {
            i18n::set_language(l);
            for pr in PRESETS {
                assert!(!preset_display(pr.name).is_empty());
            }
        }
    }
}

//! Reusable widgets in the Lumen400 style: icon buttons, the big play button, switches, segmented
//! controls, chips, key badges, menu rows, thin sliders and a spinner.

use std::ops::RangeInclusive;

use egui::{Align, Align2, Color32, CornerRadius, FontId, Layout, Rect, Response, RichText, Sense, Shape, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};

use crate::theme::{self, Palette, fade, fonts, mix, radius};

/// A glyph of the icon font as rich text.
pub fn icon_text(glyph: char, size: f32, color: Color32) -> RichText {
    RichText::new(glyph.to_string()).family(fonts::icons()).size(size).color(color)
}

fn icon_font(size: f32, fill: bool) -> FontId {
    FontId::new(size, if fill { fonts::icons_fill() } else { fonts::icons() })
}

/// Tooltip with an optional shortcut badge.
pub fn tooltip(ui: &mut Ui, p: &Palette, text: &str, shortcut: Option<&str>) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        ui.label(RichText::new(text).color(p.text));
        if let Some(s) = shortcut {
            kbd(ui, p, s);
        }
    });
}

// -------------------------------------------------------------------------------------------------
// Icon button
// -------------------------------------------------------------------------------------------------

/// A flat, round or rounded-square button showing one icon.
pub struct IconButton<'a> {
    glyph: char,
    size: f32,
    glyph_size: f32,
    tip: Option<&'a str>,
    shortcut: Option<String>,
    active: bool,
    enabled: bool,
    glass: bool,
    fill: bool,
    bold: bool,
}

impl<'a> IconButton<'a> {
    pub fn new(glyph: char) -> Self {
        Self { glyph, size: 36.0, glyph_size: 20.0, tip: None, shortcut: None, active: false, enabled: true, glass: false, fill: false, bold: false }
    }
    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self.glyph_size = (size * 0.56).round();
        self
    }
    pub fn tip(mut self, text: &'a str) -> Self {
        self.tip = Some(text);
        self
    }
    pub fn shortcut(mut self, s: Option<String>) -> Self {
        self.shortcut = s;
        self
    }
    /// Highlighted with the accent (a toggle that is on).
    pub fn active(mut self, a: bool) -> Self {
        self.active = a;
        self
    }
    pub fn enabled(mut self, e: bool) -> Self {
        self.enabled = e;
        self
    }
    /// Colours for controls floating over the video.
    pub fn glass(mut self) -> Self {
        self.glass = true;
        self
    }
    pub fn filled_glyph(mut self) -> Self {
        self.fill = true;
        self
    }
    pub fn bold_glyph(mut self) -> Self {
        self.bold = true;
        self
    }

    pub fn show(self, ui: &mut Ui, p: &Palette) -> Response {
        let sense = if self.enabled { Sense::click() } else { Sense::hover() };
        let (rect, resp) = ui.allocate_exact_size(Vec2::splat(self.size), sense);
        if ui.is_rect_visible(rect) {
            let hovered = resp.hovered() && self.enabled;
            let t = ui.ctx().animate_bool_with_time(resp.id.with("hover"), hovered, 0.12);
            let (fg, hover_bg, accent) = if self.glass { (p.glass_text, p.glass_hover, p.accent_glass) } else { (p.text, p.hover, p.accent) };
            let pressed = resp.is_pointer_button_down_on() && self.enabled;
            let body = if pressed { rect.shrink(1.5) } else { rect };
            let cr = CornerRadius::same(radius::CONTROL);
            if self.active {
                ui.painter().rect_filled(body, cr, fade(accent, 0.20));
            }
            if t > 0.0 {
                ui.painter().rect_filled(body, cr, fade(hover_bg, t));
            }
            let color = if !self.enabled {
                fade(fg, 0.35)
            } else if self.active {
                accent
            } else {
                mix(if self.glass { p.glass_dim } else { p.text_dim }, fg, t.max(if pressed { 1.0 } else { 0.0 }))
            };
            let font = if self.bold { FontId::new(self.glyph_size, fonts::icons_bold()) } else { icon_font(self.glyph_size, self.fill) };
            ui.painter().text(body.center(), Align2::CENTER_CENTER, self.glyph.to_string(), font, color);
        }
        if self.enabled
            && let Some(tip) = self.tip
        {
            let shortcut = self.shortcut.clone();
            return resp.on_hover_ui(|ui| tooltip(ui, p, tip, shortcut.as_deref()));
        }
        resp
    }
}

// -------------------------------------------------------------------------------------------------
// Play button
// -------------------------------------------------------------------------------------------------

/// The large accent button in the middle of the dock.
pub fn play_button(ui: &mut Ui, p: &Palette, playing: bool, busy: bool, size: f32, tip: &str, shortcut: Option<String>) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    if ui.is_rect_visible(rect) {
        let t = ui.ctx().animate_bool_with_time(resp.id.with("hover"), resp.hovered(), 0.15);
        let pressed = resp.is_pointer_button_down_on();
        let center = rect.center();
        let r = size * 0.5 * if pressed { 0.94 } else { 1.0 + 0.03 * t };
        // Soft halo.
        if t > 0.01 {
            theme::paint_glow(ui.painter(), center, r * 1.9, fade(p.accent_glass, 0.30 * t));
        }
        let top = mix(p.accent_glass, Color32::WHITE, 0.16);
        ui.painter().circle_filled(center, r, p.accent_glass);
        // A touch of light on the upper half.
        ui.painter().add(Shape::Path(egui::epaint::PathShape {
            points: arc_points(center, r - 0.5, std::f32::consts::PI * 1.08, std::f32::consts::PI * 1.92, 24),
            closed: false,
            fill: Color32::TRANSPARENT,
            stroke: Stroke::new(1.2, fade(top, 0.9)).into(),
        }));
        let glyph = if playing { crate::ui::icons::PAUSE } else { crate::ui::icons::PLAY };
        // The play triangle needs an optical offset to the right so its visual mass is centered in the circle.
        let nudge = if playing { 0.0 } else { size * 0.092 };
        ui.painter().text(center + vec2(nudge, 0.0), Align2::CENTER_CENTER, glyph.to_string(), icon_font(size * 0.46, true), Color32::from_rgb(14, 11, 26));
        if busy {
            let time = ui.input(|i| i.time) as f32;
            spinner(ui.painter(), center, r + 4.0, 2.0, p.accent_glass, time);
            ui.ctx().request_repaint();
        }
    }
    resp.on_hover_ui(|ui| tooltip(ui, p, tip, shortcut.as_deref()))
}

fn arc_points(center: egui::Pos2, radius: f32, from: f32, to: f32, n: usize) -> Vec<egui::Pos2> {
    (0..=n)
        .map(|i| {
            let a = from + (to - from) * i as f32 / n as f32;
            center + Vec2::angled(a) * radius
        })
        .collect()
}

/// A rotating arc. `time` is the current time in seconds.
pub fn spinner(painter: &egui::Painter, center: egui::Pos2, radius: f32, width: f32, color: Color32, time: f32) {
    let start = time * 4.2;
    let sweep = std::f32::consts::PI * (1.1 + 0.5 * (time * 2.4).sin());
    painter.add(Shape::Path(egui::epaint::PathShape {
        points: arc_points(center, radius, start, start + sweep, 32),
        closed: false,
        fill: Color32::TRANSPARENT,
        stroke: Stroke::new(width, color).into(),
    }));
}

// -------------------------------------------------------------------------------------------------
// Switch, segmented control, chips, badges
// -------------------------------------------------------------------------------------------------

/// A sliding on/off switch.
pub fn toggle_switch(ui: &mut Ui, p: &Palette, on: &mut bool) -> Response {
    let size = vec2(42.0, 24.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_with_time(resp.id.with("on"), *on, 0.14);
    if ui.is_rect_visible(rect) {
        let hover = ui.ctx().animate_bool_with_time(resp.id.with("hover"), resp.hovered(), 0.12);
        let track = mix(p.sunken, p.accent, t);
        ui.painter().rect_filled(rect, CornerRadius::same(radius::PILL), track);
        ui.painter().rect_stroke(rect, CornerRadius::same(radius::PILL), Stroke::new(1.0, mix(p.border_strong, p.accent, t)), StrokeKind::Inside);
        let knob_r = 9.0 + hover;
        let x = egui::lerp(rect.left() + 12.0..=rect.right() - 12.0, t);
        let knob_color = mix(p.text_dim, p.on_accent.gamma_multiply(1.0), t);
        let knob_color = if p.dark { mix(knob_color, Color32::WHITE, 0.15 * t) } else { mix(Color32::from_gray(110), Color32::WHITE, t) };
        ui.painter().circle_filled(pos2(x, rect.center().y), knob_r, knob_color);
    }
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Switch, ui.is_enabled(), *on, ""));
    resp
}

/// A row of mutually exclusive pills. Returns `true` when the selection changed.
pub fn segmented<T: Copy + PartialEq>(ui: &mut Ui, p: &Palette, value: &mut T, options: &[(T, &str)]) -> bool {
    let mut changed = false;
    let pad = vec2(14.0, 7.0);
    let font = FontId::new(13.0, fonts::medium());
    let galleys: Vec<_> = options.iter().map(|(_, label)| ui.painter().layout_no_wrap((*label).to_owned(), font.clone(), Color32::PLACEHOLDER)).collect();
    let widths: Vec<f32> = galleys.iter().map(|g| g.size().x + pad.x * 2.0).collect();
    let total = widths.iter().sum::<f32>() + 6.0;
    let height = 13.0 + pad.y * 2.0 + 6.0;
    let base_id = ui.next_auto_id();
    let (rect, _) = ui.allocate_exact_size(vec2(total, height), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(radius::CONTROL + 3), p.sunken);
    ui.painter().rect_stroke(rect, CornerRadius::same(radius::CONTROL + 3), Stroke::new(1.0, p.border), StrokeKind::Inside);
    let mut x = rect.left() + 3.0;
    for (i, (v, _)) in options.iter().enumerate() {
        let r = Rect::from_min_size(pos2(x, rect.top() + 3.0), vec2(widths[i], height - 6.0));
        let resp = ui.interact(r, base_id.with(i), Sense::click());
        let selected = *value == *v;
        let t = ui.ctx().animate_bool_with_time(resp.id.with("sel"), selected, 0.14);
        let hover = ui.ctx().animate_bool_with_time(resp.id.with("hover"), resp.hovered() && !selected, 0.1);
        if t > 0.0 {
            ui.painter().rect_filled(r, CornerRadius::same(radius::CONTROL), fade(p.accent_soft, t.max(0.01)).gamma_multiply(1.0));
            ui.painter().rect_stroke(r, CornerRadius::same(radius::CONTROL), Stroke::new(1.0, fade(p.accent, 0.6 * t)), StrokeKind::Inside);
        }
        if hover > 0.0 {
            ui.painter().rect_filled(r, CornerRadius::same(radius::CONTROL), fade(p.hover, hover));
        }
        let color = mix(p.text_dim, p.accent, t);
        let color = if selected && p.dark { mix(color, p.text, 0.35) } else { color };
        ui.painter().galley(r.center() - galleys[i].size() * 0.5, galleys[i].clone(), color);
        if resp.clicked() && !selected {
            *value = *v;
            changed = true;
        }
        x += widths[i];
    }
    changed
}

/// A small rounded label that can be selected (speed presets, tags).
pub fn chip(ui: &mut Ui, p: &Palette, text: &str, selected: bool) -> Response {
    let font = FontId::new(12.5, fonts::medium());
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, Color32::PLACEHOLDER);
    let size = vec2(galley.size().x + 22.0, 28.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let hover = ui.ctx().animate_bool_with_time(resp.id.with("hover"), resp.hovered(), 0.1);
    let cr = CornerRadius::same(radius::PILL);
    let fill = if selected { p.accent_soft } else { mix(p.raised, p.hover, hover) };
    ui.painter().rect_filled(rect, cr, fill);
    ui.painter().rect_stroke(rect, cr, Stroke::new(1.0, if selected { fade(p.accent, 0.7) } else { p.border }), StrokeKind::Inside);
    let color = if selected { mix(p.accent, p.text, if p.dark { 0.3 } else { 0.0 }) } else { p.text_dim };
    ui.painter().galley(rect.center() - galley.size() * 0.5, galley, color);
    resp
}

/// A keyboard key badge (`Ctrl+K`).
pub fn kbd(ui: &mut Ui, p: &Palette, text: &str) {
    let font = FontId::new(11.5, fonts::mono());
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, Color32::PLACEHOLDER);
    let size = vec2((galley.size().x + 12.0).max(22.0), 20.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(6), p.sunken);
    ui.painter().rect_stroke(rect, CornerRadius::same(6), Stroke::new(1.0, p.border_strong), StrokeKind::Inside);
    ui.painter().galley(rect.center() - galley.size() * 0.5, galley, p.text_dim);
}

/// Width of [`button`] for this label, to centre a row of buttons.
pub fn button_width(ui: &Ui, label: &str, has_icon: bool) -> f32 {
    let font = FontId::new(14.0, fonts::semibold());
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, Color32::WHITE);
    galley.size().x + if has_icon { 24.0 } else { 0.0 } + 32.0
}

/// Primary (accent) or secondary button with an optional leading icon.
pub fn button(ui: &mut Ui, p: &Palette, icon: Option<char>, label: &str, primary: bool) -> Response {
    let font = FontId::new(14.0, fonts::semibold());
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, Color32::PLACEHOLDER);
    let icon_w = if icon.is_some() { 24.0 } else { 0.0 };
    let size = vec2(galley.size().x + icon_w + 32.0, 38.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let hover = ui.ctx().animate_bool_with_time(resp.id.with("hover"), resp.hovered(), 0.12);
    let pressed = resp.is_pointer_button_down_on();
    let body = if pressed { rect.shrink(1.0) } else { rect };
    let cr = CornerRadius::same(radius::CONTROL + 2);
    let (fill, fg, stroke) = if primary {
        (mix(p.accent, p.accent_hover, hover), p.on_accent, Stroke::NONE)
    } else {
        (mix(p.raised, p.hover, hover), p.text, Stroke::new(1.0, mix(p.border, p.border_strong, hover)))
    };
    ui.painter().rect_filled(body, cr, fill);
    if stroke != Stroke::NONE {
        ui.painter().rect_stroke(body, cr, stroke, StrokeKind::Inside);
    }
    let total = galley.size().x + icon_w;
    let mut x = body.center().x - total * 0.5;
    if let Some(i) = icon {
        ui.painter().text(pos2(x + 9.0, body.center().y), Align2::CENTER_CENTER, i.to_string(), icon_font(18.0, false), fg);
        x += icon_w;
    }
    ui.painter().galley(pos2(x, body.center().y - galley.size().y * 0.5), galley, fg);
    resp
}

// -------------------------------------------------------------------------------------------------
// Layout helpers for settings-like windows
// -------------------------------------------------------------------------------------------------

/// A section heading.
pub fn section_title(ui: &mut Ui, p: &Palette, text: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(text.to_uppercase()).font(FontId::new(11.5, fonts::semibold())).color(p.text_faint).extra_letter_spacing(0.8));
    ui.add_space(2.0);
}

/// A settings row: label (+ hint) on the left, the control on the right.
pub fn setting_row(ui: &mut Ui, p: &Palette, label: &str, hint: Option<&str>, add: impl FnOnce(&mut Ui)) {
    const ROW: f32 = 46.0;
    ui.scope(|ui| {
        // Rows touch each other: the divider below each one is the only separation.
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.horizontal(|ui| {
            ui.set_min_height(ROW);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                // A long hint wraps before it reaches the control, which is laid out after it.
                if hint.is_some() {
                    ui.set_max_width((ui.available_width() * 0.56).max(240.0));
                }
                // The label block (one or two lines) is centred in the row, level with the control.
                let block = if hint.is_some() { 20.0 + 2.0 + 16.0 } else { 20.0 };
                ui.add_space(((ROW - block) * 0.5).max(0.0));
                ui.label(RichText::new(label).color(p.text));
                if let Some(h) = hint {
                    ui.label(RichText::new(h).size(12.0).color(p.text_faint));
                }
            });
            ui.with_layout(Layout::right_to_left(Align::Center), add);
        });
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
        ui.painter().rect_filled(rect, 0.0, fade(p.border, 0.8));
    });
}

// -------------------------------------------------------------------------------------------------
// Menu rows
// -------------------------------------------------------------------------------------------------

/// What a menu row shows in its leading column.
#[derive(Clone, Copy)]
pub enum Lead {
    None,
    Icon(char),
    /// A check mark when `true`, empty space when `false`.
    Check(bool),
    /// A filled dot for radio-style groups.
    Radio(bool),
}

/// One row of a popup or context menu.
pub fn menu_item(ui: &mut Ui, p: &Palette, lead: Lead, label: &str, shortcut: Option<&str>, enabled: bool) -> Response {
    let height = 32.0;
    let width = ui.available_width().max(220.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(width, height), if enabled { Sense::click() } else { Sense::hover() });
    if ui.is_rect_visible(rect) {
        let hover = ui.ctx().animate_bool_with_time(resp.id.with("hover"), resp.hovered() && enabled, 0.08);
        if hover > 0.0 {
            ui.painter().rect_filled(rect, CornerRadius::same(8), fade(p.hover, hover));
        }
        let fg = if enabled { p.text } else { p.text_faint };
        let lead_x = rect.left() + 18.0;
        match lead {
            Lead::None => {}
            Lead::Icon(c) => {
                ui.painter().text(pos2(lead_x, rect.center().y), Align2::CENTER_CENTER, c.to_string(), icon_font(17.0, false), if enabled { p.text_dim } else { p.text_faint });
            }
            Lead::Check(on) => {
                if on {
                    ui.painter().text(pos2(lead_x, rect.center().y), Align2::CENTER_CENTER, crate::ui::icons::CHECK.to_string(), FontId::new(15.0, fonts::icons_bold()), p.accent);
                }
            }
            Lead::Radio(on) => {
                let c = pos2(lead_x, rect.center().y);
                ui.painter().circle_stroke(c, 6.0, Stroke::new(1.5, if on { p.accent } else { p.text_faint }));
                if on {
                    ui.painter().circle_filled(c, 3.0, p.accent);
                }
            }
        }
        let text_x = rect.left() + if matches!(lead, Lead::None) { 12.0 } else { 38.0 };
        ui.painter().text(pos2(text_x, rect.center().y), Align2::LEFT_CENTER, label, FontId::new(14.0, fonts::medium()), fg);
        if let Some(s) = shortcut {
            ui.painter().text(pos2(rect.right() - 12.0, rect.center().y), Align2::RIGHT_CENTER, s, FontId::new(12.0, fonts::mono()), p.text_faint);
        }
    }
    resp
}

/// A thin separator between groups of menu rows.
pub fn menu_separator(ui: &mut Ui, p: &Palette) {
    ui.add_space(3.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect.shrink2(vec2(8.0, 0.0)), 0.0, p.border);
    ui.add_space(3.0);
}

// -------------------------------------------------------------------------------------------------
// Thin slider
// -------------------------------------------------------------------------------------------------

/// A slim slider with an accent fill: volume, equaliser bands, picture adjustments.
pub struct ThinSlider<'a> {
    value: &'a mut f32,
    range: RangeInclusive<f32>,
    length: f32,
    vertical: bool,
    glass: bool,
    /// Positions (in value units) that get a tick.
    marks: &'a [f32],
    /// Above this value the fill turns amber (volume boost).
    warn_above: Option<f32>,
    /// Snap to the nearest multiple of this step.
    step: Option<f32>,
    /// A value that the knob snaps to when it is close (the neutral position).
    detent: Option<f32>,
    /// The fill starts here instead of at the minimum (bipolar sliders: gains, adjustments).
    origin: Option<f32>,
}

impl<'a> ThinSlider<'a> {
    pub fn new(value: &'a mut f32, range: RangeInclusive<f32>) -> Self {
        Self { value, range, length: 120.0, vertical: false, glass: false, marks: &[], warn_above: None, step: None, detent: None, origin: None }
    }
    pub fn length(mut self, l: f32) -> Self {
        self.length = l;
        self
    }
    pub fn vertical(mut self) -> Self {
        self.vertical = true;
        self
    }
    pub fn glass(mut self) -> Self {
        self.glass = true;
        self
    }
    pub fn marks(mut self, m: &'a [f32]) -> Self {
        self.marks = m;
        self
    }
    pub fn warn_above(mut self, v: f32) -> Self {
        self.warn_above = Some(v);
        self
    }
    pub fn step(mut self, s: f32) -> Self {
        self.step = Some(s);
        self
    }
    pub fn detent(mut self, v: f32) -> Self {
        self.detent = Some(v);
        self
    }
    /// Fills from `v` to the knob, in either direction.
    pub fn origin(mut self, v: f32) -> Self {
        self.origin = Some(v);
        self
    }

    pub fn show(self, ui: &mut Ui, p: &Palette) -> Response {
        let (lo, hi) = (*self.range.start(), *self.range.end());
        let thick = 22.0;
        let size = if self.vertical { vec2(thick, self.length) } else { vec2(self.length, thick) };
        let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let inset = 8.0;
        let (a, b) = if self.vertical { (rect.bottom() - inset, rect.top() + inset) } else { (rect.left() + inset, rect.right() - inset) };
        let to_pos = |v: f32| egui::lerp(a..=b, ((v - lo) / (hi - lo)).clamp(0.0, 1.0));
        if (resp.dragged() || resp.clicked() || resp.is_pointer_button_down_on())
            && let Some(pt) = resp.interact_pointer_pos()
        {
            let along = if self.vertical { pt.y } else { pt.x };
            let mut v = lo + ((along - a) / (b - a)).clamp(0.0, 1.0) * (hi - lo);
            if let Some(s) = self.step {
                v = (v / s).round() * s;
            }
            if let Some(d) = self.detent
                && (v - d).abs() < (hi - lo) * 0.025
            {
                v = d;
            }
            let v = v.clamp(lo, hi);
            if (v - *self.value).abs() > f32::EPSILON {
                *self.value = v;
                resp.mark_changed();
            }
        }
        if ui.is_rect_visible(rect) {
            let active = resp.hovered() || resp.dragged();
            let t = ui.ctx().animate_bool_with_time(resp.id.with("active"), active, 0.12);
            let (track, fill_col, knob) = if self.glass {
                (Color32::from_white_alpha(46), p.accent_glass, Color32::WHITE)
            } else {
                (p.border_strong, p.accent, if p.dark { Color32::WHITE } else { Color32::from_gray(250) })
            };
            let fill_col = match self.warn_above {
                Some(w) if *self.value > w + 0.001 => {
                    if self.glass {
                        Color32::from_rgb(251, 191, 36)
                    } else {
                        p.warn
                    }
                }
                _ => fill_col,
            };
            let thickness = 4.0 + 2.0 * t;
            let cr = CornerRadius::same(radius::PILL);
            let start = self.origin.map_or(lo, |o| o.clamp(lo, hi));
            let (track_rect, fill_rect) = if self.vertical {
                let x = rect.center().x;
                let tr = Rect::from_min_max(pos2(x - thickness / 2.0, b), pos2(x + thickness / 2.0, a));
                let (y0, y1) = (to_pos(*self.value), to_pos(start));
                (tr, Rect::from_min_max(pos2(x - thickness / 2.0, y0.min(y1)), pos2(x + thickness / 2.0, y0.max(y1))))
            } else {
                let y = rect.center().y;
                let tr = Rect::from_min_max(pos2(a, y - thickness / 2.0), pos2(b, y + thickness / 2.0));
                let (x0, x1) = (to_pos(start), to_pos(*self.value));
                (tr, Rect::from_min_max(pos2(x0.min(x1), y - thickness / 2.0), pos2(x0.max(x1), y + thickness / 2.0)))
            };
            ui.painter().rect_filled(track_rect, cr, track);
            for &m in self.marks {
                let pos = to_pos(m);
                let (c0, c1) = if self.vertical {
                    (pos2(rect.center().x - 5.0, pos), pos2(rect.center().x + 5.0, pos))
                } else {
                    (pos2(pos, rect.center().y - 5.0), pos2(pos, rect.center().y + 5.0))
                };
                ui.painter().line_segment([c0, c1], Stroke::new(1.0, fade(if self.glass { Color32::WHITE } else { p.text_faint }, 0.35)));
            }
            ui.painter().rect_filled(fill_rect, cr, fill_col);
            let center = if self.vertical { pos2(rect.center().x, to_pos(*self.value)) } else { pos2(to_pos(*self.value), rect.center().y) };
            let knob_r = 5.5 + 1.5 * t;
            ui.painter().circle_filled(center + vec2(0.0, 1.0), knob_r + 1.0, Color32::from_black_alpha(60));
            ui.painter().circle_filled(center, knob_r, knob);
        }
        resp
    }
}

#[cfg(test)]
mod tests {
    use egui::{Context, RawInput};

    use super::*;

    fn run(f: impl FnMut(&mut Ui)) {
        let ctx = Context::default();
        crate::fonts::install(&ctx);
        let pal = Palette::new(lumen400_core::settings::ThemeMode::Dark, lumen400_core::settings::Accent::Violet, true);
        theme::apply(&ctx, &pal, 1.0);
        let mut f = f;
        for _ in 0..2 {
            let mut out = ctx.run_ui(RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0))), ..Default::default() }, |ui| {
                f(ui);
            });
            out.textures_delta.clear();
        }
    }

    fn pal() -> Palette {
        Palette::new(lumen400_core::settings::ThemeMode::Dark, lumen400_core::settings::Accent::Violet, true)
    }

    #[test]
    fn widgets_lay_out_without_panicking() {
        let p = pal();
        let mut on = false;
        let mut seg = 1;
        let mut vol = 0.5;
        run(|ui| {
            IconButton::new(crate::ui::icons::PLAY).tip("Play").shortcut(Some("Space".into())).show(ui, &p);
            IconButton::new(crate::ui::icons::GEAR).glass().active(true).show(ui, &p);
            play_button(ui, &p, true, true, 52.0, "Pause", None);
            toggle_switch(ui, &p, &mut on);
            segmented(ui, &p, &mut seg, &[(0, "A"), (1, "B"), (2, "C")]);
            chip(ui, &p, "1.25×", true);
            kbd(ui, &p, "Ctrl+K");
            button(ui, &p, Some(crate::ui::icons::FOLDER_OPEN), "Open", true);
            button(ui, &p, None, "Cancel", false);
            section_title(ui, &p, "Section");
            setting_row(ui, &p, "Label", Some("hint"), |ui| {
                ui.label("x");
            });
            menu_item(ui, &p, Lead::Check(true), "Item", Some("Ctrl+O"), true);
            menu_separator(ui, &p);
            ThinSlider::new(&mut vol, 0.0..=2.0).marks(&[1.0]).warn_above(1.0).show(ui, &p);
        });
        assert!(!on);
        assert_eq!(seg, 1);
    }

    #[test]
    fn spinner_arc_points_stay_on_the_circle() {
        let pts = arc_points(pos2(10.0, 10.0), 5.0, 0.0, 3.0, 16);
        assert_eq!(pts.len(), 17);
        for q in pts {
            assert!(((q - pos2(10.0, 10.0)).length() - 5.0).abs() < 1e-4);
        }
    }
}

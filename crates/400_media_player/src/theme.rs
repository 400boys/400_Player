//! The 400_media_player design system: colour palette, typography and the egui style derived from them.
//!
//! The look is built around a deep blue-black "nocturne" canvas, translucent glass overlays that
//! float above the picture and a single saturated accent — the *400* shade of a Tailwind-style
//! colour ramp, hence the name. Overlays that sit on top of the video are always dark glass
//! (video content is arbitrary, so their contrast must not depend on the chosen theme); panels,
//! windows and menus follow the selected theme.

use egui::{Color32, CornerRadius, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle, Vec2};
use media_player_400_core::settings::{Accent, ThemeMode};

/// Radii shared by every component so the whole UI feels cut from the same block.
pub mod radius {
    pub const WINDOW: u8 = 18;
    pub const CARD: u8 = 14;
    pub const CONTROL: u8 = 10;
    pub const PILL: u8 = 255;
}

/// Colour tokens. Build one with [`Palette::new`].
#[derive(Clone, Debug)]
pub struct Palette {
    pub dark: bool,
    /// The accent colour used for highlights (400 shade on dark, 600 on light).
    pub accent: Color32,
    pub accent_hover: Color32,
    /// Translucent accent, for fills of selected items.
    pub accent_soft: Color32,
    pub accent_faint: Color32,
    /// Text drawn on top of a solid accent fill.
    pub on_accent: Color32,

    /// Behind the video (letter-box bars): always near black.
    pub video_bg: Color32,
    /// Window / panel background.
    pub panel: Color32,
    /// Cards, inputs, buttons.
    pub raised: Color32,
    pub hover: Color32,
    /// Darker than `panel` (text fields, tracks).
    pub sunken: Color32,
    pub border: Color32,
    pub border_strong: Color32,

    pub text: Color32,
    pub text_dim: Color32,
    pub text_faint: Color32,

    pub danger: Color32,
    pub warn: Color32,

    /// Floating controls above the video (always dark).
    pub glass: Color32,
    pub glass_hover: Color32,
    pub glass_border: Color32,
    pub glass_text: Color32,
    pub glass_dim: Color32,
    /// The accent as it appears *on glass*: always the bright 400 shade, whatever the theme.
    pub accent_glass: Color32,
}

fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

fn alpha(c: [u8; 3], a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], a)
}

fn lighten(c: [u8; 3], t: f32) -> [u8; 3] {
    [0, 1, 2].map(|i| (f32::from(c[i]) + (255.0 - f32::from(c[i])) * t).round() as u8)
}

impl Palette {
    /// Resolves a theme mode (with the OS preference for [`ThemeMode::System`]) and an accent.
    pub fn new(mode: ThemeMode, accent: Accent, system_dark: bool) -> Self {
        let (dark, oled) = match mode {
            ThemeMode::Dark => (true, false),
            ThemeMode::Oled => (true, true),
            ThemeMode::Light => (false, false),
            ThemeMode::System => (system_dark, false),
        };
        let a = if dark { accent.rgb() } else { accent.rgb_deep() };
        let (panel, raised, hover, sunken) = if !dark {
            ([255, 255, 255], [243, 243, 249], [233, 233, 243], [236, 236, 244])
        } else if oled {
            ([5, 5, 8], [14, 14, 20], [24, 24, 34], [0, 0, 0])
        } else {
            ([14, 14, 22], [23, 23, 35], [32, 32, 48], [9, 9, 15])
        };
        let (text, text_dim, text_faint) = if dark { ([236, 236, 245], [160, 160, 184], [106, 106, 130]) } else { ([20, 20, 31], [91, 91, 112], [142, 142, 163]) };
        let border = if dark { Color32::from_white_alpha(20) } else { Color32::from_black_alpha(22) };
        let border_strong = if dark { Color32::from_white_alpha(44) } else { Color32::from_black_alpha(50) };
        Palette {
            dark,
            accent: rgb(a),
            accent_hover: rgb(if dark { lighten(a, 0.18) } else { [0, 1, 2].map(|i| (f32::from(a[i]) * 0.88) as u8) }),
            accent_soft: alpha(a, if dark { 56 } else { 40 }),
            accent_faint: alpha(a, if dark { 24 } else { 18 }),
            on_accent: if dark { Color32::from_rgb(12, 10, 22) } else { Color32::WHITE },
            video_bg: if oled { Color32::BLACK } else { Color32::from_rgb(6, 6, 10) },
            panel: rgb(panel),
            raised: rgb(raised),
            hover: rgb(hover),
            sunken: rgb(sunken),
            border,
            border_strong,
            text: rgb(text),
            text_dim: rgb(text_dim),
            text_faint: rgb(text_faint),
            danger: if dark { Color32::from_rgb(251, 113, 133) } else { Color32::from_rgb(225, 29, 72) },
            warn: if dark { Color32::from_rgb(251, 191, 36) } else { Color32::from_rgb(217, 119, 6) },
            glass: Color32::from_rgba_unmultiplied(13, 13, 21, 196),
            glass_hover: Color32::from_white_alpha(26),
            glass_border: Color32::from_white_alpha(30),
            glass_text: Color32::from_rgb(244, 244, 250),
            glass_dim: Color32::from_rgb(176, 176, 198),
            accent_glass: rgb(accent.rgb()),
        }
    }

    pub fn popup_shadow(&self) -> Shadow {
        Shadow { offset: [0, 10], blur: 34, spread: 2, color: Color32::from_black_alpha(if self.dark { 150 } else { 60 }) }
    }

    pub fn glass_shadow(&self) -> Shadow {
        Shadow { offset: [0, 8], blur: 36, spread: 0, color: Color32::from_black_alpha(140) }
    }

    /// Frame of the floating control dock.
    pub fn glass_frame(&self) -> egui::Frame {
        egui::Frame::new()
            .fill(self.glass)
            .stroke(Stroke::new(1.0, self.glass_border))
            .corner_radius(CornerRadius::same(radius::WINDOW))
            .shadow(self.glass_shadow())
            .inner_margin(Margin::symmetric(16, 10))
    }

    /// Frame of cards inside windows and drawers.
    pub fn card_frame(&self) -> egui::Frame {
        egui::Frame::new().fill(self.raised).stroke(Stroke::new(1.0, self.border)).corner_radius(CornerRadius::same(radius::CARD)).inner_margin(Margin::same(14))
    }

    /// Frame of drawers and big windows.
    pub fn window_frame(&self) -> egui::Frame {
        egui::Frame::new()
            .fill(self.panel)
            .stroke(Stroke::new(1.0, self.border_strong))
            .corner_radius(CornerRadius::same(radius::WINDOW))
            .shadow(self.popup_shadow())
            .inner_margin(Margin::same(18))
    }

    /// Frame of context menus and popups.
    pub fn popup_frame(&self) -> egui::Frame {
        egui::Frame::new()
            .fill(self.panel)
            .stroke(Stroke::new(1.0, self.border_strong))
            .corner_radius(CornerRadius::same(12))
            .shadow(self.popup_shadow())
            .inner_margin(Margin::same(6))
    }
}

// -------------------------------------------------------------------------------------------------
// Typography
// -------------------------------------------------------------------------------------------------

/// Font families registered by [`crate::fonts::install`].
pub mod fonts {
    use egui::FontFamily;

    pub fn medium() -> FontFamily {
        FontFamily::Name("inter-medium".into())
    }
    pub fn semibold() -> FontFamily {
        FontFamily::Name("inter-semibold".into())
    }
    pub fn bold() -> FontFamily {
        FontFamily::Name("inter-bold".into())
    }
    pub fn mono() -> FontFamily {
        FontFamily::Monospace
    }
    pub fn icons() -> FontFamily {
        FontFamily::Name("phosphor".into())
    }
    pub fn icons_bold() -> FontFamily {
        FontFamily::Name("phosphor-bold".into())
    }
    pub fn icons_fill() -> FontFamily {
        FontFamily::Name("phosphor-fill".into())
    }
}

pub fn display_style() -> TextStyle {
    TextStyle::Name("Display".into())
}
pub fn title_style() -> TextStyle {
    TextStyle::Name("Title".into())
}
pub fn caption_style() -> TextStyle {
    TextStyle::Name("Caption".into())
}
pub fn timecode_style() -> TextStyle {
    TextStyle::Name("Timecode".into())
}

/// Installs the palette as the egui style of the current theme.
pub fn apply(ctx: &egui::Context, p: &Palette, ui_scale: f32) {
    ctx.options_mut(|o| {
        o.theme_preference = if p.dark { egui::ThemePreference::Dark } else { egui::ThemePreference::Light };
    });
    let mut style = (*ctx.global_style()).clone();
    style.text_styles = [
        (TextStyle::Small, FontId::new(11.5, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(14.0, fonts::medium())),
        (TextStyle::Heading, FontId::new(19.0, fonts::semibold())),
        (TextStyle::Monospace, FontId::new(13.0, FontFamily::Monospace)),
        (display_style(), FontId::new(40.0, fonts::bold())),
        (title_style(), FontId::new(22.0, fonts::semibold())),
        (caption_style(), FontId::new(12.0, fonts::medium())),
        (timecode_style(), FontId::new(13.0, FontFamily::Monospace)),
    ]
    .into();

    let sp = &mut style.spacing;
    sp.item_spacing = Vec2::new(8.0, 8.0);
    sp.button_padding = Vec2::new(14.0, 8.0);
    sp.window_margin = Margin::same(18);
    sp.menu_margin = Margin::same(6);
    sp.interact_size = Vec2::new(36.0, 34.0);
    sp.indent = 16.0;
    sp.slider_width = 160.0;
    sp.slider_rail_height = 6.0;
    sp.combo_width = 140.0;
    sp.icon_width = 18.0;
    sp.icon_width_inner = 10.0;
    sp.scroll.bar_width = 8.0;
    sp.scroll.floating = true;
    sp.scroll.floating_width = 4.0;
    sp.scroll.floating_allocated_width = 0.0;
    sp.tooltip_width = 360.0;
    style.interaction.tooltip_delay = 0.35;
    style.interaction.tooltip_grace_time = 0.2;

    let v = &mut style.visuals;
    v.dark_mode = p.dark;
    v.override_text_color = None;
    v.weak_text_alpha = 0.7;
    v.panel_fill = p.panel;
    v.window_fill = p.panel;
    v.window_stroke = Stroke::new(1.0, p.border_strong);
    v.window_corner_radius = CornerRadius::same(radius::WINDOW);
    v.window_shadow = p.popup_shadow();
    v.popup_shadow = p.popup_shadow();
    v.menu_corner_radius = CornerRadius::same(12);
    v.extreme_bg_color = p.sunken;
    v.faint_bg_color = p.raised;
    v.code_bg_color = p.raised;
    v.hyperlink_color = p.accent;
    v.warn_fg_color = p.warn;
    v.error_fg_color = p.danger;
    v.slider_trailing_fill = true;
    v.handle_shape = egui::style::HandleShape::Circle;
    v.interact_cursor = Some(egui::CursorIcon::PointingHand);
    v.selection.bg_fill = p.accent_soft;
    v.selection.stroke = Stroke::new(1.0, p.accent);
    v.text_cursor.stroke = Stroke::new(2.0, p.accent);

    let cr = CornerRadius::same(radius::CONTROL);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.panel;
    w.noninteractive.weak_bg_fill = p.panel;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.noninteractive.corner_radius = cr;

    w.inactive.bg_fill = p.raised;
    w.inactive.weak_bg_fill = p.raised;
    w.inactive.bg_stroke = Stroke::new(1.0, p.border);
    w.inactive.fg_stroke = Stroke::new(1.0, p.text);
    w.inactive.corner_radius = cr;

    w.hovered.bg_fill = p.hover;
    w.hovered.weak_bg_fill = p.hover;
    w.hovered.bg_stroke = Stroke::new(1.0, p.border_strong);
    w.hovered.fg_stroke = Stroke::new(1.5, p.text);
    w.hovered.corner_radius = cr;
    w.hovered.expansion = 0.0;

    w.active.bg_fill = p.accent_soft;
    w.active.weak_bg_fill = p.accent_soft;
    w.active.bg_stroke = Stroke::new(1.0, p.accent);
    w.active.fg_stroke = Stroke::new(1.5, p.text);
    w.active.corner_radius = cr;
    w.active.expansion = 0.0;

    w.open.bg_fill = p.hover;
    w.open.weak_bg_fill = p.hover;
    w.open.bg_stroke = Stroke::new(1.0, p.accent);
    w.open.fg_stroke = Stroke::new(1.0, p.text);
    w.open.corner_radius = cr;

    ctx.set_global_style(style);
    ctx.set_zoom_factor(ui_scale.clamp(0.8, 1.6));
}

/// Mixes two colours (`t = 0` → `a`, `t = 1` → `b`) in gamma space.
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let f = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgba_premultiplied(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()), f(a.a(), b.a()))
}

/// `c` with its opacity multiplied by `k`.
pub fn fade(c: Color32, k: f32) -> Color32 {
    c.gamma_multiply(k.clamp(0.0, 1.0))
}

/// Paints a vertical gradient (`top` at the upper edge, `bottom` at the lower one).
pub fn paint_vgradient(painter: &egui::Painter, rect: egui::Rect, top: Color32, bottom: Color32) {
    let mut mesh = egui::Mesh::default();
    let uv = egui::epaint::WHITE_UV;
    let v = |pos: egui::Pos2, color: Color32| egui::epaint::Vertex { pos, uv, color };
    mesh.vertices.push(v(rect.left_top(), top));
    mesh.vertices.push(v(rect.right_top(), top));
    mesh.vertices.push(v(rect.right_bottom(), bottom));
    mesh.vertices.push(v(rect.left_bottom(), bottom));
    mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    painter.add(egui::Shape::mesh(mesh));
}

/// Paints a soft radial glow (aurora of the welcome screen, halos of buttons): full colour at the
/// centre, fading to nothing at `radius` along a smooth curve.
pub fn paint_glow(painter: &egui::Painter, center: egui::Pos2, radius: f32, color: Color32) {
    const SEGMENTS: usize = 40;
    const RINGS: usize = 5;
    let mut mesh = egui::Mesh::default();
    let uv = egui::epaint::WHITE_UV;
    mesh.vertices.push(egui::epaint::Vertex { pos: center, uv, color });
    for ring in 1..=RINGS {
        let r = ring as f32 / RINGS as f32;
        let falloff = (1.0 - r) * (1.0 - r);
        let c = fade(color, falloff);
        for i in 0..SEGMENTS {
            let a = i as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
            mesh.vertices.push(egui::epaint::Vertex { pos: center + Vec2::angled(a) * (radius * r), uv, color: c });
        }
    }
    let at = |ring: usize, i: usize| (1 + (ring - 1) * SEGMENTS + i % SEGMENTS) as u32;
    for i in 0..SEGMENTS {
        mesh.indices.extend_from_slice(&[0, at(1, i), at(1, i + 1)]);
    }
    for ring in 1..RINGS {
        for i in 0..SEGMENTS {
            let (a, b, c, d) = (at(ring, i), at(ring, i + 1), at(ring + 1, i + 1), at(ring + 1, i));
            mesh.indices.extend_from_slice(&[a, b, c, a, c, d]);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// Fills a rounded rectangle with a vertical gradient (a plain `rect_filled` only does flat colours).
pub fn paint_rounded_vgradient(painter: &egui::Painter, rect: egui::Rect, radius: f32, top: Color32, bottom: Color32) {
    let r = radius.min(rect.width() * 0.5).min(rect.height() * 0.5).max(0.0);
    let steps = 8usize;
    let mut outline: Vec<egui::Pos2> = Vec::with_capacity(4 * (steps + 1));
    let corners = [
        (egui::pos2(rect.right() - r, rect.top() + r), -std::f32::consts::FRAC_PI_2),
        (egui::pos2(rect.right() - r, rect.bottom() - r), 0.0),
        (egui::pos2(rect.left() + r, rect.bottom() - r), std::f32::consts::FRAC_PI_2),
        (egui::pos2(rect.left() + r, rect.top() + r), std::f32::consts::PI),
    ];
    for (center, start) in corners {
        for i in 0..=steps {
            let a = start + std::f32::consts::FRAC_PI_2 * i as f32 / steps as f32;
            outline.push(center + Vec2::angled(a) * r);
        }
    }
    let h = rect.height().max(1.0);
    let color_at = |y: f32| mix(top, bottom, (y - rect.top()) / h);
    let uv = egui::epaint::WHITE_UV;
    let mut mesh = egui::Mesh::default();
    mesh.vertices.push(egui::epaint::Vertex { pos: rect.center(), uv, color: color_at(rect.center().y) });
    for p in &outline {
        mesh.vertices.push(egui::epaint::Vertex { pos: *p, uv, color: color_at(p.y) });
    }
    let n = outline.len() as u32;
    for i in 0..n {
        mesh.indices.extend_from_slice(&[0, 1 + i, 1 + (i + 1) % n]);
    }
    painter.add(egui::Shape::mesh(mesh));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palettes_resolve_for_every_mode() {
        for mode in ThemeMode::ALL {
            for accent in Accent::ALL {
                let p = Palette::new(mode, accent, true);
                assert_ne!(p.text, p.panel, "{mode:?}/{accent:?}: text must differ from the background");
                assert_ne!(p.accent, p.panel);
            }
        }
        assert!(Palette::new(ThemeMode::System, Accent::Violet, true).dark);
        assert!(!Palette::new(ThemeMode::System, Accent::Violet, false).dark);
        assert!(!Palette::new(ThemeMode::Light, Accent::Violet, true).dark);
    }

    #[test]
    fn text_contrast_is_readable() {
        fn lum(c: Color32) -> f32 {
            let f = |v: u8| {
                let v = f32::from(v) / 255.0;
                if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
            };
            0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
        }
        fn ratio(a: Color32, b: Color32) -> f32 {
            let (la, lb) = (lum(a), lum(b));
            (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
        }
        for mode in [ThemeMode::Dark, ThemeMode::Oled, ThemeMode::Light] {
            for accent in Accent::ALL {
                let p = Palette::new(mode, accent, true);
                assert!(ratio(p.text, p.panel) >= 7.0, "{mode:?}: body text contrast {}", ratio(p.text, p.panel));
                assert!(ratio(p.text_dim, p.panel) >= 4.5, "{mode:?}: secondary text contrast {}", ratio(p.text_dim, p.panel));
                assert!(ratio(p.on_accent, p.accent) >= 4.5, "{mode:?}/{accent:?}: text on accent {}", ratio(p.on_accent, p.accent));
                assert!(ratio(p.accent, p.panel) >= 3.0, "{mode:?}/{accent:?}: accent vs panel {}", ratio(p.accent, p.panel));
            }
        }
    }

    #[test]
    fn mixing() {
        assert_eq!(mix(Color32::BLACK, Color32::WHITE, 0.0), Color32::BLACK);
        assert_eq!(mix(Color32::BLACK, Color32::WHITE, 1.0), Color32::WHITE);
        assert_eq!(fade(Color32::WHITE, 0.0).a(), 0);
    }
}

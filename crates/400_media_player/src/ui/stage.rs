//! The stage: the picture (or, for music, the cover and the spectrum), the subtitles, and the
//! overlays that report what the player is doing (opening, ended, failed).

use egui::{Align, Align2, Color32, Context, CornerRadius, CursorIcon, FontId, Id, Painter, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, UiBuilder, pos2, vec2};
use media_player_400_core::subtitle::Cue;
use media_player_400_engine::PlayState;

use crate::app::MediaPlayer400App;
use crate::i18n::{Key as T, t};
use crate::state::SubSource;
use crate::theme::{self, Palette, fade, fonts, mix};
use crate::ui::icons;
use crate::util::file_name_of;
use crate::video::{self, DrawParams, FitMode, dest_rect, display_size, total_rotation};
use crate::widgets::{self, button, button_width, spinner};

impl MediaPlayer400App {
    pub fn draw_stage(&mut self, ui: &mut Ui, stage: Rect, controls: f32, now: f64) {
        let ctx = ui.ctx().clone();
        let painter = ui.painter_at(stage);
        let audio_only = self.media.as_ref().is_some_and(|m| !m.has_video());

        let resp = ui.interact(stage, Id::unique("stage"), Sense::click_and_drag());
        self.stage_input(&ctx, &resp, now);

        let mut picture = stage;
        if audio_only {
            self.draw_visualizer(&painter, stage, now);
        } else if let Some(frame) = self.frame.clone() {
            picture = self.paint_picture(&ctx, &painter, stage, frame);
        }
        if self.snap.state != PlayState::Failed {
            self.draw_subtitles(&painter, picture, controls);
        }

        match self.snap.state {
            PlayState::Opening | PlayState::Buffering | PlayState::Idle => self.draw_busy(&painter, stage, now),
            PlayState::Ended => self.draw_ended(ui, &painter, stage),
            PlayState::Failed => self.draw_failed(ui, stage),
            _ => {}
        }

        // The pointer disappears together with the controls.
        if controls < 0.01 && self.snap.state == PlayState::Playing && resp.hovered() {
            ctx.set_cursor_icon(CursorIcon::None);
        }
    }

    fn stage_input(&mut self, ctx: &Context, resp: &Response, now: f64) {
        let modal = self.windows.any_modal();
        if resp.double_clicked() && !self.swallow_click && !modal {
            // The first click of the pair already toggled playback: put it back first, then toggle fullscreen.
            self.toggle_play();
            self.toggle_fullscreen();
            let (icon, key) = if self.fullscreen { (icons::ARROWS_OUT, T::Fullscreen) } else { (icons::ARROWS_IN, T::ExitFullscreen) };
            self.osd(icon, t(key), None);
        } else if resp.clicked() && !modal {
            if self.swallow_click {
                // This click only closed a menu.
            } else if self.windows.any_window() || self.menu.is_some() {
                self.menu = None;
            } else {
                self.toggle_play();
            }
        }
        if resp.secondary_clicked() && !modal {
            let pos = resp.interact_pointer_pos().unwrap_or_else(|| resp.rect.center());
            self.open_menu(pos, false, crate::state::Page::Root);
        }
        if resp.dragged_by(egui::PointerButton::Primary) && (self.view.zoom != 1.0 || self.view.fit == FitMode::Original) {
            self.view.pan += resp.drag_delta();
            ctx.set_cursor_icon(CursorIcon::Grabbing);
        }
        let _ = now;
    }

    /// Draws the video frame; returns where the picture landed.
    fn paint_picture(&mut self, ctx: &Context, painter: &Painter, stage: Rect, frame: std::sync::Arc<media_player_400_engine::VideoFrame>) -> Rect {
        let stream_rotation = self.media.as_ref().and_then(|m| m.primary_video().map(|v| v.rotation)).unwrap_or(0);
        let rotation = total_rotation(stream_rotation, self.view.rotation);
        let disp = display_size(frame.width, frame.height, frame.sar, rotation, self.view.aspect);
        let dest = dest_rect(stage, disp, &self.view, ctx.pixels_per_point());
        let params = DrawParams { rotation, flip_h: self.view.flip_h, adjust: self.view.adjust, ..DrawParams::default() };
        let ambient = self.settings.ambient_mode && self.view.fit == FitMode::Fit;
        video::paint(painter, stage, dest, frame, params, ambient);
        dest
    }

    // =============================================================================================
    // Subtitles
    // =============================================================================================

    fn draw_subtitles(&mut self, painter: &Painter, picture: Rect, controls: f32) {
        if self.sub_source == SubSource::Off {
            return;
        }
        let cues = self.active_cues();
        if cues.is_empty() {
            return;
        }
        let ctx = painter.ctx().clone();
        for cue in &cues {
            for bm in &cue.bitmaps {
                let cw = bm.canvas_w.max(1) as f32;
                let ch = bm.canvas_h.max(1) as f32;
                let sx = picture.width() / cw;
                let sy = picture.height() / ch;
                let rw = (bm.w as f32 * sx).max(1.0);
                let rh = (bm.h as f32 * sy).max(1.0);
                let rx = (picture.left() + bm.x as f32 * sx).clamp(picture.left(), (picture.right() - rw).max(picture.left()));
                let mut ry = (picture.top() + bm.y as f32 * sy).clamp(picture.top(), (picture.bottom() - rh).max(picture.top()));
                if let Some(dock) = self.dock_rect
                    && ry + rh > dock.top() - 10.0
                    && bm.y as f32 > ch * 0.5
                {
                    let avoid = (dock.top() - 10.0 - rh).max(picture.top());
                    ry = mix_f32(ry, ry.min(avoid), controls);
                }
                let dest = Rect::from_min_size(pos2(rx, ry), vec2(rw, rh));
                let tex = self.sub_bitmaps.texture(&ctx, bm);
                painter.add(
                    egui::epaint::RectShape::filled(dest, CornerRadius::ZERO, Color32::WHITE).with_texture(tex, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0))),
                );
            }
        }
        let style = self.settings.subtitle_style.clone();
        let size = (picture.height() * 0.046 * style.scale).clamp(12.0, 120.0);
        let mut bottom = picture.bottom() - picture.height() * style.bottom_margin;
        if let Some(dock) = self.dock_rect {
            // Subtitles make room for the dock while it is on screen.
            let avoid = dock.top() - 14.0;
            bottom = mix_f32(bottom, bottom.min(avoid), controls);
        }
        paint_cues(painter, picture, &cues, &style, size, bottom);
    }

    // =============================================================================================
    // Audio-only presentation
    // =============================================================================================

    fn draw_visualizer(&mut self, painter: &Painter, stage: Rect, now: f64) {
        let p = self.pal.clone();
        let level = self.spectrum.level.clamp(0.0, 1.0);
        let accent = p.accent_glass;
        let warm = mix(accent, Color32::from_rgb(232, 121, 249), 0.5);
        let (w, h) = (stage.width(), stage.height());
        let big = w.max(h);
        theme::paint_glow(painter, pos2(stage.left() + w * 0.28, stage.top() + h * 0.30), big * (0.55 + 0.08 * level), fade(accent, 0.26 + 0.12 * level));
        theme::paint_glow(painter, pos2(stage.left() + w * 0.80, stage.top() + h * 0.70), big * (0.50 + 0.06 * level), fade(warm, 0.18 + 0.10 * level));

        // Layout, top to bottom: the cover and the text (centred together), then the spectrum band.
        let bars_h = (h * 0.20).clamp(48.0, 150.0);
        let text_h = 108.0;
        let top_pad = 56.0_f32.min(h * 0.12);
        let region = Rect::from_min_max(pos2(stage.left(), stage.top() + top_pad), pos2(stage.right(), stage.bottom() - bars_h - 8.0));
        let cover_size = (region.height() - text_h - 24.0).clamp(96.0, 320.0).min(w * 0.6);
        let block_h = cover_size + text_h;
        let block_top = region.top() + ((region.height() - block_h) * 0.5).max(0.0);
        let cover = Rect::from_min_size(pos2(stage.center().x - cover_size * 0.5, block_top), vec2(cover_size, cover_size));
        let radius = (cover_size * 0.075) as u8;
        painter.add(egui::epaint::Shadow { offset: [0, 18], blur: 48, spread: 2, color: Color32::from_black_alpha(150) }.as_shape(cover, CornerRadius::same(radius)));
        match &self.cover {
            Some(tex) => {
                painter.add(
                    egui::epaint::RectShape::filled(cover, CornerRadius::same(radius), Color32::WHITE).with_texture(tex.id(), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0))),
                );
            }
            None => {
                theme::paint_rounded_vgradient(painter, cover, f32::from(radius), mix(accent, Color32::WHITE, 0.15), mix(accent, Color32::from_rgb(40, 20, 110), 0.7));
                painter.text(
                    cover.center(),
                    Align2::CENTER_CENTER,
                    icons::MUSIC_NOTES.to_string(),
                    FontId::new(cover_size * 0.4, fonts::icons_fill()),
                    Color32::from_white_alpha(230),
                );
            }
        }
        painter.rect_stroke(cover, CornerRadius::same(radius), Stroke::new(1.0, Color32::from_white_alpha(28)), StrokeKind::Inside);

        // Text.
        let title = self.display_title();
        let artist = self.media.as_ref().and_then(|m| m.artist().map(str::to_owned));
        let album = self.media.as_ref().and_then(|m| m.tag("album").map(str::to_owned));
        let title_only = match &artist {
            Some(a) => title.strip_prefix(&format!("{a} — ")).unwrap_or(&title).to_owned(),
            None => title.clone(),
        };
        let mut y = cover.bottom() + 26.0;
        let max_chars = (w / 15.0) as usize;
        painter.text(
            pos2(stage.center().x, y),
            Align2::CENTER_TOP,
            crate::util::ellipsize_middle(&title_only, max_chars.max(12)),
            FontId::new(26.0, fonts::bold()),
            Color32::from_rgb(244, 244, 250),
        );
        y += 36.0;
        if let Some(a) = artist {
            painter.text(pos2(stage.center().x, y), Align2::CENTER_TOP, crate::util::ellipsize_middle(&a, max_chars.max(12)), FontId::new(17.0, fonts::medium()), p.accent_glass);
            y += 26.0;
        }
        if let Some(a) = album {
            painter.text(
                pos2(stage.center().x, y),
                Align2::CENTER_TOP,
                crate::util::ellipsize_middle(&a, max_chars.max(12)),
                FontId::new(14.0, fonts::medium()),
                Color32::from_white_alpha(130),
            );
        }

        // Spectrum along the bottom.
        let bands = &self.spectrum.bands;
        if !bands.is_empty() {
            let area = Rect::from_min_max(pos2(stage.left() + w * 0.12, stage.bottom() - bars_h), pos2(stage.right() - w * 0.12, stage.bottom() - 6.0));
            let n = bands.len();
            let slot = area.width() / n as f32;
            let bar = (slot * 0.62).clamp(2.0, 14.0);
            for (i, v) in bands.iter().enumerate() {
                let x = area.left() + slot * (i as f32 + 0.5);
                let k = i as f32 / (n.max(2) - 1) as f32;
                let hgt = (v.clamp(0.0, 1.0).powf(0.85) * area.height()).max(bar);
                let col = mix(accent, warm, k);
                let r = Rect::from_min_max(pos2(x - bar * 0.5, area.bottom() - hgt), pos2(x + bar * 0.5, area.bottom()));
                painter.rect_filled(r, CornerRadius::same((bar * 0.5) as u8), fade(col, 0.82));
                if let Some(pk) = self.spectrum.peaks.get(i) {
                    let py = area.bottom() - (pk.clamp(0.0, 1.0).powf(0.85) * area.height()).max(bar) - 5.0;
                    painter.rect_filled(Rect::from_center_size(pos2(x, py), vec2(bar, 2.5)), CornerRadius::same(1), fade(col, 0.55));
                }
            }
        }
        let _ = now;
    }

    // =============================================================================================
    // State overlays
    // =============================================================================================

    fn draw_busy(&mut self, painter: &Painter, stage: Rect, now: f64) {
        let since = self.opening_since.unwrap_or(now);
        let t_visible = ((now - since - 0.25) / 0.25).clamp(0.0, 1.0) as f32;
        if t_visible <= 0.0 {
            return;
        }
        let p = &self.pal;
        let c = stage.center();
        painter.circle_filled(c, 40.0, fade(p.glass, t_visible));
        painter.circle_stroke(c, 40.0, Stroke::new(1.0, fade(p.glass_border, t_visible)));
        spinner(painter, c, 22.0, 3.0, fade(p.accent_glass, t_visible), now as f32);
        let label = if self.snap.state == PlayState::Buffering { t(T::Buffering) } else { t(T::Opening) };
        shadowed(painter, pos2(c.x, c.y + 62.0), label, 13.5, fade(Color32::from_white_alpha(210), t_visible));
        self.ctx.request_repaint();
    }

    fn draw_ended(&mut self, ui: &mut Ui, painter: &Painter, stage: Rect) {
        let p = self.pal.clone();
        painter.rect_filled(stage, 0.0, Color32::from_black_alpha(120));
        let c = stage.center();
        let r = Rect::from_center_size(c - vec2(0.0, 14.0), vec2(88.0, 88.0));
        let resp = ui.interact(r, Id::unique("replay"), Sense::click());
        let hover = ui.ctx().animate_bool_with_time(Id::unique("replay-hover"), resp.hovered(), 0.12);
        painter.circle_filled(r.center(), 44.0 + 2.0 * hover, mix(p.glass, p.accent_glass.gamma_multiply(0.35), hover));
        painter.circle_stroke(r.center(), 44.0 + 2.0 * hover, Stroke::new(1.5, mix(p.glass_border, p.accent_glass, hover)));
        painter.text(r.center(), Align2::CENTER_CENTER, icons::ARROW_CCW.to_string(), FontId::new(38.0, fonts::icons_bold()), p.glass_text);
        shadowed(painter, pos2(c.x, c.y + 54.0), t(T::PlayAgain), 15.0, p.glass_text);
        if resp.clicked() {
            self.player.play();
            self.ended_handled = false;
        }
    }

    fn draw_failed(&mut self, ui: &mut Ui, stage: Rect) {
        let p = self.pal.clone();
        let w = 440.0f32.min(stage.width() - 40.0);
        let rect = Rect::from_center_size(stage.center(), vec2(w, 210.0));
        let mut stop = false;
        let mut open = false;
        ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
            p.glass_frame().inner_margin(egui::Margin::same(22)).show(ui, |ui| {
                ui.set_width(w - 44.0);
                ui.vertical_centered(|ui| {
                    ui.label(widgets::icon_text(icons::WARNING_CIRCLE, 38.0, p.danger));
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(t(T::PlaybackFailed)).font(FontId::new(19.0, fonts::semibold())).color(p.glass_text));
                    let name = self.source.as_deref().map(file_name_of).unwrap_or_default();
                    ui.label(egui::RichText::new(crate::util::ellipsize_middle(&name, 52)).size(13.0).color(p.glass_dim));
                    ui.add_space(10.0);
                    let bw = [button_width(ui, t(T::OpenFile), true), button_width(ui, t(T::Close), false)];
                    ui.horizontal(|ui| {
                        ui.add_space(((w - 44.0 - bw[0] - bw[1] - 10.0) * 0.5).max(0.0));
                        if button(ui, &p, Some(icons::FOLDER_OPEN), t(T::OpenFile), true).clicked() {
                            open = true;
                        }
                        if button(ui, &p, None, t(T::Close), false).clicked() {
                            stop = true;
                        }
                    });
                });
            });
        });
        if open {
            self.run(crate::actions::Action::OpenFile);
        }
        if stop {
            self.stop();
        }
    }
}

/// Lays out and paints the active cues.
pub(crate) fn paint_cues(painter: &Painter, picture: Rect, cues: &[Cue], style: &media_player_400_core::settings::SubtitleStyle, size: f32, bottom: f32) {
    use media_player_400_core::subtitle::Align as SubAlign;
    let color = Color32::from_rgb(style.color[0], style.color[1], style.color[2]);
    let max_w = picture.width() * 0.88;
    let mut y_bottom = bottom;
    let mut y_top = picture.top() + picture.height() * 0.06;
    let outline = (size * 0.05).clamp(1.0, 3.2);
    for cue in cues {
        if cue.lines.is_empty() {
            continue;
        }
        let mut job = egui::text::LayoutJob::default();
        job.wrap.max_width = max_w;
        job.halign = match cue.align.horizontal() {
            -1 => Align::Min,
            1 => Align::Max,
            _ => Align::Center,
        };
        for (li, line) in cue.lines.iter().enumerate() {
            if li > 0 {
                job.append("\n", 0.0, egui::TextFormat { font_id: FontId::new(size, fonts::medium()), color, ..Default::default() });
            }
            for span in &line.spans {
                let bold = span.bold || style.bold;
                let c = span.color.map_or(color, |c| Color32::from_rgb(c[0], c[1], c[2]));
                job.append(
                    &span.text,
                    0.0,
                    egui::TextFormat {
                        font_id: FontId::new(size, if bold { fonts::bold() } else { fonts::medium() }),
                        color: c,
                        italics: span.italic,
                        underline: if span.underline { Stroke::new(1.0, c) } else { Stroke::NONE },
                        ..Default::default()
                    },
                );
            }
        }
        let galley = painter.layout_job(job);
        let size_g = galley.rect.size();
        let cx = match cue.align.horizontal() {
            -1 => picture.left() + picture.width() * 0.06 + size_g.x * 0.5,
            1 => picture.right() - picture.width() * 0.06 - size_g.x * 0.5,
            _ => picture.center().x,
        };
        let top = if cue.align.is_top() {
            let t = y_top;
            y_top += size_g.y + 6.0;
            t
        } else if matches!(cue.align, SubAlign::MiddleLeft | SubAlign::MiddleCenter | SubAlign::MiddleRight) {
            picture.center().y - size_g.y * 0.5
        } else {
            y_bottom -= size_g.y;
            let t = y_bottom;
            y_bottom -= 6.0;
            t
        };
        let rect = Rect::from_min_size(pos2(cx - size_g.x * 0.5, top), size_g);
        let origin = pos2(rect.min.x - galley.rect.min.x, rect.min.y - galley.rect.min.y);
        if style.background {
            painter.rect_filled(
                rect.expand2(vec2(size * 0.4, size * 0.12)),
                CornerRadius::same((size * 0.25) as u8),
                Color32::from_black_alpha((style.bg_opacity.clamp(0.0, 1.0) * 255.0) as u8),
            );
        }
        if style.outline {
            let k = std::f32::consts::FRAC_1_SQRT_2;
            let offsets = [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0), (k, k), (-k, k), (k, -k), (-k, -k)];
            for (dx, dy) in offsets {
                painter.galley_with_override_text_color(origin + vec2(dx, dy) * outline, galley.clone(), Color32::from_black_alpha(235));
            }
        }
        painter.galley(origin, galley, color);
    }
}

fn mix_f32(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

/// Text with a soft shadow, centred on `pos`.
fn shadowed(painter: &Painter, pos: Pos2, text: &str, size: f32, color: Color32) {
    painter.text(pos + vec2(0.0, 1.0), Align2::CENTER_CENTER, text, FontId::new(size, fonts::medium()), Color32::from_black_alpha(150));
    painter.text(pos, Align2::CENTER_CENTER, text, FontId::new(size, fonts::medium()), color);
}

#[allow(dead_code)]
fn palette_unused(_: &Palette) {}

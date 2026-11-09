//! Messages drawn over the picture: the on-screen display (volume, seek, speed…), the toasts and
//! the statistics panel.

use egui::{Align2, Color32, Context, CornerRadius, FontId, Id, Order, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
use lumen400_core::time::format_clock;
use lumen400_engine::PlayState;

use crate::app::LumenApp;
use crate::files;
use crate::i18n::{Key as T, t};
use crate::state::{ToastAction, ToastKind};
use crate::theme::{Palette, fade, fonts};
use crate::ui::icons;
use crate::util::{format_bitrate, format_bytes, localize_decimal};
use crate::widgets::icon_text;

/// How long an on-screen display message stays, seconds.
pub const OSD_LIFETIME: f64 = 1.5;

impl LumenApp {
    /// The transient message at the top of the picture.
    pub fn draw_osd(&mut self, ctx: &Context, stage: Rect, now: f64) {
        let Some(osd) = self.osd.clone() else { return };
        let age = now - osd.born;
        let appear = (age / 0.10).clamp(0.0, 1.0) as f32;
        let vanish = ((OSD_LIFETIME - age) / 0.35).clamp(0.0, 1.0) as f32;
        let alpha = appear * vanish;
        if alpha <= 0.0 {
            return;
        }
        let p = self.pal.clone();
        let rise = (1.0 - appear) * -8.0;
        let width = if osd.progress.is_some() { 260.0 } else { 0.0 };
        egui::Area::new(Id::unique("osd")).order(Order::Middle).interactable(false).pivot(Align2::CENTER_TOP).fixed_pos(pos2(stage.center().x, stage.top() + 78.0 + rise)).show(
            ctx,
            |ui| {
                ui.set_opacity(alpha);
                p.glass_frame().inner_margin(egui::Margin::symmetric(18, 12)).corner_radius(CornerRadius::same(255)).show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(10.0, 8.0);
                    if osd.progress.is_none() {
                        ui.horizontal(|ui| {
                            ui.label(icon_text(osd.icon, 22.0, p.accent_glass));
                            ui.label(egui::RichText::new(&osd.text).font(FontId::new(16.0, fonts::semibold())).color(p.glass_text));
                        });
                    } else {
                        ui.set_width(width);
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(icon_text(osd.icon, 22.0, p.accent_glass));
                                ui.label(egui::RichText::new(&osd.text).font(FontId::new(16.0, fonts::semibold())).color(p.glass_text));
                            });
                            if let Some(v) = osd.progress {
                                level_bar(ui, &p, v, osd.tick);
                            }
                        });
                    }
                });
            },
        );
    }

    /// Notifications stacked in the top-right corner.
    pub fn draw_toasts(&mut self, ctx: &Context, stage: Rect, now: f64) {
        if self.toasts.is_empty() {
            return;
        }
        let p = self.pal.clone();
        let mut dismissed: Vec<u64> = Vec::new();
        let mut revealed: Option<std::path::PathBuf> = None;
        let mut y = stage.top() + 66.0;
        let toasts: Vec<_> = self.toasts.iter().map(|t| (t.id, t.kind, t.title.clone(), t.detail.clone(), t.born, t.ttl, t.action.clone())).collect();
        for (id, kind, title, detail, born, ttl, action) in toasts {
            let age = now - born;
            let appear = (age / 0.18).clamp(0.0, 1.0) as f32;
            let vanish = ((ttl - age) / 0.3).clamp(0.0, 1.0) as f32;
            let alpha = appear.min(vanish);
            let slide = (1.0 - ease_out(appear)) * 26.0;
            let (icon, tint) = match kind {
                ToastKind::Info => (icons::INFO, p.accent_glass),
                ToastKind::Success => (icons::CHECK_CIRCLE, p.success_glass()),
                ToastKind::Warning => (icons::WARNING, p.warn_glass()),
                ToastKind::Error => (icons::WARNING_CIRCLE, p.danger_glass()),
            };
            let out = egui::Area::new(Id::unique(("toast", id))).order(Order::Middle).pivot(Align2::RIGHT_TOP).fixed_pos(pos2(stage.right() - 16.0 + slide, y)).show(ctx, |ui| {
                ui.set_opacity(alpha);
                let frame = p.glass_frame().corner_radius(CornerRadius::same(14)).inner_margin(egui::Margin::symmetric(14, 11));
                let inner = frame.show(ui, |ui| {
                    ui.set_max_width(330.0);
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        ui.label(icon_text(icon, 20.0, tint));
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 3.0;
                            ui.add(egui::Label::new(egui::RichText::new(&title).font(FontId::new(14.0, fonts::semibold())).color(p.glass_text)).wrap());
                            if let Some(d) = &detail {
                                let color = if action.is_some() { p.accent_glass } else { p.glass_dim };
                                ui.add(egui::Label::new(egui::RichText::new(d).font(FontId::new(12.5, fonts::medium())).color(color)).wrap());
                            }
                        });
                    });
                });
                // Accent stripe on the left edge.
                let r = inner.response.rect;
                ui.painter().rect_filled(Rect::from_min_size(r.min + vec2(0.0, 12.0), vec2(3.0, r.height() - 24.0)), CornerRadius::same(2), fade(tint, 0.9));
                ui.interact(r, Id::unique(("toast-hit", id)), Sense::click())
            });
            y += out.response.rect.height() + 10.0;
            if out.inner.clicked() {
                dismissed.push(id);
                if let Some(ToastAction::Reveal(path)) = action {
                    revealed = Some(path);
                }
            }
        }
        if let Some(path) = revealed {
            files::reveal(&path);
        }
        if !dismissed.is_empty() {
            self.toasts.retain(|t| !dismissed.contains(&t.id));
        }
    }

    /// The playback statistics panel (`I`).
    pub fn draw_stats(&mut self, ctx: &Context, stage: Rect) {
        if !self.windows.stats {
            return;
        }
        let p = self.pal.clone();
        let s = &self.snap.stats;
        let mut rows: Vec<(&'static str, String)> = Vec::new();
        let state = match self.snap.state {
            PlayState::Idle => "idle",
            PlayState::Opening => "opening",
            PlayState::Buffering => "buffering",
            PlayState::Playing => "playing",
            PlayState::Paused => "paused",
            PlayState::Ended => "ended",
            PlayState::Failed => "failed",
        };
        rows.push((t(T::StatState), state.to_owned()));
        rows.push((t(T::StatPosition), format!("{} / {}", format_clock(self.snap.position), self.snap.duration.map_or("–".to_owned(), format_clock))));
        if let Some((w, h)) = self.snap.video_size {
            rows.push((t(T::StatVideo), format!("{w}×{h}")));
        }
        rows.push((t(T::StatDecoder), s.hw_decoder.map_or_else(|| t(T::StatSoftware).to_owned(), |d| format!("{} ({d})", t(T::StatHardware)))));
        if let Some(filter) = s.deinterlacer {
            rows.push((t(T::StatDeinterlacer), filter.to_owned()));
        }
        rows.push((t(T::StatFrames), format!("{} · {} {} · {} {}", s.decoded_frames, s.presented, t(T::StatShown), s.dropped_late + s.dropped_decoder, t(T::StatDropped))));
        rows.push((t(T::StatQueue), format!("{}", s.frame_queue)));
        if let Some(d) = s.av_diff_ms {
            rows.push((t(T::StatAvSync), localize_decimal(format!("{d:+.1} ms"))));
        }
        rows.push((
            t(T::StatAudio),
            format!(
                "{} · {:.0} ms · {:.0} ms · {} {}",
                if s.audio_virtual { t(T::StatVirtual).to_owned() } else { s.audio_device.clone() },
                s.audio_buffer_ms,
                s.audio_latency_ms,
                s.audio_underruns,
                t(T::StatUnderruns)
            ),
        ));
        rows.push((t(T::StatRead), format_bytes(s.bytes_read)));
        if let Some(m) = &self.media
            && let Some(b) = m.bit_rate
        {
            rows.push((t(T::StatBitrate), format_bitrate(b)));
        }
        rows.push((t(T::StatGpu), format!("{} ({})", self.gpu.adapter, self.gpu.backend)));
        rows.push((t(T::StatFrameTime), localize_decimal(format!("{:.1} ms", ctx.input(|i| i.unstable_dt) * 1000.0))));

        egui::Area::new(Id::unique("stats")).order(Order::Middle).interactable(false).fixed_pos(pos2(stage.left() + 16.0, stage.top() + 66.0)).show(ctx, |ui| {
            p.glass_frame().corner_radius(CornerRadius::same(14)).inner_margin(egui::Margin::symmetric(14, 11)).show(ui, |ui| {
                ui.set_max_width(440.0);
                ui.spacing_mut().item_spacing = vec2(10.0, 3.0);
                ui.label(egui::RichText::new(t(T::Statistics)).font(FontId::new(13.0, fonts::semibold())).color(p.accent_glass));
                ui.add_space(3.0);
                egui::Grid::new("stats-grid").num_columns(2).spacing(vec2(14.0, 3.0)).min_row_height(16.0).show(ui, |ui| {
                    for (k, v) in rows {
                        ui.label(egui::RichText::new(k).font(FontId::new(12.0, fonts::medium())).color(p.glass_dim));
                        ui.label(egui::RichText::new(v).font(FontId::new(12.0, fonts::mono())).color(p.glass_text));
                        ui.end_row();
                    }
                });
            });
        });
        // The panel shows live numbers: keep refreshing while it is open.
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }
}

/// The level bar below the volume message; `tick` marks the 100 % position when the scale goes beyond.
fn level_bar(ui: &mut egui::Ui, p: &Palette, value: f32, tick: Option<f32>) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 8.0), Sense::hover());
    let pill = CornerRadius::same(255);
    ui.painter().rect_filled(rect, pill, Color32::from_white_alpha(40));
    let v = value.clamp(0.0, 1.0);
    let boosted = tick.is_some_and(|t| value > t + 1e-3);
    let fill = if boosted { p.warn_glass() } else { p.accent_glass };
    if v > 0.0 {
        let r = Rect::from_min_size(rect.min, vec2((rect.width() * v).max(8.0), rect.height()));
        ui.painter().rect_filled(r, pill, fill);
    }
    if let Some(t) = tick {
        let x = rect.left() + rect.width() * t;
        ui.painter().rect_stroke(
            Rect::from_center_size(pos2(x, rect.center().y), vec2(2.0, 12.0)),
            CornerRadius::same(1),
            Stroke::new(1.0, Color32::from_white_alpha(130)),
            StrokeKind::Middle,
        );
    }
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

/// Bright variants of the status colours: toasts sit on dark glass whatever the theme.
trait GlassTints {
    fn success_glass(&self) -> Color32;
    fn warn_glass(&self) -> Color32;
    fn danger_glass(&self) -> Color32;
}

impl GlassTints for Palette {
    fn success_glass(&self) -> Color32 {
        Color32::from_rgb(52, 211, 153)
    }
    fn warn_glass(&self) -> Color32 {
        Color32::from_rgb(251, 191, 36)
    }
    fn danger_glass(&self) -> Color32 {
        Color32::from_rgb(251, 113, 133)
    }
}

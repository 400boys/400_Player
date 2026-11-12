//! The floating control dock: seek bar, transport, volume and the tool buttons.

use egui::{Align, Align2, Color32, Context, CornerRadius, FontId, Id, Layout, Order, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use media_player_400_core::time::format_clock_with;
use media_player_400_engine::{PlayState, SeekMode};

use crate::actions::Action;
use crate::app::MediaPlayer400App;
use crate::control::volume_icon;
use crate::i18n::{Key as T, t, t1};
use crate::state::{AbLoop, Page, SeekDrag, SubSource};
use crate::theme::{self, Palette, fade, fonts, mix};
use crate::ui::icons;
use crate::util::format_speed;
use crate::widgets::{IconButton, ThinSlider, play_button, tooltip};

const DOCK_MAX_WIDTH: f32 = 980.0;
const DOCK_MARGIN: f32 = 18.0;

impl MediaPlayer400App {
    pub fn draw_dock(&mut self, ctx: &Context, stage: Rect, controls: f32) {
        let width = (stage.width() - 2.0 * DOCK_MARGIN).clamp(300.0, DOCK_MAX_WIDTH);
        let compact = width < 640.0;
        let slide = (1.0 - controls) * 16.0;
        let anchor = pos2(stage.center().x - width * 0.5, stage.bottom() - DOCK_MARGIN + slide);
        let p = self.pal.clone();
        let mut actions: Vec<Action> = Vec::new();

        let out = egui::Area::new(Id::unique("dock")).order(Order::Middle).pivot(Align2::LEFT_BOTTOM).fixed_pos(anchor).interactable(controls > 0.4).show(ctx, |ui| {
            ui.set_opacity(controls);
            p.glass_frame().show(ui, |ui| {
                ui.set_width(width - 34.0);
                ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
                self.seek_row(ui, &p);
                ui.add_space(2.0);
                self.control_row(ui, &p, compact, &mut actions);
            });
        });
        self.dock_rect = Some(out.response.rect);
        self.dispatch(actions);
    }

    // =============================================================================================
    // Row 1: position, seek bar, duration
    // =============================================================================================

    fn seek_row(&mut self, ui: &mut Ui, p: &Palette) {
        let duration = self.snap.duration.filter(|d| *d > 0.0);
        let shown = self.seek_drag.map_or(self.snap.position, |d| d.target);
        let long = duration.is_some_and(|d| d >= 3600.0);
        let label_w = if long { 64.0 } else { 50.0 };
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            let mono = FontId::new(13.0, fonts::mono());
            match duration {
                Some(_) => {
                    let (r, _) = ui.allocate_exact_size(vec2(label_w, 24.0), Sense::hover());
                    ui.painter().text(r.right_center(), Align2::RIGHT_CENTER, format_clock_with(shown, long), mono.clone(), p.glass_text);
                }
                None => {
                    let (r, _) = ui.allocate_exact_size(vec2(label_w.max(66.0), 24.0), Sense::hover());
                    ui.painter().circle_filled(r.left_center() + vec2(8.0, 0.0), 4.0, p.danger);
                    ui.painter().text(r.left_center() + vec2(18.0, 0.0), Align2::LEFT_CENTER, t(T::Live), FontId::new(12.0, fonts::bold()), p.glass_text);
                }
            }
            let bar_w = (ui.available_width() - label_w - 12.0).max(40.0);
            self.seekbar(ui, p, bar_w);
            if let Some(d) = duration {
                let text = if self.settings.show_remaining_time { format!("−{}", format_clock_with((d - shown).max(0.0), long)) } else { format_clock_with(d, long) };
                let (r, resp) = ui.allocate_exact_size(vec2(label_w, 24.0), Sense::click());
                let hover = ui.ctx().animate_bool_with_time(resp.id.with("h"), resp.hovered(), 0.1);
                ui.painter().text(r.left_center(), Align2::LEFT_CENTER, text, mono, mix(p.glass_dim, p.glass_text, hover));
                let tip = if self.settings.show_remaining_time { T::Elapsed } else { T::Remaining };
                let shortcut = Action::ToggleRemaining.shortcut_label();
                let resp = resp.on_hover_ui(|ui| tooltip(ui, p, t(tip), shortcut.as_deref()));
                if resp.clicked() {
                    self.run(Action::ToggleRemaining);
                }
            }
        });
    }

    /// The seek bar: drag to scrub (fast seeks while moving, an exact one on release), hover for
    /// a preview, chapters as separate segments, the A–B section highlighted.
    fn seekbar(&mut self, ui: &mut Ui, p: &Palette, width: f32) {
        let height = 26.0;
        let (rect, resp) = ui.allocate_exact_size(vec2(width, height), Sense::click_and_drag());
        let duration = self.snap.duration.filter(|d| *d > 0.0);
        let seekable = duration.is_some() && self.player.is_seekable();
        let (left, right) = (rect.left() + 7.0, rect.right() - 7.0);
        let now = ui.ctx().input(|i| i.time);
        let to_time = |x: f32, d: f64| f64::from(((x - left) / (right - left)).clamp(0.0, 1.0)) * d;

        if let (true, Some(d)) = (seekable, duration) {
            if resp.is_pointer_button_down_on() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    let target = to_time(pos.x, d);
                    match &mut self.seek_drag {
                        None => {
                            self.seek_drag = Some(SeekDrag { target, last_sent: now, last_target_sent: target });
                            self.player.seek(target, SeekMode::Fast);
                        }
                        Some(sd) => {
                            sd.target = target;
                            // Scrubbing: a fast seek every 60 ms at most, only when the target moved.
                            if now - sd.last_sent > 0.06 && (target - sd.last_target_sent).abs() > 0.05 {
                                sd.last_sent = now;
                                sd.last_target_sent = target;
                                self.player.seek(target, SeekMode::Fast);
                            }
                        }
                    }
                }
            } else if let Some(sd) = self.seek_drag.take() {
                self.seek_to(sd.target, SeekMode::Accurate);
            }
        }

        if !ui.is_rect_visible(rect) {
            return;
        }
        let painter = ui.painter();
        let active = (resp.hovered() || self.seek_drag.is_some()) && seekable;
        let t = ui.ctx().animate_bool_with_time(resp.id.with("active"), active, 0.12);
        let cy = rect.center().y;
        let thick = 4.0 + 3.0 * t;
        let shown = self.seek_drag.map_or(self.snap.position, |d| d.target);
        let frac = duration.map_or(if self.snap.state == PlayState::Playing { 1.0 } else { 0.0 }, |d| (shown / d).clamp(0.0, 1.0) as f32);
        let fill_x = left + (right - left) * frac;
        let track_bg = Color32::from_white_alpha(44);
        let accent = p.accent_glass;
        let pill = CornerRadius::same(255);

        // Segments: one per chapter (a hairline gap between them), or the whole bar.
        let mut cuts: Vec<f32> = vec![left];
        if let (Some(m), Some(d)) = (&self.media, duration)
            && m.chapters.len() > 1
            && m.chapters.len() < 120
        {
            cuts.extend(m.chapters.iter().skip(1).map(|c| left + (right - left) * (c.start / d).clamp(0.0, 1.0) as f32));
        }
        cuts.push(right);
        for (i, w) in cuts.windows(2).enumerate() {
            let a = w[0] + if i > 0 { 1.5 } else { 0.0 };
            let b = w[1] - if i + 2 < cuts.len() { 1.5 } else { 0.0 };
            if b - a < 1.0 {
                continue;
            }
            let seg = Rect::from_min_max(pos2(a, cy - thick * 0.5), pos2(b, cy + thick * 0.5));
            painter.rect_filled(seg, pill, track_bg);
            if fill_x > a {
                let f = Rect::from_min_max(seg.min, pos2(fill_x.min(b), seg.max.y));
                painter.rect_filled(f, pill, accent);
            }
        }

        // The A–B section.
        let mark = |secs: f64| left + (right - left) * duration.map_or(0.0, |d| (secs / d).clamp(0.0, 1.0) as f32);
        match self.ab {
            AbLoop::Active(a, b) => {
                let band = Rect::from_min_max(pos2(mark(a), cy - thick * 0.5 - 2.0), pos2(mark(b), cy + thick * 0.5 + 2.0));
                painter.rect_filled(band, CornerRadius::same(3), fade(Color32::WHITE, 0.16));
                for x in [mark(a), mark(b)] {
                    painter.rect_filled(Rect::from_center_size(pos2(x, cy), vec2(3.0, thick + 9.0)), CornerRadius::same(1), Color32::WHITE);
                }
            }
            AbLoop::A(a) => {
                painter.rect_filled(Rect::from_center_size(pos2(mark(a), cy), vec2(3.0, thick + 9.0)), CornerRadius::same(1), Color32::WHITE);
            }
            AbLoop::Off => {}
        }

        // Knob.
        if seekable {
            let r = 4.5 + 3.0 * t;
            theme::paint_glow(painter, pos2(fill_x, cy), r * 3.2, fade(accent, 0.35 * t));
            painter.circle_filled(pos2(fill_x, cy + 1.0), r + 1.0, Color32::from_black_alpha(70));
            painter.circle_filled(pos2(fill_x, cy), r, Color32::WHITE);
        }

        // Preview under the pointer.
        if let (true, Some(d), Some(hp)) = (seekable, duration, resp.hover_pos().or_else(|| self.seek_drag.and_then(|_| resp.interact_pointer_pos()))) {
            let x = hp.x.clamp(left, right);
            painter.line_segment([pos2(x, cy - 9.0), pos2(x, cy + 9.0)], Stroke::new(1.5, fade(Color32::WHITE, 0.55)));
            self.draw_preview(ui.ctx(), p, rect, x, to_time(x, d));
        }
    }

    /// The bubble above the seek bar: preview picture, chapter, time.
    fn draw_preview(&mut self, ctx: &Context, p: &Palette, bar: Rect, x: f32, time: f64) {
        let painter = ctx.layer_painter(egui::LayerId::new(Order::Tooltip, Id::unique("seek-preview")));
        let thumb = self.player.thumbnail(time);
        let long = self.snap.duration.is_some_and(|d| d >= 3600.0);
        let time_text = format_clock_with(time, long);
        let chapter = self.media.as_ref().and_then(|m| m.chapter_at(time).and_then(|i| m.chapters.get(i)).map(|c| c.title.clone())).filter(|s| !s.trim().is_empty());

        let font = FontId::new(13.0, fonts::mono());
        let time_galley = painter.layout_no_wrap(time_text, font, p.glass_text);
        let chap_galley = chapter.map(|c| painter.layout_no_wrap(crate::util::ellipsize_middle(&c, 34), FontId::new(12.5, fonts::medium()), p.glass_dim));
        let tex = thumb.as_ref().map(|t| (self.thumbs.texture(ctx, t), t.image.width as f32, t.image.height as f32));

        let img_size = tex.map(|(_, w, h)| {
            let s = (176.0 / w).min(120.0 / h).min(1.0f32.max(176.0 / w));
            vec2((w * s).round(), (h * s).round())
        });
        let text_w = time_galley.size().x.max(chap_galley.as_ref().map_or(0.0, |g| g.size().x));
        let inner_w = img_size.map_or(0.0, |s| s.x).max(text_w + 4.0);
        let text_h = time_galley.size().y + chap_galley.as_ref().map_or(0.0, |g| g.size().y + 2.0);
        let pad = 7.0;
        let total = vec2(inner_w + 2.0 * pad, img_size.map_or(0.0, |s| s.y + 6.0) + text_h + 2.0 * pad);
        let screen = ctx.content_rect();
        let cx = x.clamp(screen.left() + 8.0 + total.x * 0.5, screen.right() - 8.0 - total.x * 0.5);
        let bubble = Rect::from_min_size(pos2(cx - total.x * 0.5, bar.top() - 14.0 - total.y), total);

        painter.add(p.popup_shadow().as_shape(bubble, CornerRadius::same(12)));
        painter.rect_filled(bubble, CornerRadius::same(12), p.glass.gamma_multiply(1.15).to_opaque());
        painter.rect_stroke(bubble, CornerRadius::same(12), Stroke::new(1.0, p.glass_border), egui::StrokeKind::Inside);
        let mut y = bubble.top() + pad;
        if let (Some((id, ..)), Some(size)) = (tex, img_size) {
            let r = Rect::from_min_size(pos2(bubble.center().x - size.x * 0.5, y), size);
            painter.add(egui::epaint::RectShape::filled(r, CornerRadius::same(7), Color32::WHITE).with_texture(id, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0))));
            y += size.y + 6.0;
        }
        if let Some(g) = chap_galley {
            let h = g.size().y;
            painter.galley(pos2(bubble.center().x - g.size().x * 0.5, y), g, p.glass_dim);
            y += h + 2.0;
        }
        painter.galley(pos2(bubble.center().x - time_galley.size().x * 0.5, y), time_galley, p.glass_text);
    }

    // =============================================================================================
    // Row 2: transport and tools
    // =============================================================================================

    fn control_row(&mut self, ui: &mut Ui, p: &Palette, compact: bool, actions: &mut Vec<Action>) {
        let state = self.snap.state;
        let playing = matches!(state, PlayState::Playing | PlayState::Opening | PlayState::Buffering) && self.is_wanted_playing();
        let busy = matches!(state, PlayState::Opening | PlayState::Buffering);
        let has_audio = self.media.as_ref().is_some_and(|m| !m.audio.is_empty());
        let has_video = self.media.as_ref().is_some_and(|m| m.has_video());
        let multi = self.playlist.len() > 1;

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.set_min_height(54.0);

            // --- transport ------------------------------------------------------------------------
            let prev_tip = t(T::Previous);
            if IconButton::new(icons::SKIP_BACK)
                .size(40.0)
                .glass()
                .filled_glyph()
                .enabled(multi || self.snap.position > 3.0)
                .tip(prev_tip)
                .shortcut(Action::Previous.shortcut_label())
                .show(ui, p)
                .clicked()
            {
                actions.push(Action::Previous);
            }
            let tip = if playing { t(T::Pause) } else { t(T::Play) };
            if play_button(ui, p, playing, busy, 54.0, tip, Action::PlayPause.shortcut_label()).clicked() {
                actions.push(Action::PlayPause);
            }
            if IconButton::new(icons::SKIP_FORWARD).size(40.0).glass().filled_glyph().enabled(multi).tip(t(T::Next)).shortcut(Action::Next.shortcut_label()).show(ui, p).clicked() {
                actions.push(Action::Next);
            }
            ui.add_space(8.0);

            // --- volume ---------------------------------------------------------------------------
            let vol_icon = volume_icon(self.settings.volume, self.settings.muted);
            let tip = if self.settings.muted { t(T::Unmute) } else { t(T::Mute) };
            if IconButton::new(vol_icon).size(38.0).glass().tip(tip).shortcut(Action::ToggleMute.shortcut_label()).show(ui, p).clicked() {
                actions.push(Action::ToggleMute);
            }
            if !compact {
                let mut v = if self.settings.muted { 0.0 } else { self.settings.volume };
                let pct = (v * 100.0).round() as i32;
                let r = ThinSlider::new(&mut v, 0.0..=2.0).length(96.0).glass().marks(&[1.0]).warn_above(1.0).detent(1.0).step(0.01).show(ui, p);
                let r = r.on_hover_ui(|ui| {
                    ui.label(RichText::new(t1(T::VolumeValue, pct)).color(p.text));
                });
                if r.changed() {
                    self.set_volume(v, false);
                }
                if r.dragged() || r.hovered() {
                    let label = format!("{pct}%");
                    ui.add_sized([40.0, 20.0], egui::Label::new(RichText::new(label).font(FontId::new(12.0, fonts::mono())).color(p.glass_dim)));
                }
            }

            // --- tools (right aligned, added from the right) --------------------------------------------
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let more = IconButton::new(icons::DOTS_THREE).size(38.0).glass().bold_glyph().tip(t(T::More)).show(ui, p);
                if more.clicked() && !self.swallow_click {
                    let at = pos2(more.rect.right(), more.rect.top() - 12.0);
                    self.open_menu(at, true, Page::Root);
                }
                let fs_icon = if self.fullscreen { icons::ARROWS_IN } else { icons::ARROWS_OUT };
                let fs_tip = if self.fullscreen { t(T::ExitFullscreen) } else { t(T::Fullscreen) };
                if IconButton::new(fs_icon).size(38.0).glass().tip(fs_tip).shortcut(Action::ToggleFullscreen.shortcut_label()).show(ui, p).clicked() {
                    actions.push(Action::ToggleFullscreen);
                }
                if IconButton::new(icons::PLAYLIST)
                    .size(38.0)
                    .glass()
                    .active(self.windows.playlist)
                    .tip(t(T::Playlist))
                    .shortcut(Action::TogglePlaylist.shortcut_label())
                    .show(ui, p)
                    .clicked()
                {
                    actions.push(Action::TogglePlaylist);
                }
                if !compact && has_video {
                    let r = IconButton::new(icons::SLIDERS).size(38.0).glass().active(!self.view.adjust.is_identity() || !self.view.is_neutral()).tip(t(T::Picture)).show(ui, p);
                    if r.clicked() && !self.swallow_click {
                        self.open_menu(pos2(r.rect.center().x, r.rect.top() - 12.0), true, Page::Video);
                    }
                }
                if !compact && has_audio {
                    let audio_fx = self.audio_delay.abs() > 1e-3 || self.settings.night_mode || self.settings.eq.enabled;
                    let r = IconButton::new(icons::HEADPHONES)
                        .size(38.0)
                        .glass()
                        .active(audio_fx)
                        .tip(t(T::AudioTrack))
                        .shortcut(Action::CycleAudio.shortcut_label())
                        .show(ui, p);
                    if r.clicked() && !self.swallow_click {
                        self.open_menu(pos2(r.rect.center().x, r.rect.top() - 12.0), true, Page::Audio);
                    }
                }
                if has_video || self.ext_sub.is_some() || self.media.as_ref().is_some_and(|m| !m.subtitles.is_empty()) {
                    let r = IconButton::new(icons::SUBTITLES)
                        .size(38.0)
                        .glass()
                        .active(self.sub_source != SubSource::Off)
                        .tip(t(T::Subtitles))
                        .shortcut(Action::CycleSubtitles.shortcut_label())
                        .show(ui, p);
                    if r.clicked() && !self.swallow_click {
                        self.open_menu(pos2(r.rect.center().x, r.rect.top() - 12.0), true, Page::Subtitles);
                    }
                }
                if !compact {
                    let speed = self.snap.speed.max(0.25);
                    let r = glass_chip(ui, p, &format!("{}×", format_speed(speed)), (speed - 1.0).abs() > 1e-3);
                    let r = r.on_hover_ui(|ui| tooltip(ui, p, &t1(T::SpeedValue, format_speed(speed)), None));
                    if r.clicked() && !self.swallow_click {
                        self.open_menu(pos2(r.rect.center().x, r.rect.top() - 12.0), true, Page::Speed);
                    }
                }
            });
        });
    }

    /// `true` when the user wants playback to run (the engine may still be opening).
    fn is_wanted_playing(&self) -> bool {
        !matches!(self.snap.state, PlayState::Paused | PlayState::Ended | PlayState::Idle | PlayState::Failed)
    }
}

/// A small pill button for the dock (the speed).
fn glass_chip(ui: &mut Ui, p: &Palette, text: &str, highlighted: bool) -> egui::Response {
    let font = FontId::new(12.5, fonts::mono());
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, Color32::PLACEHOLDER);
    let size = vec2((galley.size().x + 20.0).max(46.0), 28.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let hover = ui.ctx().animate_bool_with_time(resp.id.with("h"), resp.hovered(), 0.1);
    let cr = CornerRadius::same(255);
    let bg = if highlighted { fade(p.accent_glass, 0.22 + 0.12 * hover) } else { Color32::from_white_alpha((16.0 + 22.0 * hover) as u8) };
    ui.painter().rect_filled(rect, cr, bg);
    let color = if highlighted { p.accent_glass } else { mix(p.glass_dim, p.glass_text, hover) };
    ui.painter().galley(rect.center() - galley.size() * 0.5, galley, color);
    resp
}

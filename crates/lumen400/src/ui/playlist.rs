//! The playlist drawer: slides in from the right edge and pushes the picture aside.

use egui::{Align, Align2, Color32, Context, CornerRadius, FontId, Id, Layout, Order, Rect, Sense, Stroke, Ui, UiBuilder, pos2, vec2};
use lumen400_core::playlist::{ItemId, RepeatMode};
use lumen400_core::time::{format_clock, format_duration_long};

use crate::actions::Action;
use crate::app::LumenApp;
use crate::files;
use crate::i18n::{Key as T, t, t1, tf};
use crate::state::Dialog;
use crate::theme::{Palette, fade, fonts, mix};
use crate::ui::{DRAWER_WIDTH, icons};
use crate::util::{ellipsize_middle, file_name_of};
use crate::widgets::{IconButton, Lead, button, menu_item};

const ROW_HEIGHT: f32 = 58.0;

/// What a row of the list asked for.
enum RowCmd {
    Play(ItemId),
    Remove(ItemId),
    Reveal(std::path::PathBuf),
    Copy(String),
}

impl LumenApp {
    /// `width` is the animated visible width (`0` when closed).
    pub fn draw_drawer(&mut self, ctx: &Context, full: Rect, width: f32) {
        if width < 1.0 {
            return;
        }
        let p = self.pal.clone();
        let target = DRAWER_WIDTH.min(full.width() * 0.62);
        let visible = Rect::from_min_max(pos2(full.right() - width, full.top()), full.right_bottom());
        // The content keeps its final width while the drawer slides, so it never reflows.
        let content = Rect::from_min_size(visible.min, vec2(target, full.height()));
        let mut cmds: Vec<RowCmd> = Vec::new();
        let mut actions: Vec<Action> = Vec::new();

        egui::Area::new(Id::unique("drawer")).order(Order::Middle).fixed_pos(visible.min).show(ctx, |ui| {
            ui.set_clip_rect(visible);
            // The whole drawer swallows pointer input (a click between two rows must not pause the video).
            ui.allocate_rect(visible, Sense::click_and_drag());
            ui.painter().rect_filled(visible, 0.0, p.panel);
            ui.painter().line_segment([visible.left_top(), visible.left_bottom()], Stroke::new(1.0, p.border_strong));
            let mut inner = ui.new_child(UiBuilder::new().max_rect(content.shrink2(vec2(0.0, 0.0))).layout(Layout::top_down(Align::Min)));
            inner.set_clip_rect(visible);
            self.drawer_body(&mut inner, &p, &mut cmds, &mut actions);
        });

        for cmd in cmds {
            match cmd {
                RowCmd::Play(id) => self.play_item(id),
                RowCmd::Remove(id) => {
                    let was_current = self.playlist.current_id() == Some(id);
                    self.playlist.remove(id);
                    if was_current && self.source.is_some() {
                        // The item on screen stays playing; it just is not in the list any more.
                    }
                }
                RowCmd::Reveal(path) => files::reveal(&path),
                RowCmd::Copy(text) => {
                    self.ctx.copy_text(text);
                    self.osd(icons::COPY, t(T::CopiedToClipboard), None);
                }
            }
        }
        self.dispatch(actions);
    }

    fn drawer_body(&mut self, ui: &mut Ui, p: &Palette, cmds: &mut Vec<RowCmd>, actions: &mut Vec<Action>) {
        let pad = 18.0;
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);

        // --- header ------------------------------------------------------------------------------------
        ui.add_space(16.0);
        ui.horizontal(|ui| {
            ui.add_space(pad);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(egui::RichText::new(t(T::Playlist)).font(FontId::new(19.0, fonts::semibold())).color(p.text));
                ui.label(egui::RichText::new(self.playlist_summary()).font(FontId::new(12.5, fonts::medium())).color(p.text_faint));
            });
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                ui.add_space(pad - 6.0);
                if IconButton::new(icons::X).size(32.0).tip(t(T::Close)).show(ui, p).clicked() {
                    actions.push(Action::TogglePlaylist);
                }
            });
        });
        ui.add_space(12.0);

        // --- toolbar ------------------------------------------------------------------------------------
        ui.horizontal(|ui| {
            ui.add_space(pad - 4.0);
            ui.spacing_mut().item_spacing.x = 2.0;
            let last_dir = self.settings.last_dir.clone();
            if IconButton::new(icons::PLUS).size(34.0).tip(t(T::AddFiles)).shortcut(Action::AddFiles.shortcut_label()).show(ui, p).clicked() {
                self.dialogs.open_files(last_dir.clone(), true);
            }
            if IconButton::new(icons::FOLDER_OPEN).size(34.0).tip(t(T::AddFolder)).show(ui, p).clicked() {
                self.dialogs.open_folder(last_dir, true);
            }
            if IconButton::new(icons::LINK).size(34.0).tip(t(T::AddUrl)).show(ui, p).clicked() {
                self.windows.dialog = Some(Dialog::OpenUrl { text: String::new(), append: true, first_frame: true });
            }
            ui.add_space(8.0);
            let shuffle = self.playlist.shuffle();
            if IconButton::new(icons::SHUFFLE).size(34.0).active(shuffle).tip(t(T::Shuffle)).shortcut(Action::ToggleShuffle.shortcut_label()).show(ui, p).clicked() {
                actions.push(Action::ToggleShuffle);
            }
            let (repeat_icon, repeat_tip) = match self.playlist.repeat() {
                RepeatMode::Off => (icons::REPEAT, T::RepeatOff),
                RepeatMode::All => (icons::REPEAT, T::RepeatAll),
                RepeatMode::One => (icons::REPEAT_ONCE, T::RepeatOne),
            };
            if IconButton::new(repeat_icon)
                .size(34.0)
                .active(self.playlist.repeat() != RepeatMode::Off)
                .tip(t(repeat_tip))
                .shortcut(Action::ToggleRepeat.shortcut_label())
                .show(ui, p)
                .clicked()
            {
                actions.push(Action::ToggleRepeat);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(pad - 4.0);
                let empty = self.playlist.is_empty();
                if IconButton::new(icons::TRASH).size(34.0).enabled(!empty).tip(t(T::ClearPlaylist)).show(ui, p).clicked() {
                    self.playlist.clear();
                }
                if IconButton::new(icons::FLOPPY).size(34.0).enabled(!empty).tip(t(T::SavePlaylist)).show(ui, p).clicked() {
                    self.dialogs.save_playlist(self.settings.last_dir.clone());
                }
                if IconButton::new(icons::LIST_BULLETS).size(34.0).enabled(self.playlist.len() > 1).tip(t(T::SortByName)).show(ui, p).clicked() {
                    self.playlist.sort_by_name();
                }
            });
        });
        if self.playlist.len() > 1 || !self.playlist_filter.is_empty() {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.add_space(pad);
                let w = (ui.available_width() - pad).max(80.0);
                egui::Frame::NONE
                    .fill(p.sunken)
                    .stroke(Stroke::new(1.0, p.border))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(egui::Margin::symmetric(10, 5))
                    .show(ui, |ui| {
                        ui.set_width((w - 22.0).max(60.0));
                        ui.spacing_mut().item_spacing.x = 6.0;
                        ui.label(crate::widgets::icon_text(icons::SEARCH, 15.0, p.text_faint));
                        let edit_w = (ui.available_width() - if self.playlist_filter.is_empty() { 4.0 } else { 26.0 }).max(40.0);
                        ui.add(
                            egui::TextEdit::singleline(&mut self.playlist_filter)
                                .frame(egui::Frame::NONE)
                                .hint_text(egui::RichText::new(t(T::PaletteHint)).size(12.5).color(p.text_faint))
                                .desired_width(edit_w),
                        );
                        if !self.playlist_filter.is_empty() && IconButton::new(icons::X).size(20.0).tip(t(T::Reset)).show(ui, p).clicked() {
                            self.playlist_filter.clear();
                        }
                    });
            });
        }
        ui.add_space(10.0);
        let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
        ui.painter().rect_filled(line, 0.0, p.border);

        // --- the list -----------------------------------------------------------------------------------
        if self.playlist.is_empty() {
            self.drawer_empty(ui, p);
            return;
        }
        self.drawer_list(ui, p, cmds);
    }

    fn drawer_empty(&mut self, ui: &mut Ui, p: &Palette) {
        let avail = ui.available_rect_before_wrap();
        let c = avail.center() - vec2(0.0, 30.0);
        ui.painter().text(c - vec2(0.0, 34.0), Align2::CENTER_CENTER, icons::QUEUE.to_string(), FontId::new(46.0, fonts::icons()), fade(p.accent, 0.85));
        ui.painter().text(c + vec2(0.0, 12.0), Align2::CENTER_CENTER, t(T::PlaylistEmpty), FontId::new(16.0, fonts::semibold()), p.text);
        ui.painter().text(c + vec2(0.0, 36.0), Align2::CENTER_CENTER, t(T::PlaylistEmptyHint), FontId::new(13.0, fonts::medium()), p.text_faint);
        let label = t(T::AddFiles);
        let w = crate::widgets::button_width(ui, label, true);
        let r = Rect::from_center_size(c + vec2(0.0, 84.0), vec2(w, 38.0));
        let mut child = ui.new_child(UiBuilder::new().max_rect(r));
        if button(&mut child, p, Some(icons::PLUS), label, true).clicked() {
            let dir = self.settings.last_dir.clone();
            self.dialogs.open_files(dir, true);
        }
    }

    fn drawer_list(&mut self, ui: &mut Ui, p: &Palette, cmds: &mut Vec<RowCmd>) {
        let now = ui.ctx().input(|i| i.time);
        let current = self.playlist.current_id();
        let playing = self.is_playing();
        let list_rect = ui.available_rect_before_wrap();
        let (pointer, down, released) = ui.ctx().input(|i| (i.pointer.latest_pos(), i.pointer.any_down(), i.pointer.any_released()));

        let mut area = egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt("playlist-scroll");
        if self.playlist_scroll_to_current {
            if let Some(idx) = self.playlist.current_index() {
                let offset = (idx as f32 * ROW_HEIGHT - list_rect.height() * 0.4).max(0.0);
                area = area.vertical_scroll_offset(offset);
            }
            self.playlist_scroll_to_current = false;
        }

        let mut drop_target: Option<usize> = None;
        let drag_from = self.playlist_drag;
        let mut started_drag: Option<usize> = None;
        let q_lower = self.playlist_filter.trim().to_lowercase();
        let filtered: Vec<(usize, _)> = self
            .playlist
            .items()
            .iter()
            .cloned()
            .enumerate()
            .filter(|(_, it)| q_lower.is_empty() || it.display_name().to_lowercase().contains(&q_lower) || it.source.to_lowercase().contains(&q_lower))
            .collect();
        let can_drag = q_lower.is_empty();
        area.show_rows(ui, ROW_HEIGHT, filtered.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for row_i in range {
                let (idx, ref item) = filtered[row_i];
                let is_current = current == Some(item.id);
                let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click_and_drag());
                if !ui.is_rect_visible(rect) {
                    continue;
                }
                let hover = ui.ctx().animate_bool_with_time(resp.id.with("h"), resp.hovered() && drag_from.is_none(), 0.08);
                let inner = rect.shrink2(vec2(10.0, 3.0));
                let dragging_this = drag_from == Some(idx);
                let cr = CornerRadius::same(10);
                if is_current {
                    ui.painter().rect_filled(inner, cr, p.accent_faint);
                    ui.painter().rect_filled(Rect::from_min_size(inner.min + vec2(0.0, 12.0), vec2(3.0, inner.height() - 24.0)), CornerRadius::same(2), p.accent);
                } else if hover > 0.0 {
                    ui.painter().rect_filled(inner, cr, fade(p.hover, hover));
                }
                if dragging_this {
                    ui.painter().rect_stroke(inner, cr, Stroke::new(1.5, p.accent), egui::StrokeKind::Inside);
                }

                // Index / now-playing marker.
                let marker = pos2(inner.left() + 24.0, inner.center().y);
                if is_current {
                    paint_bars(ui, marker, p.accent, playing, now);
                } else {
                    ui.painter().text(marker, Align2::CENTER_CENTER, format!("{}", idx + 1), FontId::new(12.5, fonts::mono()), p.text_faint);
                }

                // Texts.
                let text_left = inner.left() + 48.0;
                let remove_zone = if resp.hovered() || is_current { 40.0 } else { 12.0 };
                let max_chars = (((inner.right() - remove_zone - text_left) / 7.4) as usize).max(8);
                let title = ellipsize_middle(&item.display_name(), max_chars);
                let title_color = if is_current { mix(p.accent, p.text, if p.dark { 0.45 } else { 0.0 }) } else { p.text };
                ui.painter().text(pos2(text_left, inner.center().y - 9.0), Align2::LEFT_CENTER, title, FontId::new(14.0, fonts::medium()), title_color);
                let mut sub: Vec<String> = Vec::new();
                if let Some(d) = item.duration {
                    sub.push(if d >= 3600.0 { format_duration_long(d) } else { format_clock(d) });
                }
                if item.is_remote() {
                    sub.push(host_of(&item.source));
                } else if let Some(parent) = item.local_path().and_then(|pth| pth.parent().map(|d| file_name_of(&d.to_string_lossy())))
                    && !parent.is_empty()
                {
                    sub.push(parent);
                }
                if !sub.is_empty() {
                    let text = ellipsize_middle(&sub.join("  ·  "), max_chars + 4);
                    ui.painter().text(pos2(text_left, inner.center().y + 11.0), Align2::LEFT_CENTER, text, FontId::new(12.0, fonts::medium()), p.text_faint);
                }

                // Remove button, only while the row is hot.
                if resp.hovered() && drag_from.is_none() {
                    let br = Rect::from_center_size(pos2(inner.right() - 20.0, inner.center().y), vec2(28.0, 28.0));
                    let br_resp = ui.interact(br, Id::unique(("pl-remove", item.id)), Sense::click());
                    let bh = ui.ctx().animate_bool_with_time(br_resp.id.with("h"), br_resp.hovered(), 0.08);
                    ui.painter().rect_filled(br, CornerRadius::same(8), fade(p.hover, bh));
                    ui.painter().text(br.center(), Align2::CENTER_CENTER, icons::X.to_string(), FontId::new(14.0, fonts::icons()), mix(p.text_faint, p.text, bh));
                    if br_resp.clicked() {
                        cmds.push(RowCmd::Remove(item.id));
                        continue;
                    }
                }

                if resp.clicked() {
                    cmds.push(RowCmd::Play(item.id));
                }
                resp.context_menu(|ui| {
                    ui.set_min_width(210.0);
                    if menu_item(ui, p, Lead::Icon(icons::PLAY), t(T::PlayNow), None, true).clicked() {
                        cmds.push(RowCmd::Play(item.id));
                        ui.close();
                    }
                    if let Some(path) = item.local_path().filter(|pth| pth.exists())
                        && menu_item(ui, p, Lead::Icon(icons::FOLDER_OPEN), t(T::ShowInFolder), None, true).clicked()
                    {
                        cmds.push(RowCmd::Reveal(path));
                        ui.close();
                    }
                    if menu_item(ui, p, Lead::Icon(icons::COPY), t(T::CopyPath), None, true).clicked() {
                        cmds.push(RowCmd::Copy(item.source.clone()));
                        ui.close();
                    }
                    if menu_item(ui, p, Lead::Icon(icons::X), t(T::RemoveItem), None, true).clicked() {
                        cmds.push(RowCmd::Remove(item.id));
                        ui.close();
                    }
                });

                if can_drag && resp.drag_started() {
                    started_drag = Some(idx);
                }
                if drag_from.is_some()
                    && down
                    && let Some(pt) = pointer
                    && rect.contains(pt)
                {
                    drop_target = Some(idx);
                    let y = if drag_from.is_some_and(|f| f < idx) { rect.bottom() } else { rect.top() };
                    ui.painter().line_segment([pos2(rect.left() + 12.0, y), pos2(rect.right() - 12.0, y)], Stroke::new(2.5, p.accent));
                }
            }
            // Dragging near the top or bottom edge scrolls the list.
            if let (Some(_), true, Some(pt)) = (drag_from, down, pointer) {
                let edge = 36.0;
                let speed = 12.0;
                if pt.y < list_rect.top() + edge {
                    ui.scroll_with_delta(vec2(0.0, speed));
                } else if pt.y > list_rect.bottom() - edge {
                    ui.scroll_with_delta(vec2(0.0, -speed));
                }
                ui.ctx().request_repaint();
            }
        });

        // Dragging state lives outside the closure (which only borrows what it needs).
        if let Some(from) = started_drag {
            self.playlist_drag = Some(from);
        }
        if let Some(from) = self.playlist_drag {
            if drag_from.is_some() && ui.ctx().input(|i| i.pointer.primary_down()) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            }
            if released || !down {
                if let Some(to) = drop_target {
                    self.playlist.move_item(from, to);
                }
                self.playlist_drag = None;
            }
        }
    }

    fn playlist_summary(&self) -> String {
        let n = self.playlist.len();
        let total: f64 = self.playlist.items().iter().filter_map(|i| i.duration).sum();
        match (n, total > 0.0) {
            (0, _) => t(T::PlaylistEmpty).to_owned(),
            (1, false) => t(T::PlaylistCountOne).to_owned(),
            (_, false) => t1(T::PlaylistCount, n),
            (_, true) => tf(T::PlaylistTotal, &[&n, &format_duration_long(total)]),
        }
    }
}

/// Three little bars that dance while the item plays.
fn paint_bars(ui: &Ui, center: egui::Pos2, color: Color32, playing: bool, time: f64) {
    let w = 3.0;
    let gap = 2.5;
    let max_h = 14.0;
    for i in 0..3 {
        let phase = time * (3.1 + 0.9 * i as f64) + i as f64 * 1.7;
        let k = if playing { 0.35 + 0.65 * (0.5 + 0.5 * phase.sin()) as f32 } else { 0.35 };
        let h = max_h * k;
        let x = center.x + (i as f32 - 1.0) * (w + gap);
        ui.painter().rect_filled(Rect::from_min_max(pos2(x - w * 0.5, center.y + max_h * 0.5 - h), pos2(x + w * 0.5, center.y + max_h * 0.5)), CornerRadius::same(1), color);
    }
}

/// `https://example.com/a/b.mp4` → `example.com`.
fn host_of(url: &str) -> String {
    let rest = url.split("://").nth(1).unwrap_or(url);
    rest.split(['/', '?', '#']).next().unwrap_or(rest).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts() {
        assert_eq!(host_of("https://example.com/a/b.mp4?x=1"), "example.com");
        assert_eq!(host_of("rtsp://cam.local:554/stream"), "cam.local:554");
        assert_eq!(host_of("plain"), "plain");
    }
}

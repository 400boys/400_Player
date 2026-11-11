//! The settings window: six tabs, every change applied immediately and saved shortly after.

use egui::{Align, Align2, Color32, Context, CornerRadius, FontId, Id, Layout, RichText, Sense, Stroke, StrokeKind, TextEdit, Ui, pos2, vec2};
use lumen400_core::playlist::RepeatMode;
use lumen400_core::settings::{Accent, Deinterlace, HwDecode, Language, Settings, ThemeMode};
use lumen400_core::subtitle::Cue;

use crate::app::LumenApp;
use crate::files;
use crate::i18n::{Key as T, t, t1};
use crate::state::SettingsTab;
use crate::theme::{self, Palette, fade, fonts, mix};
use crate::ui::palette::accent_name;
use crate::ui::{Modal, icons};
use crate::util::ellipsize_middle;
use crate::widgets::{self, ThinSlider, button, section_title, segmented, setting_row, toggle_switch};

/// Things a tab asked for that are not plain setting changes.
#[derive(Default)]
struct Requests {
    choose_screenshot_dir: bool,
    open_screenshot_dir: bool,
    open_config_dir: bool,
    clear_history: bool,
    reset_settings: bool,
    refresh_devices: bool,
    open_equalizer: bool,
}

impl LumenApp {
    pub fn draw_settings(&mut self, ctx: &Context) {
        let Some(tab) = self.windows.settings else { return };
        let p = self.pal.clone();
        let before = self.settings.clone();
        let mut s = before.clone();
        let mut tab_now = tab;
        let mut req = Requests::default();
        let screen = ctx.content_rect();
        let height = (screen.height() - 160.0).clamp(300.0, 560.0);
        let compact = screen.width() < 700.0;

        if tab == SettingsTab::Audio && !ctx.data(|d| d.get_temp::<bool>(Id::unique("devices-loaded"))).unwrap_or(false) {
            self.devices = lumen400_engine::list_output_devices();
            ctx.data_mut(|d| d.insert_temp(Id::unique("devices-loaded"), true));
        }
        let view = TabView { gpu: &self.gpu, devices: &self.devices, virtual_audio: self.virtual_audio, history_len: self.history.entries().len(), ephemeral: self.ephemeral };

        let close = Modal::new("settings", t(T::Settings), icons::GEAR, 880.0).show(ctx, &p, |ui| {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                if !compact {
                    ui.allocate_ui_with_layout(vec2(172.0, height), Layout::top_down(Align::Min), |ui| nav(ui, &p, &mut tab_now));
                    ui.add_space(8.0);
                    let (line, _) = ui.allocate_exact_size(vec2(1.0, height), Sense::hover());
                    ui.painter().rect_filled(line, 0.0, p.border);
                    ui.add_space(18.0);
                }
                ui.allocate_ui_with_layout(vec2(ui.available_width(), height), Layout::top_down(Align::Min), |ui| {
                    if compact {
                        // Narrow window: the tabs become a row of chips above the content.
                        ui.horizontal_wrapped(|ui| {
                            for t_ in SettingsTab::ALL {
                                if widgets::chip(ui, &p, tab_label(t_), tab_now == t_).clicked() {
                                    tab_now = t_;
                                }
                            }
                        });
                        ui.add_space(8.0);
                    }
                    egui::ScrollArea::vertical().id_salt(("settings-scroll", tab_now as u8)).auto_shrink([false, false]).show(ui, |ui| {
                        // Keeps the controls clear of the floating scroll bar.
                        egui::Frame::NONE.inner_margin(egui::Margin { right: 16, ..Default::default() }).show(ui, |ui| {
                            ui.add_space(2.0);
                            match tab_now {
                                SettingsTab::Appearance => appearance(ui, &p, &mut s),
                                SettingsTab::Playback => playback(ui, &p, &mut s, &view),
                                SettingsTab::Audio => audio(ui, &p, &mut s, &view, &mut req),
                                SettingsTab::Subtitles => subtitles(ui, &p, &mut s),
                                SettingsTab::Advanced => advanced(ui, &p, &mut s, &view, &mut req),
                                SettingsTab::About => about(ui, &p, &view),
                            }
                            ui.add_space(8.0);
                        });
                    });
                });
            });
        });

        // --- apply what changed -----------------------------------------------------------------------------------
        if tab_now != tab {
            self.windows.settings = Some(tab_now);
        }
        if s != before {
            let look = s.theme != before.theme || s.accent != before.accent || s.language != before.language || s.ui_scale != before.ui_scale;
            let audio = s.volume != before.volume || s.muted != before.muted || s.eq != before.eq || s.night_mode != before.night_mode;
            let engine = s.hw_decode != before.hw_decode
                || s.audio_device != before.audio_device
                || s.preferred_audio_langs != before.preferred_audio_langs
                || s.preferred_sub_langs != before.preferred_sub_langs
                || s.generate_thumbnails != before.generate_thumbnails;
            let deinterlace = s.deinterlace != before.deinterlace;
            self.settings = s;
            if look {
                self.apply_look();
            }
            if audio {
                self.apply_audio();
            }
            if engine {
                self.apply_engine_config();
            }
            if deinterlace {
                self.apply_deinterlace();
            }
            self.playlist.set_repeat(self.settings.repeat);
            self.playlist.set_shuffle(self.settings.shuffle);
            self.touch_settings();
        }

        if req.refresh_devices {
            self.devices = lumen400_engine::list_output_devices();
        }
        if req.choose_screenshot_dir {
            let start = Some(self.settings.screenshot_directory());
            self.dialogs.pick_screenshot_folder(start);
        }
        if req.open_screenshot_dir {
            let dir = self.settings.screenshot_directory();
            let _ = std::fs::create_dir_all(&dir);
            files::open_dir(&dir);
        }
        if req.open_config_dir
            && let Some(dir) = lumen400_core::settings::config_dir()
        {
            let _ = std::fs::create_dir_all(&dir);
            files::open_dir(&dir);
        }
        if req.clear_history {
            self.history.clear();
            self.save_history_now();
            self.toast(crate::state::ToastKind::Success, t(T::HistoryCleared), None);
        }
        if req.reset_settings {
            let window = self.settings.window.clone();
            self.settings = Settings { window, ..Settings::default() };
            self.playlist.set_repeat(self.settings.repeat);
            self.playlist.set_shuffle(self.settings.shuffle);
            self.apply_look();
            self.apply_audio();
            self.apply_engine_config();
            self.apply_deinterlace();
            self.touch_settings();
            self.toast(crate::state::ToastKind::Success, t(T::SettingsReset), None);
        }
        if req.open_equalizer {
            self.windows.equalizer = true;
        }
        if close {
            self.windows.settings = None;
            ctx.data_mut(|d| d.remove_temp::<bool>(Id::unique("devices-loaded")));
        }
    }
}

/// Read-only context the tabs need.
struct TabView<'a> {
    gpu: &'a crate::app::GpuInfo,
    devices: &'a [lumen400_engine::OutputDeviceInfo],
    virtual_audio: bool,
    history_len: usize,
    ephemeral: bool,
}

fn tab_label(tab: SettingsTab) -> &'static str {
    t(match tab {
        SettingsTab::Appearance => T::Appearance,
        SettingsTab::Playback => T::GroupPlayback,
        SettingsTab::Audio => T::GroupAudio,
        SettingsTab::Subtitles => T::Subtitles,
        SettingsTab::Advanced => T::Advanced,
        SettingsTab::About => T::About,
    })
}

fn tab_icon(tab: SettingsTab) -> char {
    match tab {
        SettingsTab::Appearance => icons::PALETTE,
        SettingsTab::Playback => icons::PLAY_CIRCLE,
        SettingsTab::Audio => icons::SPEAKER_HIGH,
        SettingsTab::Subtitles => icons::SUBTITLES,
        SettingsTab::Advanced => icons::SLIDERS,
        SettingsTab::About => icons::INFO,
    }
}

/// The vertical tab list.
fn nav(ui: &mut Ui, p: &Palette, tab: &mut SettingsTab) {
    ui.spacing_mut().item_spacing.y = 3.0;
    for t_ in SettingsTab::ALL {
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
        let selected = *tab == t_;
        let sel = ui.ctx().animate_bool_with_time(resp.id.with("sel"), selected, 0.14);
        let hover = ui.ctx().animate_bool_with_time(resp.id.with("hover"), resp.hovered() && !selected, 0.1);
        let cr = CornerRadius::same(11);
        if hover > 0.0 {
            ui.painter().rect_filled(rect, cr, fade(p.hover, hover));
        }
        if sel > 0.0 {
            ui.painter().rect_filled(rect, cr, fade(p.accent_soft, sel));
        }
        let fg = mix(p.text_dim, if p.dark { mix(p.accent, p.text, 0.4) } else { p.accent }, sel);
        ui.painter().text(
            pos2(rect.left() + 22.0, rect.center().y),
            Align2::CENTER_CENTER,
            tab_icon(t_).to_string(),
            FontId::new(19.0, fonts::icons()),
            mix(p.text_dim, p.accent, sel),
        );
        ui.painter().text(pos2(rect.left() + 44.0, rect.center().y), Align2::LEFT_CENTER, tab_label(t_), FontId::new(14.5, fonts::medium()), fg);
        if resp.clicked() {
            *tab = t_;
        }
    }
}

// =================================================================================================
// Controls
// =================================================================================================

/// `[slider] value` in a settings row (the row lays its controls out right to left). `true` when the
/// user moved the slider — callers must only write back then, or a unit conversion's rounding would
/// look like a change on every frame.
fn slider_control(ui: &mut Ui, p: &Palette, value: &mut f32, range: std::ops::RangeInclusive<f32>, step: f32, show: impl Fn(f32) -> String) -> bool {
    ui.label(RichText::new(show(*value)).font(FontId::new(12.5, fonts::mono())).color(p.text_dim));
    ui.add_space(8.0);
    ThinSlider::new(value, range).length(190.0).step(step).show(ui, p).changed()
}

fn text_control(ui: &mut Ui, text: &mut String, hint: &str, width: f32) {
    ui.add(TextEdit::singleline(text).hint_text(RichText::new(hint).color(ui.visuals().weak_text_color())).desired_width(width).margin(egui::Margin::symmetric(10, 7)));
}

/// A colour swatch row: the selected one gets a ring.
fn swatches(ui: &mut Ui, p: &Palette, accent: &mut Accent) {
    ui.spacing_mut().item_spacing.x = 8.0;
    for a in Accent::ALL.into_iter().rev() {
        let (rect, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
        let selected = *accent == a;
        let hover = ui.ctx().animate_bool_with_time(resp.id.with("h"), resp.hovered(), 0.1);
        let [r, g, b] = a.rgb();
        let col = Color32::from_rgb(r, g, b);
        let radius = 10.5 + 1.5 * hover;
        ui.painter().circle_filled(rect.center(), radius, col);
        if selected {
            ui.painter().circle_stroke(rect.center(), 13.5, Stroke::new(2.0, if p.dark { Color32::WHITE } else { Color32::from_gray(30) }));
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, icons::CHECK.to_string(), FontId::new(13.0, fonts::icons_bold()), Color32::from_rgb(12, 10, 22));
        }
        if resp.on_hover_ui(|ui| widgets::tooltip(ui, p, accent_name(a), None)).clicked() {
            *accent = a;
        }
    }
}

fn switch_row(ui: &mut Ui, p: &Palette, label: &str, hint: Option<&str>, value: &mut bool) {
    setting_row(ui, p, label, hint, |ui| {
        toggle_switch(ui, p, value);
    });
}

// =================================================================================================
// Tabs
// =================================================================================================

fn appearance(ui: &mut Ui, p: &Palette, s: &mut Settings) {
    section_title(ui, p, t(T::SectionLook));
    setting_row(ui, p, t(T::SetLangRow), None, |ui| {
        segmented(ui, p, &mut s.language, &[(Language::Auto, t(T::Automatic)), (Language::French, "Français"), (Language::English, "English")]);
    });
    setting_row(ui, p, t(T::ThemeRow), None, |ui| {
        segmented(
            ui,
            p,
            &mut s.theme,
            &[(ThemeMode::Dark, t(T::ThemeDark)), (ThemeMode::Oled, t(T::ThemeOled)), (ThemeMode::Light, t(T::ThemeLight)), (ThemeMode::System, t(T::ThemeSystem))],
        );
    });
    setting_row(ui, p, t(T::AccentRow), None, |ui| swatches(ui, p, &mut s.accent));

    section_title(ui, p, t(T::SectionInterface));
    setting_row(ui, p, t(T::UiScale), Some(t(T::UiScaleHint)), |ui| {
        slider_control(ui, p, &mut s.ui_scale, 0.8..=1.6, 0.05, |v| format!("{} %", (v * 100.0).round() as i32));
    });
    setting_row(ui, p, t(T::ControlsTimeout), None, |ui| {
        slider_control(ui, p, &mut s.controls_timeout, 1.0..=8.0, 0.5, |v| format!("{v:.1} s"));
    });
    switch_row(ui, p, t(T::AmbientGlow), Some(t(T::AmbientHint)), &mut s.ambient_mode);
    switch_row(ui, p, t(T::ShowRemaining), None, &mut s.show_remaining_time);
    switch_row(ui, p, t(T::CustomTitlebar), Some(t(T::RestartNeeded)), &mut s.custom_titlebar);
}

fn playback(ui: &mut Ui, p: &Palette, s: &mut Settings, view: &TabView) {
    section_title(ui, p, t(T::SectionBehaviour));
    switch_row(ui, p, t(T::ResumePlayback), Some(t(T::ResumeHint)), &mut s.resume_playback);
    switch_row(ui, p, t(T::QueueSiblings), Some(t(T::QueueSiblingsHint)), &mut s.queue_folder_siblings);
    setting_row(ui, p, t(T::Repeat), None, |ui| {
        segmented(ui, p, &mut s.repeat, &[(RepeatMode::Off, t(T::RepeatNone)), (RepeatMode::All, t(T::RepeatListShort)), (RepeatMode::One, t(T::RepeatItemShort))]);
    });
    switch_row(ui, p, t(T::Shuffle), None, &mut s.shuffle);

    section_title(ui, p, t(T::SectionSeeking));
    for (i, label) in [(0usize, T::SeekStepShort), (1, T::SeekStepMedium), (2, T::SeekStepLong)] {
        let (lo, hi, step) = match i {
            0 => (1.0, 30.0, 1.0),
            1 => (5.0, 120.0, 5.0),
            _ => (30.0, 600.0, 10.0),
        };
        let mut v = s.seek_steps[i] as f32;
        setting_row(ui, p, t(label), None, |ui| {
            if slider_control(ui, p, &mut v, lo..=hi, step, |v| format!("{} s", v.round() as i32)) {
                s.seek_steps[i] = f64::from(v);
            }
        });
    }
    setting_row(ui, p, t(T::VolumeStep), None, |ui| {
        let mut pct = s.volume_step * 100.0;
        if slider_control(ui, p, &mut pct, 1.0..=25.0, 1.0, |v| format!("{} %", v.round() as i32)) {
            s.volume_step = (pct / 100.0 * 100.0).round() / 100.0;
        }
    });
    switch_row(ui, p, t(T::Thumbnails), Some(t(T::ThumbnailsHint)), &mut s.generate_thumbnails);
    switch_row(ui, p, t(T::PreventSleep), Some(t(T::PreventSleepHint)), &mut s.prevent_sleep);

    section_title(ui, p, t(T::SectionDecoding));
    let hint = if view.gpu.software { t(T::HwDecodeHintSoftware) } else { t(T::HwDecodeHint) };
    setting_row(ui, p, t(T::HwDecode), Some(hint), |ui| {
        segmented(ui, p, &mut s.hw_decode, &[(HwDecode::Off, t(T::HwDecodeOff)), (HwDecode::Auto, t(T::Automatic))]);
    });
    setting_row(ui, p, t(T::Deinterlace), Some(t(T::DeinterlaceHint)), |ui| {
        segmented(ui, p, &mut s.deinterlace, &[(Deinterlace::Auto, t(T::Automatic)), (Deinterlace::Off, t(T::DeinterlaceOff)), (Deinterlace::On, t(T::DeinterlaceOn))]);
    });
}

fn audio(ui: &mut Ui, p: &Palette, s: &mut Settings, view: &TabView, req: &mut Requests) {
    section_title(ui, p, t(T::SectionOutput));
    let current = match &s.audio_device {
        None => t(T::DeviceDefault).to_owned(),
        Some(id) => view.devices.iter().find(|d| &d.id == id).map_or_else(|| id.clone(), |d| d.name.clone()),
    };
    setting_row(ui, p, t(T::OutputDevice), if view.virtual_audio { Some(t(T::VirtualAudioNote)) } else { None }, |ui| {
        if widgets::IconButton::new(icons::ARROW_CW).size(32.0).tip(t(T::Refresh)).show(ui, p).clicked() {
            req.refresh_devices = true;
        }
        egui::ComboBox::from_id_salt("audio-device").selected_text(ellipsize_middle(&current, 30)).width(250.0).show_ui(ui, |ui| {
            if ui.selectable_label(s.audio_device.is_none(), t(T::DeviceDefault)).clicked() {
                s.audio_device = None;
            }
            for d in view.devices {
                let label = if d.is_default { format!("{} ({})", d.name, t(T::Default)) } else { d.name.clone() };
                if ui.selectable_label(s.audio_device.as_deref() == Some(d.id.as_str()), label).clicked() {
                    s.audio_device = Some(d.id.clone());
                }
            }
        });
    });
    setting_row(ui, p, t(T::Volume), None, |ui| {
        let mut pct = s.volume * 100.0;
        if slider_control(ui, p, &mut pct, 0.0..=200.0, 1.0, |v| format!("{} %", v.round() as i32)) {
            s.volume = (pct / 100.0 * 100.0).round() / 100.0;
        }
    });

    section_title(ui, p, t(T::SectionSound));
    setting_row(ui, p, t(T::Equalizer), Some(if s.eq.enabled { t(T::EqOnHint) } else { t(T::EqOffHint) }), |ui| {
        if button(ui, p, Some(icons::EQUALIZER), t(T::Open), false).clicked() {
            req.open_equalizer = true;
        }
    });
    switch_row(ui, p, t(T::NightMode), Some(t(T::NightModeHint)), &mut s.night_mode);
    setting_row(ui, p, t(T::PreferredAudio), Some(t(T::PreferredHint)), |ui| text_control(ui, &mut s.preferred_audio_langs, "fra, eng", 170.0));
}

fn subtitles(ui: &mut Ui, p: &Palette, s: &mut Settings) {
    section_title(ui, p, t(T::SectionBehaviour));
    switch_row(ui, p, t(T::AutoSubtitles), Some(t(T::AutoSubtitlesHint)), &mut s.auto_load_subtitles);
    setting_row(ui, p, t(T::PreferredSubs), Some(t(T::PreferredHint)), |ui| text_control(ui, &mut s.preferred_sub_langs, "fra, eng", 170.0));

    section_title(ui, p, t(T::Style));
    // Live preview on a dark, picture-like card.
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 104.0), Sense::hover());
    {
        let painter = ui.painter_at(rect);
        theme::paint_rounded_vgradient(&painter, rect, 12.0, Color32::from_rgb(38, 44, 66), Color32::from_rgb(14, 16, 26));
        painter.rect_stroke(rect, CornerRadius::same(12), Stroke::new(1.0, p.border), StrokeKind::Inside);
        let cue = Cue::from_text(0.0, 1.0, t(T::SubPreview));
        let size = (rect.height() * 0.19 * s.subtitle_style.scale).clamp(10.0, 40.0);
        let bottom = rect.bottom() - rect.height() * s.subtitle_style.bottom_margin.max(0.08);
        crate::ui::stage::paint_cues(&painter, rect, &[cue], &s.subtitle_style, size, bottom);
    }
    ui.add_space(4.0);
    setting_row(ui, p, t(T::SubSize), None, |ui| {
        slider_control(ui, p, &mut s.subtitle_style.scale, 0.5..=2.5, 0.05, |v| format!("{} %", (v * 100.0).round() as i32));
    });
    setting_row(ui, p, t(T::SubColor), None, |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        for c in [[255, 255, 255], [255, 224, 102], [125, 232, 255], [150, 240, 160]].into_iter().rev() {
            let (rect, resp) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::click());
            ui.painter().circle_filled(rect.center(), 10.0, Color32::from_rgb(c[0], c[1], c[2]));
            ui.painter().circle_stroke(rect.center(), 10.0, Stroke::new(1.0, p.border_strong));
            if s.subtitle_style.color == c {
                ui.painter().circle_stroke(rect.center(), 13.0, Stroke::new(2.0, p.accent));
            }
            if resp.clicked() {
                s.subtitle_style.color = c;
            }
        }
    });
    switch_row(ui, p, t(T::SubBold), None, &mut s.subtitle_style.bold);
    switch_row(ui, p, t(T::SubOutline), None, &mut s.subtitle_style.outline);
    switch_row(ui, p, t(T::SubBackground), None, &mut s.subtitle_style.background);
    if s.subtitle_style.background {
        setting_row(ui, p, t(T::SubBackgroundOpacity), None, |ui| {
            slider_control(ui, p, &mut s.subtitle_style.bg_opacity, 0.1..=1.0, 0.05, |v| format!("{} %", (v * 100.0).round() as i32));
        });
    }
    setting_row(ui, p, t(T::SubPosition), None, |ui| {
        slider_control(ui, p, &mut s.subtitle_style.bottom_margin, 0.02..=0.3, 0.01, |v| format!("{} %", (v * 100.0).round() as i32));
    });
}

fn advanced(ui: &mut Ui, p: &Palette, s: &mut Settings, view: &TabView, req: &mut Requests) {
    section_title(ui, p, t(T::SectionFiles));
    let dir = s.screenshot_directory().to_string_lossy().into_owned();
    setting_row(ui, p, t(T::ScreenshotFolder), Some(&ellipsize_middle(&dir, 46)), |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if widgets::IconButton::new(icons::FOLDER_OPEN).size(34.0).tip(t(T::ShowFolder)).show(ui, p).clicked() {
            req.open_screenshot_dir = true;
        }
        if button(ui, p, None, t(T::Choose), false).clicked() {
            req.choose_screenshot_dir = true;
        }
    });
    setting_row(ui, p, t(T::ConfigFolder), None, |ui| {
        if widgets::IconButton::new(icons::FOLDER_OPEN).size(34.0).tip(t(T::ShowFolder)).show(ui, p).clicked() {
            req.open_config_dir = true;
        }
    });

    section_title(ui, p, t(T::SectionDesktop));
    switch_row(ui, p, t(T::SingleInstance), Some(t(T::SingleInstanceHint)), &mut s.single_instance);

    section_title(ui, p, t(T::SectionData));
    setting_row(ui, p, t(T::PlaybackHistory), Some(&t1(T::PlaylistCount, view.history_len)), |ui| {
        if button(ui, p, Some(icons::TRASH), t(T::ClearHistory), false).clicked() {
            req.clear_history = true;
        }
    });
    let confirm_id = Id::unique("reset-confirm");
    let now = ui.ctx().input(|i| i.time);
    let armed = ui.ctx().data(|d| d.get_temp::<f64>(confirm_id)).is_some_and(|t0| now - t0 < 4.0);
    setting_row(ui, p, t(T::ResetSettings), Some(if armed { t(T::ResetConfirm) } else { t(T::ResetHint) }), |ui| {
        let label = if armed { t(T::Confirm) } else { t(T::Reset) };
        if danger_button(ui, p, label, armed).clicked() {
            if armed {
                req.reset_settings = true;
                ui.ctx().data_mut(|d| d.remove_temp::<f64>(confirm_id));
            } else {
                ui.ctx().data_mut(|d| d.insert_temp(confirm_id, now));
            }
        }
    });
    if armed {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
    }
    if view.ephemeral {
        ui.add_space(8.0);
        ui.label(RichText::new(t(T::EphemeralNote)).font(FontId::new(12.0, fonts::medium())).color(p.warn));
    }

    section_title(ui, p, t(T::SectionGraphics));
    gpu_rows(ui, p, view);
}

fn danger_button(ui: &mut Ui, p: &Palette, label: &str, armed: bool) -> egui::Response {
    let font = FontId::new(14.0, fonts::semibold());
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, Color32::PLACEHOLDER);
    let size = vec2(galley.size().x + 32.0, 38.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let hover = ui.ctx().animate_bool_with_time(resp.id.with("h"), resp.hovered(), 0.1);
    let cr = CornerRadius::same(12);
    let (fill, fg) = if armed { (mix(p.danger, Color32::WHITE, 0.08 * hover), Color32::WHITE) } else { (mix(p.raised, p.hover, hover), p.danger) };
    ui.painter().rect_filled(rect, cr, fill);
    ui.painter().rect_stroke(rect, cr, Stroke::new(1.0, fade(p.danger, if armed { 0.0 } else { 0.55 })), StrokeKind::Inside);
    ui.painter().galley(rect.center() - galley.size() * 0.5, galley, fg);
    resp
}

fn kv_row(ui: &mut Ui, p: &Palette, key: &str, value: &str) {
    ui.scope(|ui| {
        ui.spacing_mut().interact_size.y = 20.0;
        ui.spacing_mut().item_spacing.y = 5.0;
        ui.horizontal(|ui| {
            ui.set_min_height(24.0);
            ui.label(RichText::new(key).font(FontId::new(13.0, fonts::medium())).color(p.text_faint));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(value).font(FontId::new(13.0, fonts::medium())).color(p.text));
            });
        });
    });
}

fn gpu_rows(ui: &mut Ui, p: &Palette, view: &TabView) {
    let g = view.gpu;
    kv_row(ui, p, t(T::GpuAdapter), &if g.software { format!("{} · {}", g.adapter, t(T::GpuSoftware)) } else { g.adapter.clone() });
    kv_row(ui, p, t(T::GpuBackend), &g.backend);
    if !g.driver.is_empty() {
        kv_row(ui, p, t(T::GpuDriver), &ellipsize_middle(&g.driver, 48));
    }
    kv_row(ui, p, t(T::Gpu16Bit), t(if g.supports_16bit { T::Yes } else { T::No }));
}

fn about(ui: &mut Ui, p: &Palette, view: &TabView) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(76.0, 76.0), Sense::hover());
        crate::ui::welcome::paint_logo(ui, rect, p);
        ui.add_space(14.0);
        ui.vertical(|ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.label(RichText::new("400_").font(FontId::new(30.0, fonts::bold())).color(p.text));
                ui.label(RichText::new("Player").font(FontId::new(30.0, fonts::bold())).color(p.accent));
            });
            ui.label(RichText::new(t(T::Tagline)).font(FontId::new(13.5, fonts::medium())).color(p.text_dim));
        });
    });
    ui.add_space(8.0);

    section_title(ui, p, t(T::AboutVersion));
    kv_row(ui, p, t(T::AboutVersion), lumen400_core::APP_VERSION);
    kv_row(ui, p, t(T::AboutLicense), "MIT OR Apache-2.0");
    kv_row(ui, p, t(T::AboutEngine), &lumen400_engine::engine_version());
    kv_row(ui, p, t(T::AboutPlatform), &format!("{} · {}", std::env::consts::OS, std::env::consts::ARCH));

    section_title(ui, p, t(T::SectionGraphics));
    gpu_rows(ui, p, view);

    section_title(ui, p, t(T::AboutCredits));
    ui.label(RichText::new(t(T::AboutIndependent)).font(FontId::new(13.0, fonts::medium())).color(p.text_dim));
    ui.add_space(6.0);
    ui.label(RichText::new(t(T::AboutBuiltWith)).font(FontId::new(12.5, fonts::medium())).color(p.text_faint));
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        for name in ["Native Rust A/V", "wgpu", "egui", "cpal", "Phosphor Icons", "Inter", "JetBrains Mono"] {
            widgets::chip(ui, p, name, false);
        }
    });
}

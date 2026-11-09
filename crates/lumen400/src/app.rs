//! The application object: owns the engine, the settings and every piece of interface state, and
//! drives one frame (`logic` → `ui`) at a time.

use std::sync::Arc;
use std::time::Duration;

use eframe::egui_wgpu;
use egui::{Context, TextureHandle};
use lumen400_core::playlist::Playlist;
use lumen400_core::settings::{History, Settings};
use lumen400_core::types::MediaInfo;
use lumen400_engine::{OutputDeviceInfo, PlayState, Player, RgbaImage, Snapshot, SpectrumView, StreamSelection, Thumbnail, VideoFrame};

use crate::actions::Action;
use crate::cli::Cli;
use crate::control::engine_config;
use crate::desktop::{self, Track};
use crate::files::Dialogs;
use crate::i18n;
use crate::state::{AbLoop, ExternalSub, MenuState, Osd, SeekDrag, SubSource, Toast, Windows};
use crate::theme::{self, Palette};
use crate::video::{VideoGpu, ViewState, gpu::supports_16bit};

/// What the GPU is, for the "about" page and the statistics.
#[derive(Clone, Debug, Default)]
pub struct GpuInfo {
    pub adapter: String,
    pub backend: String,
    pub driver: String,
    pub software: bool,
    pub supports_16bit: bool,
}

/// Preview pictures of the seek bar, uploaded once as textures.
#[derive(Default)]
pub struct ThumbCache {
    items: Vec<(u64, TextureHandle)>,
}

impl ThumbCache {
    const CAPACITY: usize = 24;

    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn texture(&mut self, ctx: &Context, t: &Thumbnail) -> egui::TextureId {
        if let Some(pos) = self.items.iter().position(|(id, _)| *id == t.id) {
            // Most recently used last.
            let item = self.items.remove(pos);
            let id = item.1.id();
            self.items.push(item);
            return id;
        }
        let img = egui::ColorImage::from_rgba_unmultiplied([t.image.width as usize, t.image.height as usize], &t.image.data);
        let handle = ctx.load_texture(format!("thumb-{}", t.id), img, egui::TextureOptions::LINEAR);
        let id = handle.id();
        self.items.push((t.id, handle));
        if self.items.len() > Self::CAPACITY {
            // Dropping the entry frees its GPU texture.
            drop(self.items.remove(0));
        }
        id
    }
}

/// Bitmap subtitle regions (PGS, VobSub, DVB), uploaded once as GPU textures.
#[derive(Default)]
pub struct SubBitmapCache {
    items: Vec<(u64, TextureHandle)>,
}

impl SubBitmapCache {
    const CAPACITY: usize = 16;

    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn texture(&mut self, ctx: &Context, bm: &lumen400_core::subtitle::BitmapRect) -> egui::TextureId {
        if let Some(pos) = self.items.iter().position(|(id, _)| *id == bm.id) {
            let item = self.items.remove(pos);
            let id = item.1.id();
            self.items.push(item);
            return id;
        }
        let img = egui::ColorImage::from_rgba_unmultiplied([bm.w as usize, bm.h as usize], &bm.rgba);
        let handle = ctx.load_texture(format!("sub-bm-{}", bm.id), img, egui::TextureOptions::LINEAR);
        let id = handle.id();
        self.items.push((bm.id, handle));
        if self.items.len() > Self::CAPACITY {
            drop(self.items.remove(0));
        }
        id
    }
}

/// What the announced track was built from, to notice when it must be built again.
#[derive(Default)]
pub struct TrackKey {
    pub source: Option<String>,
    pub media: Option<usize>,
    pub cover: Option<usize>,
}

/// The player.
pub struct LumenApp {
    pub ctx: Context,
    pub player: Player,
    pub settings: Settings,
    pub settings_dirty_since: Option<f64>,
    /// Do not read or write anything in the user's profile (screenshots, demos, tests).
    pub ephemeral: bool,
    /// Never open a sound card (CI, containers).
    pub virtual_audio: bool,
    pub awake: crate::power::Awake,
    /// The player on the desktop's bus (media keys, `playerctl`, one window), when it is there.
    pub desktop: Option<desktop::Service>,
    /// What the desktop is told is playing.
    pub track: Arc<Track>,
    pub track_key: TrackKey,
    pub track_counter: u64,
    pub history: History,
    pub history_dirty: bool,
    pub history_saved_at: f64,
    pub playlist: Playlist,
    pub pal: Palette,
    pub gpu: GpuInfo,
    pub render_state: Option<egui_wgpu::RenderState>,

    // --- the media ------------------------------------------------------------------------------
    /// What the user asked to play (path or URL).
    pub source: Option<String>,
    pub media: Option<Arc<MediaInfo>>,
    pub snap: Snapshot,
    /// The picture on screen.
    pub frame: Option<Arc<VideoFrame>>,
    pub cover: Option<TextureHandle>,
    /// The cover as it came from the file (the desktop shows it too).
    pub cover_image: Option<Arc<RgbaImage>>,
    pub view: ViewState,
    pub selection: StreamSelection,
    pub sub_source: SubSource,
    pub ext_sub: Option<ExternalSub>,
    pub sub_delay: f64,
    pub audio_delay: f64,
    pub ab: AbLoop,
    pub ab_cooldown_until: f64,
    pub failures: u32,
    pub opening_since: Option<f64>,
    pub pending_start: Option<f64>,
    pub ended_handled: bool,
    pub progress_saved_at: f64,
    pub spectrum: SpectrumView,
    pub spectrum_on: bool,
    /// Seconds between the moment a frame is picked and the moment it reaches the screen.
    pub lookahead: f64,

    // --- the interface --------------------------------------------------------------------------
    pub activity_at: f64,
    pub windows: Windows,
    pub menu: Option<MenuState>,
    pub osd: Option<Osd>,
    pub toasts: Vec<Toast>,
    pub toast_seq: u64,
    pub seek_drag: Option<SeekDrag>,
    pub thumbs: ThumbCache,
    pub sub_bitmaps: SubBitmapCache,
    pub fullscreen: bool,
    pub always_on_top: bool,
    pub window_title: String,
    pub dialogs: Dialogs,
    pub devices: Vec<OutputDeviceInfo>,
    pub scroll_acc: f32,
    pub drop_hover: bool,
    pub boot_paused: bool,
    pub quitting: bool,
    pub automation: Option<crate::automation::Automation>,
    /// Area of the dock on the last frame (subtitles and toasts keep clear of it).
    pub dock_rect: Option<egui::Rect>,
    pub stage_rect: egui::Rect,
    pub playlist_scroll_to_current: bool,
    /// Index of the playlist row being dragged to a new place.
    pub playlist_drag: Option<usize>,
    pub playlist_filter: String,
    /// A click that only closed a menu: nothing underneath may react to it.
    pub swallow_click: bool,
    pub palette_cache: Vec<crate::ui::palette::Entry>,
    pub advanced_tab: crate::ui::advanced::AdvancedTab,
    pub advanced_search: String,
    pub advanced_favorites: std::collections::HashSet<String>,
}

impl LumenApp {
    pub fn new(cc: &eframe::CreationContext<'_>, cli: &Cli, settings: Settings, ephemeral: bool, mut desktop: Option<desktop::Service>) -> Self {
        let ctx = cc.egui_ctx.clone();
        crate::fonts::install(&ctx);
        i18n::set_language(settings.language);
        let system_dark = ctx.system_theme().is_none_or(|t| t == egui::Theme::Dark);
        let pal = Palette::new(settings.theme, settings.accent, system_dark);
        theme::apply(&ctx, &pal, settings.ui_scale);

        // The renderer's pipelines live in egui's callback resources.
        let mut gpu = GpuInfo::default();
        let render_state = cc.wgpu_render_state.clone();
        if let Some(rs) = &render_state {
            let info = rs.adapter.get_info();
            gpu = GpuInfo {
                adapter: info.name.clone(),
                backend: format!("{:?}", info.backend),
                driver: [info.driver.as_str(), info.driver_info.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" "),
                software: info.device_type == eframe::wgpu::DeviceType::Cpu,
                supports_16bit: supports_16bit(rs.device.features()),
            };
            rs.renderer.write().callback_resources.insert(VideoGpu::new(&rs.device, rs.target_format));
            tracing::info!("GPU: {} ({}), 16-bit textures: {}", gpu.adapter, gpu.backend, gpu.supports_16bit);
        } else {
            tracing::error!("no wgpu render state: video cannot be displayed");
        }

        let waker_ctx = ctx.clone();
        let waker: Arc<dyn Fn() + Send + Sync> = Arc::new(move || waker_ctx.request_repaint());
        let cfg = engine_config(&settings, gpu.supports_16bit, cli.virtual_audio);
        if let Some(d) = desktop.as_mut() {
            d.set_waker(waker.clone());
        }
        let player = Player::new(cfg, Some(waker));
        player.set_volume(settings.volume);
        player.set_muted(settings.muted);
        player.set_equalizer(&settings.eq);
        player.set_night_mode(settings.night_mode);
        player.set_deinterlace(settings.deinterlace);

        let history = if ephemeral { History::default() } else { History::load() };
        let mut playlist = Playlist::new();
        playlist.set_repeat(settings.repeat);
        playlist.set_shuffle(settings.shuffle);

        let mut app = Self {
            dialogs: Dialogs::new(ctx.clone()),
            ctx,
            player,
            settings,
            settings_dirty_since: None,
            ephemeral,
            virtual_audio: cli.virtual_audio,
            awake: crate::power::Awake::default(),
            desktop,
            track: Arc::new(Track::default()),
            track_key: TrackKey::default(),
            track_counter: 0,
            history,
            history_dirty: false,
            history_saved_at: 0.0,
            playlist,
            pal,
            gpu,
            render_state,
            source: None,
            media: None,
            snap: Snapshot::default(),
            frame: None,
            cover: None,
            cover_image: None,
            view: ViewState::default(),
            selection: StreamSelection::default(),
            sub_source: SubSource::Off,
            ext_sub: None,
            sub_delay: 0.0,
            audio_delay: 0.0,
            ab: AbLoop::Off,
            ab_cooldown_until: 0.0,
            failures: 0,
            opening_since: None,
            pending_start: None,
            ended_handled: false,
            progress_saved_at: 0.0,
            spectrum: SpectrumView::default(),
            spectrum_on: false,
            lookahead: 1.0 / 60.0,
            activity_at: 0.0,
            windows: Windows::default(),
            menu: None,
            osd: None,
            toasts: Vec::new(),
            toast_seq: 0,
            seek_drag: None,
            thumbs: ThumbCache::default(),
            sub_bitmaps: SubBitmapCache::default(),
            fullscreen: false,
            always_on_top: false,
            window_title: String::new(),
            devices: Vec::new(),
            scroll_acc: 0.0,
            drop_hover: false,
            boot_paused: cli.paused,
            quitting: false,
            automation: None,
            dock_rect: None,
            stage_rect: egui::Rect::ZERO,
            playlist_scroll_to_current: false,
            playlist_drag: None,
            playlist_filter: String::new(),
            swallow_click: false,
            palette_cache: Vec::new(),
            advanced_tab: crate::ui::advanced::AdvancedTab::Home,
            advanced_search: String::new(),
            advanced_favorites: std::collections::HashSet::new(),
        };

        if let Some(path) = &cli.automation {
            match crate::automation::Automation::load(path) {
                Ok(a) => app.automation = Some(a),
                Err(e) => tracing::error!("cannot load the automation script: {e}"),
            }
        }
        app.apply_cli(cli, false);
        app.update_title();
        app
    }

    /// Everything that happens before the interface is drawn, once per frame.
    fn frame_logic(&mut self, ctx: &Context) {
        let now = ctx.input(|i| i.time);
        self.fullscreen = ctx.input(|i| i.viewport().fullscreen).unwrap_or(self.fullscreen);
        self.track_window(ctx);
        self.pump();
        self.handle_answers();
        self.expire_messages(now);
        if let Some(mut a) = self.automation.take() {
            a.step(ctx, self, now);
            self.automation = Some(a);
        }
        self.schedule_repaint(ctx, now);
    }

    /// Remembers the window geometry (written to disk at exit).
    fn track_window(&mut self, ctx: &Context) {
        let (inner, outer, maximized, fullscreen, minimized) = ctx.input(|i| {
            let v = i.viewport();
            (
                v.inner_rect,
                v.outer_rect,
                v.maximized.unwrap_or(false),
                v.fullscreen.unwrap_or(false),
                v.minimized.unwrap_or(false),
            )
        });
        if minimized {
            return;
        }
        let w = &mut self.settings.window;
        w.maximized = maximized;
        if !maximized && !fullscreen {
            if let Some(r) = inner
                && r.width() >= 480.0
                && r.height() >= 320.0
            {
                w.width = r.width();
                w.height = r.height();
            }
            if let Some(r) = outer
                && (0.0..=1600.0).contains(&r.min.x)
                && (0.0..=900.0).contains(&r.min.y)
            {
                w.x = Some(r.min.x);
                w.y = Some(r.min.y);
            }
        }
    }

    fn expire_messages(&mut self, now: f64) {
        self.toasts.retain(|t| now - t.born < t.ttl);
        if let Some(o) = &self.osd
            && now - o.born > crate::ui::osd::OSD_LIFETIME
        {
            self.osd = None;
        }
    }

    /// Asks egui for the next frame only when something will have changed by then.
    fn schedule_repaint(&mut self, ctx: &Context, now: f64) {
        let mut next: Option<f64> = None;
        let mut want = |secs: f64| {
            next = Some(next.map_or(secs, |n: f64| n.min(secs)));
        };
        match self.snap.state {
            PlayState::Playing => {
                let has_picture = self.media.as_ref().is_some_and(|m| m.has_video());
                if has_picture {
                    match self.player.next_frame_in() {
                        // Wake up just before the frame is due, so it can go out with the next refresh.
                        Some(dt) => want((dt - self.lookahead).max(0.001)),
                        None => want(0.008),
                    }
                    // The clock and the seek bar move continuously.
                    want(1.0 / 30.0);
                } else {
                    want(1.0 / 60.0);
                }
            }
            PlayState::Opening | PlayState::Buffering => want(1.0 / 30.0),
            _ => {}
        }
        // Controls fading out.
        let idle_for = now - self.activity_at;
        let timeout = f64::from(self.settings.controls_timeout);
        if self.snap.state == PlayState::Playing && idle_for < timeout + 0.6 {
            want(0.05);
        }
        if let Some(o) = &self.osd {
            want((crate::ui::osd::OSD_LIFETIME - (now - o.born)).clamp(0.016, 0.05));
        }
        if let Some(t) = self.toasts.iter().map(|t| t.ttl - (now - t.born)).fold(None, |m: Option<f64>, v| Some(m.map_or(v, |x| x.min(v)))) {
            want(t.clamp(0.016, 0.1));
        }
        if let Some(secs) = next {
            ctx.request_repaint_after(Duration::from_secs_f64(secs.clamp(0.001, 0.5)));
        }
    }

    /// Final housekeeping: remember where we stopped, write settings and history.
    fn shutdown(&mut self) {
        // Leave the bus first: a launch from now on starts a window of its own instead of handing
        // its files to one that is closing.
        self.desktop = None;
        self.record_history(false);
        self.save_history_now();
        self.settings.volume = self.settings.volume.clamp(0.0, 2.0);
        self.save_settings_now();
        self.player.stop();
    }

    /// The command a menu or button asked for, applied after the frame's drawing is done.
    pub fn dispatch(&mut self, actions: Vec<Action>) {
        for a in actions {
            self.run(a);
        }
    }
}

impl eframe::App for LumenApp {
    fn logic(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.frame_logic(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        self.pal.video_bg.to_normalized_gamma_f32()
    }

    fn raw_input_hook(&mut self, ctx: &Context, raw: &mut egui::RawInput) {
        if let Some(mut a) = self.automation.take() {
            a.inject(ctx, raw);
            self.automation = Some(a);
        }
    }

    fn on_exit(&mut self) {
        self.quitting = true;
        self.shutdown();
    }
}

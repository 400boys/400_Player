//! The video renderer: geometry (fit / fill / rotation / zoom), the egui paint callback and the
//! GPU pipeline ([`gpu`]). Colour handling is in [`color`].

pub mod color;
pub mod gpu;

#[cfg(test)]
mod gpu_tests;

use std::sync::Arc;

use eframe::egui::{self, Rect, Vec2, pos2, vec2};
use eframe::egui_wgpu;
use media_player_400_engine::VideoFrame;

pub use gpu::{Adjust, DrawParams, VideoGpu};

/// How the picture is scaled into the available area.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FitMode {
    /// Whole picture visible, black bars where the aspect ratios differ.
    #[default]
    Fit,
    /// Picture covers the area, cropping what overflows.
    Fill,
    /// Picture is stretched to the area, ignoring its aspect ratio.
    Stretch,
    /// One video pixel per screen pixel.
    Original,
}

/// User-controlled presentation of the picture.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewState {
    pub fit: FitMode,
    /// Forced display aspect ratio (width / height); `None` = the stream's own.
    pub aspect: Option<f32>,
    pub zoom: f32,
    pub pan: Vec2,
    /// Extra rotation requested by the user, degrees clockwise.
    pub rotation: u16,
    pub flip_h: bool,
    pub adjust: Adjust,
}

impl Default for ViewState {
    fn default() -> Self {
        Self { fit: FitMode::Fit, aspect: None, zoom: 1.0, pan: Vec2::ZERO, rotation: 0, flip_h: false, adjust: Adjust::default() }
    }
}

impl ViewState {
    /// Back to the neutral view (keeps the colour adjustments).
    pub fn reset_geometry(&mut self) {
        self.fit = FitMode::Fit;
        self.aspect = None;
        self.zoom = 1.0;
        self.pan = Vec2::ZERO;
        self.rotation = 0;
        self.flip_h = false;
    }

    pub fn is_neutral(&self) -> bool {
        self.fit == FitMode::Fit && self.aspect.is_none() && (self.zoom - 1.0).abs() < 1e-3 && self.pan == Vec2::ZERO && self.rotation == 0 && !self.flip_h
    }
}

/// Size of the picture as displayed (before scaling), in video pixels, honouring the pixel aspect
/// ratio, a forced aspect ratio and the total rotation.
pub fn display_size(frame_w: u32, frame_h: u32, sar: f32, rotation: u16, aspect: Option<f32>) -> Vec2 {
    let (w, h) = (frame_w.max(1) as f32, frame_h.max(1) as f32);
    let sar = if sar.is_finite() && sar > 0.01 { sar } else { 1.0 };
    let dar = aspect.filter(|a| a.is_finite() && *a > 0.05).unwrap_or(w * sar / h);
    let (dw, dh) = (h * dar, h);
    if rotation % 180 == 90 { vec2(dh, dw) } else { vec2(dw, dh) }
}

/// Where the picture goes inside `area` (points). Edges are snapped to physical pixels so a 1:1
/// presentation stays sharp.
pub fn dest_rect(area: Rect, disp: Vec2, view: &ViewState, ppp: f32) -> Rect {
    let ppp = ppp.max(0.5);
    let base = match view.fit {
        FitMode::Fit => disp * (area.width() / disp.x).min(area.height() / disp.y),
        FitMode::Fill => disp * (area.width() / disp.x).max(area.height() / disp.y),
        FitMode::Stretch => area.size(),
        FitMode::Original => disp / ppp,
    };
    let size = base * view.zoom.clamp(0.1, 16.0);
    let r = Rect::from_center_size(area.center() + view.pan, size);
    let snap = |v: f32| (v * ppp).round() / ppp;
    Rect::from_min_max(pos2(snap(r.min.x), snap(r.min.y)), pos2(snap(r.max.x), snap(r.max.y)))
}

/// `inner` expressed in the normalised device coordinates of `outer`: `[left, bottom, right, top]`.
pub fn to_ndc(inner: Rect, outer: Rect) -> [f32; 4] {
    let w = outer.width().max(1e-3);
    let h = outer.height().max(1e-3);
    [
        2.0 * (inner.min.x - outer.min.x) / w - 1.0,
        1.0 - 2.0 * (inner.max.y - outer.min.y) / h,
        2.0 * (inner.max.x - outer.min.x) / w - 1.0,
        1.0 - 2.0 * (inner.min.y - outer.min.y) / h,
    ]
}

/// Rotation (degrees clockwise) that makes the picture upright: the stream's own plus the user's.
pub fn total_rotation(stream: i32, user: u16) -> u16 {
    ((stream.rem_euclid(360) as u16) + user) % 360
}

/// The paint callback registered with egui for one frame of video.
pub struct VideoCallback {
    pub frame: Arc<VideoFrame>,
    pub params: DrawParams,
    pub ambient: bool,
}

impl egui_wgpu::CallbackTrait for VideoCallback {
    fn prepare(
        &self,
        device: &eframe::wgpu::Device,
        queue: &eframe::wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        encoder: &mut eframe::wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<eframe::wgpu::CommandBuffer> {
        let Some(gpu) = resources.get_mut::<VideoGpu>() else { return Vec::new() };
        let view = gpu::FrameView::from_frame(&self.frame);
        if gpu.upload(device, queue, &view) {
            gpu.note_look(&self.params);
            let (main, full) = gpu::uniforms(&view, &self.params, gpu.target_is_srgb());
            gpu.set_uniforms(queue, &main, &full);
            gpu.prepare_ambient(encoder, self.ambient);
        }
        Vec::new()
    }

    fn paint(&self, info: egui::PaintCallbackInfo, pass: &mut eframe::wgpu::RenderPass<'static>, resources: &egui_wgpu::CallbackResources) {
        let Some(gpu) = resources.get::<VideoGpu>() else { return };
        if self.ambient {
            // The glow is only visible in the bars: fill just those, never the area the opaque picture
            // is about to cover (on a software rasteriser the saved overdraw is a large share of the frame).
            let vp = info.viewport_in_pixels();
            let clip = info.clip_rect_in_pixels();
            let area = PxRect { x: vp.left_px, y: vp.top_px, w: vp.width_px, h: vp.height_px };
            let clip_rect = PxRect { x: clip.left_px, y: clip.top_px, w: clip.width_px, h: clip.height_px };
            for bar in ambient_bars(self.params.dst, area) {
                if let Some(r) = bar.intersect(clip_rect) {
                    pass.set_scissor_rect(r.x as u32, r.y as u32, r.w as u32, r.h as u32);
                    gpu.draw_ambient(pass);
                }
            }
            pass.set_scissor_rect(clip_rect.x.max(0) as u32, clip_rect.y.max(0) as u32, clip_rect.w.max(0) as u32, clip_rect.h.max(0) as u32);
        }
        gpu.draw_video(pass);
    }
}

/// A rectangle in physical pixels (`x`, `y` = top-left).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PxRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl PxRect {
    fn right(self) -> i32 {
        self.x + self.w
    }

    fn bottom(self) -> i32 {
        self.y + self.h
    }

    /// The overlap of two rectangles, `None` when empty.
    pub fn intersect(self, o: PxRect) -> Option<PxRect> {
        let (x0, y0) = (self.x.max(o.x), self.y.max(o.y));
        let (x1, y1) = (self.right().min(o.right()), self.bottom().min(o.bottom()));
        (x1 > x0 && y1 > y0).then_some(PxRect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 })
    }
}

/// The parts of `area` the picture (`dst`, `[left, bottom, right, top]` in the area's normalised
/// coordinates) does not cover: up to four non-overlapping rectangles. Empty when it fills the area.
pub fn ambient_bars(dst: [f32; 4], area: PxRect) -> Vec<PxRect> {
    let px = |ndc: f32, origin: i32, size: i32| origin + ((ndc * 0.5 + 0.5) * size as f32).round() as i32;
    let x0 = px(dst[0], area.x, area.w).clamp(area.x, area.right());
    let x1 = px(dst[2], area.x, area.w).clamp(area.x, area.right());
    // NDC y points up, pixel rows point down.
    let y0 = px(-dst[3], area.y, area.h).clamp(area.y, area.bottom());
    let y1 = px(-dst[1], area.y, area.h).clamp(area.y, area.bottom());
    let (x0, x1) = (x0.min(x1), x0.max(x1));
    let (y0, y1) = (y0.min(y1), y0.max(y1));
    let mut bars = Vec::with_capacity(4);
    let mut push = |r: PxRect| {
        if r.w > 0 && r.h > 0 {
            bars.push(r);
        }
    };
    push(PxRect { x: area.x, y: area.y, w: x0 - area.x, h: area.h });
    push(PxRect { x: x1, y: area.y, w: area.right() - x1, h: area.h });
    push(PxRect { x: x0, y: area.y, w: x1 - x0, h: y0 - area.y });
    push(PxRect { x: x0, y: y1, w: x1 - x0, h: area.bottom() - y1 });
    bars
}

/// Adds the video to `painter`: the picture lands in `dest`, the glow (if any) fills `area`.
pub fn paint(painter: &egui::Painter, area: Rect, dest: Rect, frame: Arc<VideoFrame>, mut params: DrawParams, ambient: bool) {
    params.dst = to_ndc(dest, area);
    params.ambient = if ambient { 0.55 } else { 0.0 };
    painter.add(egui_wgpu::Callback::new_paint_callback(area, VideoCallback { frame, params, ambient }));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area() -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), vec2(1600.0, 900.0))
    }

    #[test]
    fn fit_letterboxes_and_pillarboxes() {
        let v = ViewState::default();
        // 4:3 in a 16:9 area → pillarbox
        let r = dest_rect(area(), display_size(640, 480, 1.0, 0, None), &v, 1.0);
        assert_eq!(r.height(), 900.0);
        assert_eq!(r.width(), 1200.0);
        assert_eq!(r.center(), area().center());
        // 21:9 in a 16:9 area → letterbox
        let r = dest_rect(area(), display_size(2560, 1080, 1.0, 0, None), &v, 1.0);
        assert_eq!(r.width(), 1600.0);
        assert!((r.height() - 1600.0 * 1080.0 / 2560.0).abs() <= 1.0);
    }

    #[test]
    fn fill_crops_and_stretch_fills() {
        let mut v = ViewState { fit: FitMode::Fill, ..Default::default() };
        let r = dest_rect(area(), display_size(640, 480, 1.0, 0, None), &v, 1.0);
        assert_eq!(r.width(), 1600.0);
        assert!(r.height() > 900.0);
        v.fit = FitMode::Stretch;
        assert_eq!(dest_rect(area(), display_size(640, 480, 1.0, 0, None), &v, 1.0), area());
    }

    #[test]
    fn original_is_one_to_one_on_physical_pixels() {
        let v = ViewState { fit: FitMode::Original, ..Default::default() };
        let r = dest_rect(area(), display_size(1280, 720, 1.0, 0, None), &v, 2.0);
        assert_eq!(r.size(), vec2(640.0, 360.0));
    }

    #[test]
    fn rotation_swaps_the_display_size() {
        let d0 = display_size(1920, 1080, 1.0, 0, None);
        let d90 = display_size(1920, 1080, 1.0, 90, None);
        assert_eq!(d0, vec2(1920.0, 1080.0));
        assert_eq!(d90, vec2(1080.0, 1920.0));
        assert_eq!(display_size(1920, 1080, 1.0, 180, None), d0);
        assert_eq!(display_size(1920, 1080, 1.0, 270, None), d90);
    }

    #[test]
    fn pixel_aspect_ratio_and_overrides() {
        // 720x576 PAL 4:3 anamorphic widescreen: SAR 64:45 → 16:9
        let d = display_size(720, 576, 64.0 / 45.0, 0, None);
        assert!((d.x / d.y - 16.0 / 9.0).abs() < 0.01);
        let d = display_size(1920, 1080, 1.0, 0, Some(4.0 / 3.0));
        assert!((d.x / d.y - 4.0 / 3.0).abs() < 1e-4);
        // nonsense is ignored
        let d = display_size(100, 100, f32::NAN, 0, Some(-1.0));
        assert_eq!(d, vec2(100.0, 100.0));
    }

    #[test]
    fn zoom_and_pan_move_the_picture() {
        let mut v = ViewState::default();
        let base = dest_rect(area(), vec2(1600.0, 900.0), &v, 1.0);
        v.zoom = 2.0;
        v.pan = vec2(100.0, -50.0);
        let r = dest_rect(area(), vec2(1600.0, 900.0), &v, 1.0);
        assert_eq!(r.size(), base.size() * 2.0);
        assert_eq!(r.center(), area().center() + vec2(100.0, -50.0));
        v.reset_geometry();
        assert!(v.is_neutral());
    }

    #[test]
    fn ndc_mapping() {
        let outer = Rect::from_min_size(pos2(100.0, 50.0), vec2(800.0, 400.0));
        assert_eq!(to_ndc(outer, outer), [-1.0, -1.0, 1.0, 1.0]);
        let inner = Rect::from_min_max(pos2(100.0, 50.0), pos2(500.0, 250.0)); // top-left quarter
        assert_eq!(to_ndc(inner, outer), [-1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn snapping_keeps_edges_on_pixels() {
        let v = ViewState::default();
        let r = dest_rect(Rect::from_min_size(pos2(0.0, 0.0), vec2(1001.0, 563.0)), vec2(1280.0, 720.0), &v, 1.5);
        for edge in [r.min.x, r.min.y, r.max.x, r.max.y] {
            let px = edge * 1.5;
            assert!((px - px.round()).abs() < 1e-3, "{edge} is not on the 1.5x pixel grid");
        }
    }

    #[test]
    fn ambient_bars_cover_exactly_what_the_picture_does_not() {
        let area = PxRect { x: 10, y: 20, w: 1000, h: 600 };
        // A picture filling the area leaves nothing to glow.
        assert!(ambient_bars([-1.0, -1.0, 1.0, 1.0], area).is_empty());
        // Larger than the area (zoomed in): nothing either.
        assert!(ambient_bars([-1.5, -1.5, 1.5, 1.5], area).is_empty());
        // Pillarbox: 400 px picture centred → two side bars of 300 px.
        let bars = ambient_bars([-0.4, -1.0, 0.4, 1.0], area);
        assert_eq!(bars, vec![PxRect { x: 10, y: 20, w: 300, h: 600 }, PxRect { x: 710, y: 20, w: 300, h: 600 }]);
        // Letterbox: picture 300 px tall centred → bars above and below.
        let bars = ambient_bars([-1.0, -0.5, 1.0, 0.5], area);
        assert_eq!(bars, vec![PxRect { x: 10, y: 20, w: 1000, h: 150 }, PxRect { x: 10, y: 470, w: 1000, h: 150 }]);
        // Whatever the geometry, bars and picture tile the area exactly.
        for dst in [[-0.3, -0.2, 0.9, 0.6], [0.2, 0.2, 0.4, 0.4], [-2.0, -0.5, 0.1, 2.0]] {
            let bars = ambient_bars(dst, area);
            let covered: i64 = bars.iter().map(|b| i64::from(b.w) * i64::from(b.h)).sum();
            let picture = {
                let x0 = (((dst[0] * 0.5 + 0.5) * 1000.0).round() as i32).clamp(0, 1000);
                let x1 = (((dst[2] * 0.5 + 0.5) * 1000.0).round() as i32).clamp(0, 1000);
                let y0 = (((-dst[3] * 0.5 + 0.5) * 600.0).round() as i32).clamp(0, 600);
                let y1 = (((-dst[1] * 0.5 + 0.5) * 600.0).round() as i32).clamp(0, 600);
                i64::from((x1 - x0).max(0)) * i64::from((y1 - y0).max(0))
            };
            assert_eq!(covered + picture, 1000 * 600, "{dst:?}");
            for (i, a) in bars.iter().enumerate() {
                for b in &bars[i + 1..] {
                    assert!(a.intersect(*b).is_none(), "{a:?} overlaps {b:?}");
                }
            }
        }
    }

    #[test]
    fn total_rotation_normalises() {
        assert_eq!(total_rotation(90, 270), 0);
        assert_eq!(total_rotation(-90, 0), 270);
        assert_eq!(total_rotation(0, 180), 180);
    }
}

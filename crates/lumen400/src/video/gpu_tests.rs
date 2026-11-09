//! Renders synthetic frames through the real pipeline (on whatever adapter wgpu finds — a
//! software Vulkan such as lavapipe is enough) and checks the pixels against independent maths.
//! Tests are skipped (with a message) when no adapter is available.

use eframe::wgpu;
use lumen400_engine::{ColorInfo, GamutKind, Matrix, PixelLayout, Transfer, hdr_to_sdr_rgba};

use super::gpu::*;

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    has_16bit: bool,
}

fn gpu() -> Option<Gpu> {
    let backends = wgpu::Backends::from_env().unwrap_or(if cfg!(target_os = "windows") {
        wgpu::Backends::DX12
    } else {
        wgpu::Backends::PRIMARY
    });
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = backends;
    let instance = wgpu::Instance::new(desc);
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        force_fallback_adapter: false,
        compatible_surface: None,
        ..Default::default()
    }))
    .ok()?;
    let has_16bit = supports_16bit(adapter.features());
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("test device"),
        required_features: if has_16bit { wgpu::Features::TEXTURE_FORMAT_16BIT_NORM } else { wgpu::Features::empty() },
        ..Default::default()
    }))
    .ok()?;
    Some(Gpu { device, queue, has_16bit })
}

macro_rules! need_gpu {
    () => {
        match gpu() {
            Some(g) => g,
            None => {
                eprintln!("SKIPPED: no wgpu adapter available");
                return;
            }
        }
    };
}

/// A plane of 16-bit samples stored little-endian in `d`.
fn plane16(d: &[u8], w: u32, h: u32) -> Option<PlaneView<'_>> {
    Some(PlaneView { data: d, stride: w as usize * 2, width: w, height: h })
}

fn bytes16(v: &[u16]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn color(matrix: Matrix, full: bool) -> ColorInfo {
    ColorInfo { matrix, full_range: full, transfer: Transfer::Sdr, gamut: GamutKind::Bt709 }
}

static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Renders `frame` into a `w`×`h` target and returns the RGBA pixels (row major).
fn render(g: &Gpu, frame: &FrameView, params: &DrawParams, w: u32, h: u32) -> Vec<[u8; 4]> {
    let bytes = render_to_rgba(&g.device, &g.queue, frame, params, w, h).expect("offscreen render");
    assert_eq!(bytes.len(), (w * h * 4) as usize);
    bytes.chunks_exact(4).map(|p| [p[0], p[1], p[2], p[3]]).collect()
}

fn solid_planar8(y: u8, u: u8, v: u8, c: ColorInfo) -> (Vec<u8>, Vec<u8>, Vec<u8>, ColorInfo) {
    (vec![y; 16 * 16], vec![u; 8 * 8], vec![v; 8 * 8], c)
}

fn frame8<'a>(yp: &'a [u8], up: &'a [u8], vp: &'a [u8], c: ColorInfo) -> FrameView<'a> {
    let p = |d: &'a [u8], w: u32, h: u32| Some(PlaneView { data: d, stride: w as usize, width: w, height: h });
    FrameView {
        layout: PixelLayout::Planar8,
        bit_depth: 8,
        planes: [p(yp, 16, 16), p(up, 8, 8), p(vp, 8, 8)],
        color: c,
        serial: SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
    }
}

fn near(px: [u8; 4], rgb: [i32; 3], tol: i32, what: &str) {
    for i in 0..3 {
        assert!((i32::from(px[i]) - rgb[i]).abs() <= tol, "{what}: got {px:?}, expected {rgb:?} ±{tol}");
    }
}

/// Independent reference: the textbook integer formulas for limited-range BT.709 / BT.601.
fn reference(kr: f64, kb: f64, y: f64, u: f64, v: f64) -> [i32; 3] {
    let yy = (y - 16.0) / 219.0;
    let cb = (u - 128.0) / 224.0;
    let cr = (v - 128.0) / 224.0;
    let kg = 1.0 - kr - kb;
    let r = yy + 2.0 * (1.0 - kr) * cr;
    let b = yy + 2.0 * (1.0 - kb) * cb;
    let g = (yy - kr * r - kb * b) / kg;
    [r, g, b].map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as i32)
}

#[test]
fn planar8_limited_range_matches_the_reference() {
    let g = need_gpu!();
    for (m, kr, kb) in [(Matrix::Bt709, 0.2126, 0.0722), (Matrix::Bt601, 0.299, 0.114), (Matrix::Bt2020, 0.2627, 0.0593)] {
        for (y, u, v) in [(16u8, 128u8, 128u8), (235, 128, 128), (126, 128, 128), (63, 102, 240), (173, 42, 26), (32, 240, 118), (200, 90, 160)] {
            let (yp, up, vp, c) = solid_planar8(y, u, v, color(m, false));
            let px = render(&g, &frame8(&yp, &up, &vp, c), &DrawParams::default(), 16, 16);
            near(px[8 * 16 + 8], reference(kr, kb, f64::from(y), f64::from(u), f64::from(v)), 2, &format!("{m:?} ({y},{u},{v})"));
        }
    }
}

#[test]
fn full_range_is_not_rescaled() {
    let g = need_gpu!();
    let (yp, up, vp, c) = solid_planar8(255, 128, 128, color(Matrix::Bt709, true));
    near(render(&g, &frame8(&yp, &up, &vp, c), &DrawParams::default(), 16, 16)[0], [255, 255, 255], 1, "full-range white");
    let (yp, up, vp, c) = solid_planar8(0, 128, 128, color(Matrix::Bt709, true));
    near(render(&g, &frame8(&yp, &up, &vp, c), &DrawParams::default(), 16, 16)[0], [0, 0, 0], 1, "full-range black");
    let (yp, up, vp, c) = solid_planar8(128, 128, 128, color(Matrix::Bt709, true));
    near(render(&g, &frame8(&yp, &up, &vp, c), &DrawParams::default(), 16, 16)[0], [128, 128, 128], 1, "full-range grey");
}

#[test]
fn ten_bit_planar16_matches_eight_bit() {
    let g = need_gpu!();
    if !g.has_16bit {
        eprintln!("SKIPPED: adapter has no 16-bit normalised textures");
        return;
    }
    for (y, u, v) in [(63u16, 102u16, 240u16), (173, 42, 26), (235, 128, 128), (16, 128, 128)] {
        let (y10, u10, v10) = (bytes16(&vec![y * 4; 16 * 16]), bytes16(&[u * 4; 8 * 8]), bytes16(&[v * 4; 8 * 8]));
        let f = FrameView {
            layout: PixelLayout::Planar16,
            bit_depth: 10,
            planes: [plane16(&y10, 16, 16), plane16(&u10, 8, 8), plane16(&v10, 8, 8)],
            color: color(Matrix::Bt709, false),
            serial: SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        };
        let px = render(&g, &f, &DrawParams::default(), 16, 16);
        near(px[8 * 16 + 8], reference(0.2126, 0.0722, f64::from(y), f64::from(u), f64::from(v)), 2, &format!("10-bit ({y},{u},{v})"));
    }
}

#[test]
fn p010_is_left_aligned() {
    let g = need_gpu!();
    if !g.has_16bit {
        eprintln!("SKIPPED: adapter has no 16-bit normalised textures");
        return;
    }
    // 10-bit white and mid grey, MSB aligned as P010 stores them.
    for (y10, expect) in [(940u16, 255), (502, 126)] {
        let yp = bytes16(&vec![y10 << 6; 16 * 16]);
        let uvp = bytes16(&vec![512u16 << 6; 8 * 8 * 2]);
        let f = FrameView {
            layout: PixelLayout::SemiPlanar16,
            bit_depth: 10,
            planes: [Some(PlaneView { data: &yp, stride: 32, width: 16, height: 16 }), Some(PlaneView { data: &uvp, stride: 32, width: 8, height: 8 }), None],
            color: color(Matrix::Bt709, false),
            serial: SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        };
        near(render(&g, &f, &DrawParams::default(), 16, 16)[8 * 16 + 8], [expect; 3], 2, &format!("P010 luma {y10}"));
    }
}

#[test]
fn nv12_matches_planar() {
    let g = need_gpu!();
    let yp = vec![63u8; 16 * 16];
    let mut uv = Vec::new();
    for _ in 0..8 * 8 {
        uv.extend_from_slice(&[102, 240]);
    }
    let f = FrameView {
        layout: PixelLayout::SemiPlanar8,
        bit_depth: 8,
        planes: [Some(PlaneView { data: &yp, stride: 16, width: 16, height: 16 }), Some(PlaneView { data: &uv, stride: 16, width: 8, height: 8 }), None],
        color: color(Matrix::Bt709, false),
        serial: SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
    };
    near(render(&g, &f, &DrawParams::default(), 16, 16)[8 * 16 + 8], [255, 0, 0], 3, "NV12 red");
}

#[test]
fn strided_planes_are_read_correctly() {
    let g = need_gpu!();
    // Rows are padded to 32 bytes; the padding holds a colour that must never show up.
    let mut yp = vec![250u8; 32 * 16];
    for row in 0..16 {
        for x in 0..16 {
            yp[row * 32 + x] = if x < 8 { 16 } else { 235 };
        }
    }
    let up = vec![128u8; 16 * 8];
    let vp = vec![128u8; 16 * 8];
    let f = FrameView {
        layout: PixelLayout::Planar8,
        bit_depth: 8,
        planes: [
            Some(PlaneView { data: &yp, stride: 32, width: 16, height: 16 }),
            Some(PlaneView { data: &up, stride: 16, width: 8, height: 8 }),
            Some(PlaneView { data: &vp, stride: 16, width: 8, height: 8 }),
        ],
        color: color(Matrix::Bt709, false),
        serial: SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
    };
    let px = render(&g, &f, &DrawParams::default(), 16, 16);
    near(px[3], [0, 0, 0], 2, "left half black");
    near(px[12], [255, 255, 255], 2, "right half white");
    near(px[15], [255, 255, 255], 2, "last column must not read padding");
}

fn rgba_2x1(a: [u8; 4], b: [u8; 4]) -> Vec<u8> {
    [a, b].concat()
}

fn rgba_frame<'a>(d: &'a [u8], w: u32, h: u32, transfer: Transfer) -> FrameView<'a> {
    FrameView {
        layout: PixelLayout::Rgba8,
        bit_depth: 8,
        planes: [Some(PlaneView { data: d, stride: w as usize * 4, width: w, height: h }), None, None],
        color: ColorInfo { matrix: Matrix::Bt709, full_range: true, transfer, gamut: GamutKind::Bt709 },
        serial: SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
    }
}

#[test]
fn rgba_passes_through() {
    let g = need_gpu!();
    let d = rgba_2x1([200, 100, 50, 255], [10, 20, 30, 255]);
    let px = render(&g, &rgba_frame(&d, 2, 1, Transfer::Sdr), &DrawParams::default(), 2, 1);
    near(px[0], [200, 100, 50], 1, "left");
    near(px[1], [10, 20, 30], 1, "right");
}

#[test]
fn rotation_turns_the_picture_clockwise() {
    let g = need_gpu!();
    let d = rgba_2x1([255, 0, 0, 255], [0, 0, 255, 255]); // red | blue
    let f = rgba_frame(&d, 2, 1, Transfer::Sdr);
    let px = render(&g, &f, &DrawParams { rotation: 90, ..Default::default() }, 1, 2);
    near(px[0], [255, 0, 0], 2, "90° cw: left pixel goes to the top");
    near(px[1], [0, 0, 255], 2, "90° cw: right pixel goes to the bottom");
    let f = rgba_frame(&d, 2, 1, Transfer::Sdr);
    let px = render(&g, &f, &DrawParams { rotation: 180, ..Default::default() }, 2, 1);
    near(px[0], [0, 0, 255], 2, "180°");
    let f = rgba_frame(&d, 2, 1, Transfer::Sdr);
    let px = render(&g, &f, &DrawParams { flip_h: true, ..Default::default() }, 2, 1);
    near(px[0], [0, 0, 255], 2, "mirrored");
}

#[test]
fn destination_rectangle_limits_the_picture() {
    let g = need_gpu!();
    let d = rgba_2x1([255, 255, 255, 255], [255, 255, 255, 255]);
    let f = rgba_frame(&d, 2, 1, Transfer::Sdr);
    // Draw into the right half of a 4x1 target.
    let params = DrawParams { dst: [0.0, -1.0, 1.0, 1.0], ..Default::default() };
    let px = render(&g, &f, &params, 4, 1);
    near(px[0], [0, 0, 0], 0, "outside, left");
    near(px[1], [0, 0, 0], 0, "outside, left 2");
    near(px[2], [255, 255, 255], 1, "inside");
    near(px[3], [255, 255, 255], 1, "inside 2");
}

#[test]
fn picture_adjustments() {
    let g = need_gpu!();
    let d = rgba_2x1([100, 100, 100, 255], [100, 100, 100, 255]);
    let bright = render(&g, &rgba_frame(&d, 2, 1, Transfer::Sdr), &DrawParams { adjust: Adjust { brightness: 0.2, ..Default::default() }, ..Default::default() }, 2, 1);
    near(bright[0], [151, 151, 151], 2, "brightness +0.2");
    let grey = render(&g, &rgba_frame(&d, 2, 1, Transfer::Sdr), &DrawParams { adjust: Adjust { saturation: 0.0, ..Default::default() }, ..Default::default() }, 2, 1);
    near(grey[0], [100, 100, 100], 1, "grey stays grey without saturation");
    let d = rgba_2x1([255, 0, 0, 255], [255, 0, 0, 255]);
    let desat = render(&g, &rgba_frame(&d, 2, 1, Transfer::Sdr), &DrawParams { adjust: Adjust { saturation: 0.0, ..Default::default() }, ..Default::default() }, 2, 1);
    assert!(desat[0][0].abs_diff(desat[0][1]) <= 1 && desat[0][1].abs_diff(desat[0][2]) <= 1, "{:?}", desat[0]);
}

/// ST 2084 inverse EOTF, for building test signals.
fn pq_encode(nits: f64) -> f64 {
    let (m1, m2, c1, c2, c3) = (0.1593017578125, 78.84375, 0.8359375, 18.8515625, 18.6875);
    let y = (nits / 10000.0).powf(m1);
    ((c1 + c2 * y) / (1.0 + c3 * y)).powf(m2)
}

#[test]
fn pq_is_tone_mapped_into_sdr() {
    let g = need_gpu!();
    let grey = |nits: f64| {
        let v = (pq_encode(nits) * 255.0).round() as u8;
        rgba_2x1([v, v, v, 255], [v, v, v, 255])
    };
    let level = |nits: f64| {
        let d = grey(nits);
        render(&g, &rgba_frame(&d, 2, 1, Transfer::Pq), &DrawParams::default(), 2, 1)[0][0]
    };
    let (black, shadow, diffuse, highlight, peak) = (level(0.0), level(10.0), level(203.0), level(600.0), level(1000.0));
    assert!(black <= 2, "black {black}");
    assert!(shadow > black && shadow < diffuse, "shadows {shadow}");
    assert!(diffuse > 190 && diffuse < 250, "diffuse white {diffuse}");
    assert!(highlight > diffuse, "highlights must stay distinguishable: {highlight} vs {diffuse}");
    assert!(peak >= highlight && peak >= 250, "content peak {peak}");
    // Monotonic.
    let mut last = 0;
    for n in [0.0, 1.0, 5.0, 20.0, 50.0, 100.0, 203.0, 400.0, 800.0, 1000.0, 2000.0] {
        let v = level(n);
        assert!(v >= last, "{n} nits: {v} < {last}");
        last = v;
    }
}

#[test]
fn hlg_reference_white_lands_near_sdr_white() {
    let g = need_gpu!();
    // HLG reference white is a signal of 0.75.
    let v = (0.75f64 * 255.0).round() as u8;
    let d = rgba_2x1([v, v, v, 255], [v, v, v, 255]);
    let px = render(&g, &rgba_frame(&d, 2, 1, Transfer::Hlg), &DrawParams::default(), 2, 1)[0];
    assert!(px[0] > 190 && px[0] < 250, "HLG reference white: {px:?}");
    let v = 255;
    let d = rgba_2x1([v, v, v, 255], [v, v, v, 255]);
    assert!(render(&g, &rgba_frame(&d, 2, 1, Transfer::Hlg), &DrawParams::default(), 2, 1)[0][0] >= 250);
}

#[test]
fn ambient_glow_follows_the_picture_colour() {
    let g = need_gpu!();
    let d = rgba_2x1([255, 0, 0, 255], [255, 0, 0, 255]);
    let f = rgba_frame(&d, 2, 1, Transfer::Sdr);
    // Picture in the middle quarter of a wide target; the glow fills the rest.
    let params = DrawParams { dst: [-0.25, -0.25, 0.25, 0.25], ambient: 0.5, ..Default::default() };
    let px = render(&g, &f, &params, 64, 32);
    let corner = px[0];
    assert!(corner[0] > 60 && corner[0] < 160, "glow should be a dimmed red: {corner:?}");
    assert!(corner[1] < 20 && corner[2] < 20, "{corner:?}");
    near(px[16 * 64 + 32], [255, 0, 0], 2, "picture on top of the glow");
}

#[test]
fn uploads_are_skipped_for_unchanged_frames_and_rebuilt_on_format_change() {
    let g = need_gpu!();
    let mut gpu = VideoGpu::new(&g.device, wgpu::TextureFormat::Rgba8Unorm);
    let d = rgba_2x1([1, 2, 3, 255], [4, 5, 6, 255]);
    let f = rgba_frame(&d, 2, 1, Transfer::Sdr);
    assert!(gpu.upload(&g.device, &g.queue, &f));
    assert!(gpu.has_frame());
    assert!(gpu.upload(&g.device, &g.queue, &f), "same serial is a no-op");
    let (yp, up, vp, c) = solid_planar8(16, 128, 128, color(Matrix::Bt709, false));
    assert!(gpu.upload(&g.device, &g.queue, &frame8(&yp, &up, &vp, c)), "format change rebuilds the textures");
    // A truncated plane is rejected instead of reading out of bounds.
    let short = vec![0u8; 10];
    let bad = FrameView {
        layout: PixelLayout::Planar8,
        bit_depth: 8,
        planes: [
            Some(PlaneView { data: &short, stride: 16, width: 16, height: 16 }),
            Some(PlaneView { data: &up, stride: 8, width: 8, height: 8 }),
            Some(PlaneView { data: &vp, stride: 8, width: 8, height: 8 }),
        ],
        color: color(Matrix::Bt709, false),
        serial: SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
    };
    assert!(!gpu.upload(&g.device, &g.queue, &bad));
}

fn hdr_frame<'a>(d: &'a [u8], w: u32, transfer: Transfer) -> FrameView<'a> {
    FrameView {
        layout: PixelLayout::Rgba8,
        bit_depth: 8,
        planes: [Some(PlaneView { data: d, stride: w as usize * 4, width: w, height: 1 }), None, None],
        color: ColorInfo { matrix: Matrix::Bt2020, full_range: true, transfer, gamut: GamutKind::Bt2020 },
        serial: SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
    }
}

fn pq_byte(nits: f64) -> u8 {
    (pq_encode(nits) * 255.0).round() as u8
}

/// PQ test pixels: greys, warm, green and blue, from black to 10 000 nits.
fn pq_samples() -> Vec<[u8; 4]> {
    let mut v = Vec::new();
    for nits in [0.0, 0.5, 2.0, 10.0, 50.0, 100.0, 203.0, 300.0, 600.0, 1000.0, 2000.0, 10000.0] {
        let g = pq_byte(nits);
        v.push([g, g, g, 255]);
        v.push([g, pq_byte(nits * 0.1), pq_byte(nits * 0.05), 255]);
        v.push([pq_byte(nits * 0.1), g, pq_byte(nits * 0.1), 255]);
        v.push([pq_byte(nits * 0.03), pq_byte(nits * 0.1), g, 255]);
    }
    v
}

#[test]
fn the_cpu_and_gpu_hdr_paths_agree() {
    let g = need_gpu!();
    for (transfer, samples) in [(Transfer::Pq, pq_samples()), (Transfer::Hlg, (0..=255).step_by(5).map(|v| [v as u8, (v / 2) as u8, (v / 4) as u8, 255]).collect::<Vec<_>>())] {
        let flat: Vec<u8> = samples.iter().flatten().copied().collect();
        let w = samples.len() as u32;
        let gpu_px = render(&g, &hdr_frame(&flat, w, transfer), &DrawParams::default(), w, 1);
        let mut cpu = flat.clone();
        hdr_to_sdr_rgba(&mut cpu, transfer, GamutKind::Bt2020);
        for (i, px) in gpu_px.iter().enumerate() {
            for c in 0..3 {
                let (a, b) = (i32::from(px[c]), i32::from(cpu[i * 4 + c]));
                assert!((a - b).abs() <= 3, "{transfer:?} sample {i} {:?}: gpu {px:?} vs cpu {:?}", samples[i], &cpu[i * 4..i * 4 + 3]);
            }
        }
    }
}

#[test]
fn bright_saturated_hdr_colours_keep_their_hue() {
    let g = need_gpu!();
    let d: Vec<u8> = [[pq_byte(600.0), pq_byte(5.0), pq_byte(5.0), 255], [pq_byte(5.0), pq_byte(5.0), pq_byte(500.0), 255]].concat();
    let px = render(&g, &hdr_frame(&d, 2, Transfer::Pq), &DrawParams::default(), 2, 1);
    assert!(px[0][0] > 200 && px[0][1] < 60 && px[0][2] < 60, "red turned into {:?}", px[0]);
    assert!(px[1][2] > 150 && px[1][0] < 60 && px[1][1] < 80, "blue turned into {:?}", px[1]);
}

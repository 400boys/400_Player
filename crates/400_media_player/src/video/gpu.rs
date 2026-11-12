//! GPU side of the video renderer: plane textures, pipelines and draw calls.
//!
//! Everything here works on a plain [`FrameView`] (borrowed planes) so it can be tested without an
//! engine frame, and drawn either by egui's paint callback or into an offscreen target.

use std::num::NonZeroU64;

use eframe::wgpu::{self, util::DeviceExt};
use media_player_400_engine::{ColorInfo, PixelLayout, VideoFrame};

use super::color;

const SHADER: &str = include_str!("shader.wgsl");

/// Size of the tiny texture the ambient glow is derived from.
pub const AMBIENT_SIZE: (u32, u32) = (32, 18);

/// One plane of picture data.
#[derive(Clone, Copy)]
pub struct PlaneView<'a> {
    pub data: &'a [u8],
    /// Bytes per row.
    pub stride: usize,
    /// Texels per row / number of rows.
    pub width: u32,
    pub height: u32,
}

/// A decoded picture as the renderer sees it.
pub struct FrameView<'a> {
    pub layout: PixelLayout,
    pub bit_depth: u8,
    pub planes: [Option<PlaneView<'a>>; 3],
    pub color: ColorInfo,
    /// Changes whenever the picture changes (skips redundant uploads).
    pub serial: u64,
}

impl<'a> FrameView<'a> {
    pub fn from_frame(f: &'a VideoFrame) -> Self {
        let mut planes = [None, None, None];
        for (i, slot) in planes.iter_mut().enumerate().take(f.layout.planes()) {
            let p = f.plane(i);
            *slot = Some(PlaneView { data: p.data, stride: p.stride, width: p.width, height: p.height });
        }
        Self { layout: f.layout, bit_depth: f.bit_depth, planes, color: f.color, serial: f.serial }
    }
}

/// Everything the shader reads, mirrored from `Params` in `shader.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Uniforms {
    pub dst: [f32; 4],
    pub uvm: [f32; 4],
    pub row0: [f32; 4],
    pub row1: [f32; 4],
    pub row2: [f32; 4],
    pub gam0: [f32; 4],
    pub gam1: [f32; 4],
    pub gam2: [f32; 4],
    pub range: [f32; 4],
    pub misc: [f32; 4],
    pub adjust: [f32; 4],
    pub flags: [f32; 4],
    pub tgt: [f32; 4],
}

/// User-facing picture settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Adjust {
    pub brightness: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub gamma: f32,
}

impl Default for Adjust {
    fn default() -> Self {
        Self { brightness: 0.0, contrast: 1.0, saturation: 1.0, gamma: 1.0 }
    }
}

impl Adjust {
    pub fn is_identity(&self) -> bool {
        *self == Self::default()
    }
}

/// How a frame is drawn: where, how rotated, how bright…
#[derive(Clone, Copy, Debug)]
pub struct DrawParams {
    /// Destination of the picture in NDC of the paint rect: `[left, bottom, right, top]`.
    pub dst: [f32; 4],
    pub rotation: u16,
    pub flip_h: bool,
    pub adjust: Adjust,
    /// Glow behind the picture, `0.0` = off.
    pub ambient: f32,
    pub opacity: f32,
    /// Peak luminance of HDR content in units of SDR white (203 nits).
    pub hdr_peak: f32,
}

impl Default for DrawParams {
    fn default() -> Self {
        Self { dst: [-1.0, -1.0, 1.0, 1.0], rotation: 0, flip_h: false, adjust: Adjust::default(), ambient: 0.0, opacity: 1.0, hdr_peak: 1000.0 / 203.0 }
    }
}

/// Builds the uniform block for a frame. `full` is the same block with the destination covering
/// the whole paint rect (used by the ambient passes).
pub fn uniforms(frame: &FrameView, p: &DrawParams, target_srgb: bool) -> (Uniforms, Uniforms) {
    let c = &frame.color;
    let yuv = color::yuv_to_rgb(c.matrix);
    let gam = if color::wide_gamut(c) { color::GAMUT_2020_TO_709 } else { color::IDENTITY };
    let high_bit = frame.bit_depth > 8;
    let u = Uniforms {
        dst: p.dst,
        uvm: color::uv_matrix(p.rotation, p.flip_h),
        row0: yuv[0],
        row1: yuv[1],
        row2: yuv[2],
        gam0: gam[0],
        gam1: gam[1],
        gam2: gam[2],
        range: color::range_params(c.full_range, frame.bit_depth),
        misc: [color::layout_code(frame.layout), color::transfer_code(c.transfer), p.hdr_peak.max(1.0), color::bit_scale(frame.layout, frame.bit_depth)],
        adjust: [p.adjust.brightness, p.adjust.contrast, p.adjust.saturation, p.adjust.gamma.max(0.1)],
        flags: [if color::wide_gamut(c) { 1.0 } else { 0.0 }, if high_bit { 1.5 / 255.0 } else { 0.0 }, p.opacity, p.ambient],
        tgt: [if target_srgb { 1.0 } else { 0.0 }, AMBIENT_SIZE.0 as f32, AMBIENT_SIZE.1 as f32, 0.0],
    };
    let full = Uniforms { dst: [-1.0, -1.0, 1.0, 1.0], ..u };
    (u, full)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct FrameKey {
    layout: PixelLayout,
    dims: [(u32, u32); 3],
}

struct FrameGpu {
    key: FrameKey,
    textures: Vec<wgpu::Texture>,
    bg_main: wgpu::BindGroup,
    bg_src: wgpu::BindGroup,
    bg_amb: wgpu::BindGroup,
}

/// Pipelines and per-frame resources of the video renderer. Lives in egui's callback resources.
pub struct VideoGpu {
    bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    pipe_video: wgpu::RenderPipeline,
    pipe_amb_src: wgpu::RenderPipeline,
    pipe_amb_draw: wgpu::RenderPipeline,
    ubuf_main: wgpu::Buffer,
    ubuf_full: wgpu::Buffer,
    dummy: wgpu::TextureView,
    amb_view: wgpu::TextureView,
    frame: Option<FrameGpu>,
    uploaded_serial: u64,
    /// The ambient texture must be re-rendered (new picture or new settings).
    amb_dirty: bool,
    target_srgb: bool,
    /// Hash of the settings the ambient texture depends on.
    last_look: u64,
    /// Microseconds the last upload took (for the statistics overlay).
    pub last_upload_us: u32,
}

fn plane_formats(layout: PixelLayout) -> [Option<wgpu::TextureFormat>; 3] {
    use wgpu::TextureFormat as F;
    match layout {
        PixelLayout::Planar8 => [Some(F::R8Unorm); 3],
        PixelLayout::SemiPlanar8 => [Some(F::R8Unorm), Some(F::Rg8Unorm), None],
        PixelLayout::Planar16 => [Some(F::R16Unorm); 3],
        PixelLayout::SemiPlanar16 => [Some(F::R16Unorm), Some(F::Rg16Unorm), None],
        PixelLayout::Rgba8 => [Some(F::Rgba8Unorm), None, None],
    }
}

fn bytes_per_texel(f: wgpu::TextureFormat) -> usize {
    use wgpu::TextureFormat as F;
    match f {
        F::R8Unorm => 1,
        F::Rg8Unorm | F::R16Unorm => 2,
        F::Rg16Unorm | F::Rgba8Unorm => 4,
        _ => 4,
    }
}

/// `true` when the device can sample 16-bit normalised textures (10/12-bit video without conversion).
pub fn supports_16bit(features: wgpu::Features) -> bool {
    features.contains(wgpu::Features::TEXTURE_FORMAT_16BIT_NORM)
}

fn tex_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture { multisampled: false, sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2 },
        count: None,
    }
}

impl VideoGpu {
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("400_media_player video shader"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("400_media_player video bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(std::mem::size_of::<Uniforms>() as u64),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                tex_entry(2),
                tex_entry(3),
                tex_entry(4),
                tex_entry(5),
            ],
        });
        let layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("400_media_player video pipeline layout"), bind_group_layouts: &[Some(&bgl)], immediate_size: 0 });
        let blend = Some(wgpu::BlendState {
            color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha, operation: wgpu::BlendOperation::Add },
            alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha, operation: wgpu::BlendOperation::Add },
        });
        let make = |label: &str, fs: &str, format: wgpu::TextureFormat, blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState { module: &module, entry_point: Some("vs_quad"), compilation_options: Default::default(), buffers: &[] },
                primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let pipe_video = make("400_media_player video", "fs_video", target_format, blend);
        let pipe_amb_src = make("400_media_player ambient source", "fs_ambient_src", wgpu::TextureFormat::Rgba8Unorm, None);
        let pipe_amb_draw = make("400_media_player ambient glow", "fs_ambient_draw", target_format, blend);

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("400_media_player video sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let ubuf = |label: &str| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytemuck::bytes_of(&Uniforms::default()),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            })
        };
        let dummy_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("400_media_player dummy"),
            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let amb_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("400_media_player ambient"),
            size: wgpu::Extent3d { width: AMBIENT_SIZE.0, height: AMBIENT_SIZE.1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        Self {
            bgl,
            sampler,
            pipe_video,
            pipe_amb_src,
            pipe_amb_draw,
            ubuf_main: ubuf("400_media_player params"),
            ubuf_full: ubuf("400_media_player params (full)"),
            dummy: dummy_tex.create_view(&Default::default()),
            amb_view: amb_tex.create_view(&Default::default()),
            frame: None,
            uploaded_serial: 0,
            amb_dirty: true,
            target_srgb: target_format.is_srgb(),
            last_look: 0,
            last_upload_us: 0,
        }
    }

    pub fn target_is_srgb(&self) -> bool {
        self.target_srgb
    }

    /// `true` when a picture has been uploaded (tests).
    #[cfg(test)]
    pub fn has_frame(&self) -> bool {
        self.frame.is_some()
    }

    fn bind_group(&self, device: &wgpu::Device, ubuf: &wgpu::Buffer, views: &[wgpu::TextureView; 3], amb: &wgpu::TextureView) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("400_media_player video bind group"),
            layout: &self.bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&views[0]) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&views[1]) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&views[2]) },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(amb) },
            ],
        })
    }

    fn rebuild(&mut self, device: &wgpu::Device, frame: &FrameView) -> bool {
        let formats = plane_formats(frame.layout);
        let mut dims = [(0, 0); 3];
        let mut textures = Vec::new();
        for (slot, (fmt, plane)) in dims.iter_mut().zip(formats.iter().zip(frame.planes.iter())) {
            let Some(fmt) = *fmt else { continue };
            let Some(p) = *plane else { return false };
            *slot = (p.width, p.height);
            textures.push(device.create_texture(&wgpu::TextureDescriptor {
                label: Some("400_media_player plane"),
                size: wgpu::Extent3d { width: p.width.max(1), height: p.height.max(1), depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: fmt,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            }));
        }
        // Unused planes are bound to a 1x1 dummy so one layout fits every pixel format.
        let view_of = |i: usize| -> wgpu::TextureView { textures.get(i).map_or_else(|| self.dummy.clone(), |t| t.create_view(&Default::default())) };
        let views = [view_of(0), view_of(1), view_of(2)];
        let bg_main = self.bind_group(device, &self.ubuf_main, &views, &self.dummy);
        let bg_src = self.bind_group(device, &self.ubuf_full, &views, &self.dummy);
        let bg_amb = self.bind_group(device, &self.ubuf_full, &views, &self.amb_view);
        self.frame = Some(FrameGpu { key: FrameKey { layout: frame.layout, dims }, textures, bg_main, bg_src, bg_amb });
        true
    }

    /// Uploads the planes of `frame` (a no-op when the same picture is already on the GPU).
    /// Returns `false` when the frame could not be used.
    pub fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: &FrameView) -> bool {
        if self.frame.is_some() && frame.serial == self.uploaded_serial {
            return true;
        }
        let t0 = std::time::Instant::now();
        let formats = plane_formats(frame.layout);
        let mut dims = [(0, 0); 3];
        for (slot, plane) in dims.iter_mut().zip(frame.planes.iter()) {
            if let Some(p) = plane {
                *slot = (p.width, p.height);
            }
        }
        let key = FrameKey { layout: frame.layout, dims };
        if self.frame.as_ref().is_none_or(|f| f.key != key) && !self.rebuild(device, frame) {
            return false;
        }
        let Some(gpu) = self.frame.as_ref() else { return false };
        let mut ti = 0;
        for (i, (fmt, plane)) in formats.iter().zip(frame.planes.iter()).enumerate() {
            let (Some(fmt), Some(p)) = (*fmt, *plane) else { continue };
            let bpt = bytes_per_texel(fmt);
            let row_bytes = p.width as usize * bpt;
            let needed = p.stride * (p.height as usize).saturating_sub(1) + row_bytes;
            if p.stride < row_bytes || p.data.len() < needed {
                tracing::warn!("plane {i}: {} bytes for {}x{} (stride {})", p.data.len(), p.width, p.height, p.stride);
                return false;
            }
            queue.write_texture(
                wgpu::TexelCopyTextureInfo { texture: &gpu.textures[ti], mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                &p.data[..needed],
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(p.stride as u32), rows_per_image: Some(p.height) },
                wgpu::Extent3d { width: p.width, height: p.height, depth_or_array_layers: 1 },
            );
            ti += 1;
        }
        self.uploaded_serial = frame.serial;
        self.amb_dirty = true;
        self.last_upload_us = t0.elapsed().as_micros().min(u128::from(u32::MAX)) as u32;
        true
    }

    /// Writes the uniform blocks (call once per paint, after [`Self::upload`]).
    pub fn set_uniforms(&mut self, queue: &wgpu::Queue, main: &Uniforms, full: &Uniforms) {
        queue.write_buffer(&self.ubuf_main, 0, bytemuck::bytes_of(main));
        queue.write_buffer(&self.ubuf_full, 0, bytemuck::bytes_of(full));
    }

    /// Re-renders the tiny ambient texture when the picture changed.
    pub fn prepare_ambient(&mut self, encoder: &mut wgpu::CommandEncoder, enabled: bool) {
        if !enabled || !self.amb_dirty {
            return;
        }
        let Some(f) = self.frame.as_ref() else { return };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("400_media_player ambient"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.amb_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipe_amb_src);
        pass.set_bind_group(0, &f.bg_src, &[]);
        pass.draw(0..4, 0..1);
        drop(pass);
        self.amb_dirty = false;
    }

    /// Marks the ambient texture stale (rotation or colour settings changed).
    pub fn invalidate_ambient(&mut self) {
        self.amb_dirty = true;
    }

    /// Records the look settings; the ambient texture is refreshed when they changed.
    pub fn note_look(&mut self, p: &DrawParams) {
        let mut h = u64::from(p.rotation) << 1 | u64::from(p.flip_h);
        for v in [p.adjust.brightness, p.adjust.contrast, p.adjust.saturation, p.adjust.gamma, p.hdr_peak] {
            h = h.wrapping_mul(0x100_0000_01b3).wrapping_add(u64::from(v.to_bits()));
        }
        if h != self.last_look {
            self.last_look = h;
            self.amb_dirty = true;
        }
    }

    pub fn draw_ambient(&self, pass: &mut wgpu::RenderPass<'_>) {
        if let Some(f) = self.frame.as_ref() {
            pass.set_pipeline(&self.pipe_amb_draw);
            pass.set_bind_group(0, &f.bg_amb, &[]);
            pass.draw(0..4, 0..1);
        }
    }

    pub fn draw_video(&self, pass: &mut wgpu::RenderPass<'_>) {
        if let Some(f) = self.frame.as_ref() {
            pass.set_pipeline(&self.pipe_video);
            pass.set_bind_group(0, &f.bg_main, &[]);
            pass.draw(0..4, 0..1);
        }
    }
}

/// Renders `frame` into a `width`×`height` RGBA8 image (rows tightly packed) on the given device,
/// with a pipeline of its own. This is the screenshot path — it produces exactly the pixels the
/// window shows (colour conversion, HDR tone mapping, rotation) — and the way the tests read the
/// shader's output back.
pub fn render_to_rgba(device: &wgpu::Device, queue: &wgpu::Queue, frame: &FrameView, params: &DrawParams, width: u32, height: u32) -> Option<Vec<u8>> {
    if width == 0 || height == 0 || width > 16_384 || height > 16_384 {
        return None;
    }
    let mut gpu = VideoGpu::new(device, wgpu::TextureFormat::Rgba8Unorm);
    if !gpu.upload(device, queue, frame) {
        return None;
    }
    let (main, full) = uniforms(frame, params, false);
    gpu.set_uniforms(queue, &main, &full);
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("400_media_player offscreen target"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    // Buffer copies need rows padded to 256 bytes.
    let row = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("400_media_player readback"),
        size: u64::from(row) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&Default::default());
    let glow = params.ambient > 0.0;
    gpu.prepare_ambient(&mut enc, glow);
    {
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("400_media_player offscreen pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if glow {
            gpu.draw_ambient(&mut pass);
        }
        gpu.draw_video(&mut pass);
    }
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo { texture: &target, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(height) } },
        wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
    );
    queue.submit([enc.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    buf.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
    rx.recv().ok()?.ok()?;
    let mapped = buf.slice(..).get_mapped_range().ok()?;
    let mut out = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height as usize {
        out.extend_from_slice(&mapped[y * row as usize..y * row as usize + width as usize * 4]);
    }
    drop(mapped);
    buf.unmap();
    Some(out)
}

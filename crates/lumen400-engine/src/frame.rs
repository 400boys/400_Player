//! Decoded video frames as handed to the renderer, pure-Rust pixel buffers, colour conversion,
//! scaling, and the pixel-format policy.

/// Internal pixel formats decoded or converted by the engine.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub enum Pixel {
    #[default]
    YUV420P,
    YUV422P,
    YUV444P,
    YUVJ420P,
    YUVJ422P,
    YUVJ444P,
    NV12,
    YUV420P10LE,
    YUV422P10LE,
    YUV444P10LE,
    YUV420P10BE,
    YUV420P12LE,
    YUV422P12LE,
    YUV444P12LE,
    P010LE,
    P016LE,
    RGBA,
    RGB24,
    PAL8,
    GRAY8,
    YUVA420P,
}

#[allow(dead_code)]
impl Pixel {
    pub fn from_name(s: &str) -> Self {
        match s {
            "yuv422p" => Self::YUV422P,
            "yuv444p" => Self::YUV444P,
            "yuvj420p" => Self::YUVJ420P,
            "yuvj422p" => Self::YUVJ422P,
            "yuvj444p" => Self::YUVJ444P,
            "nv12" => Self::NV12,
            "yuv420p10le" => Self::YUV420P10LE,
            "yuv422p10le" => Self::YUV422P10LE,
            "yuv444p10le" => Self::YUV444P10LE,
            "yuv420p10be" => Self::YUV420P10BE,
            "yuv420p12le" => Self::YUV420P12LE,
            "yuv422p12le" => Self::YUV422P12LE,
            "yuv444p12le" => Self::YUV444P12LE,
            "p010le" => Self::P010LE,
            "p016le" => Self::P016LE,
            "rgba" => Self::RGBA,
            "rgb24" => Self::RGB24,
            "pal8" => Self::PAL8,
            "gray" => Self::GRAY8,
            "yuva420p" => Self::YUVA420P,
            _ => Self::YUV420P,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::YUV420P => "yuv420p",
            Self::YUV422P => "yuv422p",
            Self::YUV444P => "yuv444p",
            Self::YUVJ420P => "yuvj420p",
            Self::YUVJ422P => "yuvj422p",
            Self::YUVJ444P => "yuvj444p",
            Self::NV12 => "nv12",
            Self::YUV420P10LE => "yuv420p10le",
            Self::YUV422P10LE => "yuv422p10le",
            Self::YUV444P10LE => "yuv444p10le",
            Self::YUV420P10BE => "yuv420p10be",
            Self::YUV420P12LE => "yuv420p12le",
            Self::YUV422P12LE => "yuv422p12le",
            Self::YUV444P12LE => "yuv444p12le",
            Self::P010LE => "p010le",
            Self::P016LE => "p016le",
            Self::RGBA => "rgba",
            Self::RGB24 => "rgb24",
            Self::PAL8 => "pal8",
            Self::GRAY8 => "gray",
            Self::YUVA420P => "yuva420p",
        }
    }
}

/// Colour space (matrix coefficients) signalled by the stream.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Space {
    #[default]
    Unspecified,
    BT709,
    BT470BG,
    SMPTE170M,
    FCC,
    BT2020NCL,
    BT2020CL,
    SMPTE240M,
}

#[allow(dead_code)]
impl Space {
    pub fn from_code(c: u8) -> Self {
        match c {
            1 => Self::BT709,
            5 => Self::BT470BG,
            6 => Self::SMPTE170M,
            4 => Self::FCC,
            9 => Self::BT2020NCL,
            10 => Self::BT2020CL,
            7 => Self::SMPTE240M,
            _ => Self::Unspecified,
        }
    }
    pub fn name(self) -> Option<&'static str> {
        match self {
            Self::Unspecified => None,
            Self::BT709 => Some("bt709"),
            Self::BT470BG => Some("bt470bg"),
            Self::SMPTE170M => Some("smpte170m"),
            Self::FCC => Some("fcc"),
            Self::BT2020NCL => Some("bt2020nc"),
            Self::BT2020CL => Some("bt2020c"),
            Self::SMPTE240M => Some("smpte240m"),
        }
    }
}

/// Sample range signalled by the stream.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Range {
    #[default]
    Unspecified,
    MPEG,
    JPEG,
}

/// Transfer characteristic (EOTF) signalled by the stream.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(dead_code)]
pub enum TransferCharacteristic {
    #[default]
    Unspecified,
    BT709,
    SMPTE2084,
    #[allow(non_camel_case_types)]
    ARIB_STD_B67,
}

/// Colour primaries signalled by the stream.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Primaries {
    #[default]
    Unspecified,
    BT709,
    BT2020,
}

#[allow(dead_code)]
impl Range {
    pub fn from_code(c: u8) -> Self {
        match c {
            1 => Self::MPEG,
            2 => Self::JPEG,
            _ => Self::Unspecified,
        }
    }
}

#[allow(dead_code)]
impl TransferCharacteristic {
    pub fn from_code(c: u8) -> Self {
        match c {
            1 => Self::BT709,
            16 => Self::SMPTE2084,
            18 => Self::ARIB_STD_B67,
            _ => Self::Unspecified,
        }
    }
}

#[allow(dead_code)]
impl Primaries {
    pub fn from_code(c: u8) -> Self {
        match c {
            1 => Self::BT709,
            9 => Self::BT2020,
            _ => Self::Unspecified,
        }
    }
}

/// How the planes of a frame are laid out in memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelLayout {
    /// Y, U, V as three 8-bit planes (4:2:0, 4:2:2 or 4:4:4).
    Planar8,
    /// Y plane + interleaved UV plane, 8-bit (NV12).
    SemiPlanar8,
    /// Y, U, V as three 16-bit little-endian planes holding 10/12-bit samples (LSB aligned).
    Planar16,
    /// Y plane + interleaved UV plane, 16-bit little-endian, MSB aligned (P010 / P016).
    SemiPlanar16,
    /// One packed RGBA plane, 8-bit.
    Rgba8,
}

impl PixelLayout {
    /// Number of planes uploaded to the GPU.
    pub fn planes(self) -> usize {
        match self {
            Self::Planar8 | Self::Planar16 => 3,
            Self::SemiPlanar8 | Self::SemiPlanar16 => 2,
            Self::Rgba8 => 1,
        }
    }
    /// Bytes per sample of the first plane.
    pub fn bytes_per_sample(self) -> usize {
        match self {
            Self::Planar16 | Self::SemiPlanar16 => 2,
            _ => 1,
        }
    }
    pub fn is_high_bit_depth(self) -> bool {
        matches!(self, Self::Planar16 | Self::SemiPlanar16)
    }
}

/// YUV → RGB matrix family.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Matrix {
    Bt601,
    Bt709,
    Bt2020,
    Smpte240,
}

/// Transfer function of the signal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transfer {
    Sdr,
    /// SMPTE ST 2084 (HDR10 / Dolby Vision base layer).
    Pq,
    /// ARIB STD-B67 (Hybrid Log-Gamma).
    Hlg,
}

/// Colour primaries (only the two gamuts that matter in practice).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GamutKind {
    Bt709,
    Bt2020,
}

/// Everything the shader needs to interpret the samples.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColorInfo {
    pub matrix: Matrix,
    pub full_range: bool,
    pub transfer: Transfer,
    pub gamut: GamutKind,
}

/// One plane of a frame.
#[derive(Clone, Copy, Debug)]
pub struct Plane<'a> {
    /// Raw bytes, `stride * height` long.
    pub data: &'a [u8],
    /// Bytes per row.
    pub stride: usize,
    /// Width in *samples* (texels for the GPU; an interleaved UV plane counts pairs).
    pub width: u32,
    pub height: u32,
}

/// Owned plane storage inside [`RawVideoFrame`].
#[derive(Clone, Debug, Default)]
pub(crate) struct PlaneBuf {
    pub data: Vec<u8>,
    pub stride: usize,
    pub width: u32,
    pub height: u32,
}

/// Pure-Rust decoded picture buffer.
#[derive(Clone, Debug, Default)]
pub struct RawVideoFrame {
    format: Pixel,
    width: u32,
    height: u32,
    planes: Vec<PlaneBuf>,
    pts: Option<i64>,
    best_effort_timestamp: Option<i64>,
    sar: (i32, i32),
    color_space: Space,
    color_range: Range,
    color_trc: TransferCharacteristic,
    color_primaries: Primaries,
    interlaced: bool,
    top_field_first: bool,
}

impl RawVideoFrame {
    #[allow(dead_code)]
    pub fn empty() -> Self {
        Self { sar: (1, 1), ..Default::default() }
    }

    pub fn new(format: Pixel, width: u32, height: u32) -> Self {
        let cw = width.div_ceil(2);
        let ch = height.div_ceil(2);
        let plane = |w: u32, h: u32, bpp: usize, fill: u8| {
            let stride = w as usize * bpp;
            PlaneBuf { data: vec![fill; stride * h as usize], stride, width: w, height: h }
        };
        let planes = match format {
            Pixel::YUV420P | Pixel::YUVJ420P => {
                vec![plane(width, height, 1, 16), plane(cw, ch, 1, 128), plane(cw, ch, 1, 128)]
            }
            Pixel::YUV422P | Pixel::YUVJ422P => {
                vec![plane(width, height, 1, 16), plane(cw, height, 1, 128), plane(cw, height, 1, 128)]
            }
            Pixel::YUV444P | Pixel::YUVJ444P => {
                vec![plane(width, height, 1, 16), plane(width, height, 1, 128), plane(width, height, 1, 128)]
            }
            Pixel::NV12 => {
                vec![plane(width, height, 1, 16), plane(cw, ch, 2, 128)]
            }
            Pixel::YUV420P10LE | Pixel::YUV420P12LE | Pixel::YUV420P10BE => {
                vec![plane(width, height, 2, 0), plane(cw, ch, 2, 0), plane(cw, ch, 2, 0)]
            }
            Pixel::YUV422P10LE | Pixel::YUV422P12LE => {
                vec![plane(width, height, 2, 0), plane(cw, height, 2, 0), plane(cw, height, 2, 0)]
            }
            Pixel::YUV444P10LE | Pixel::YUV444P12LE => {
                vec![plane(width, height, 2, 0), plane(width, height, 2, 0), plane(width, height, 2, 0)]
            }
            Pixel::P010LE | Pixel::P016LE => {
                vec![plane(width, height, 2, 0), plane(cw, ch, 4, 0)]
            }
            Pixel::RGBA => vec![plane(width, height, 4, 0)],
            Pixel::RGB24 => vec![plane(width, height, 3, 0)],
            Pixel::GRAY8 => vec![plane(width, height, 1, 0)],
            Pixel::PAL8 => vec![plane(width, height, 1, 0), plane(256, 1, 4, 0)],
            Pixel::YUVA420P => {
                vec![
                    plane(width, height, 1, 16),
                    plane(cw, ch, 1, 128),
                    plane(cw, ch, 1, 128),
                    plane(width, height, 1, 255),
                ]
            }
        };
        Self {
            format,
            width,
            height,
            planes,
            pts: None,
            best_effort_timestamp: None,
            sar: (1, 1),
            color_space: Space::Unspecified,
            color_range: Range::Unspecified,
            color_trc: TransferCharacteristic::Unspecified,
            color_primaries: Primaries::Unspecified,
            interlaced: false,
            top_field_first: true,
        }
    }

    pub fn from_planar_buffers(
        format: Pixel,
        width: u32,
        height: u32,
        buffers: Vec<(Vec<u8>, usize, u32, u32)>,
    ) -> Self {
        let planes = buffers
            .into_iter()
            .map(|(data, stride, width, height)| PlaneBuf { data, stride, width, height })
            .collect();
        Self {
            format,
            width,
            height,
            planes,
            pts: None,
            best_effort_timestamp: None,
            sar: (1, 1),
            color_space: Space::Unspecified,
            color_range: Range::Unspecified,
            color_trc: TransferCharacteristic::Unspecified,
            color_primaries: Primaries::Unspecified,
            interlaced: false,
            top_field_first: true,
        }
    }

    pub fn format(&self) -> Pixel {
        self.format
    }
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn planes_count(&self) -> usize {
        self.planes.len()
    }
    pub fn stride(&self, i: usize) -> usize {
        self.planes.get(i).map_or(0, |p| p.stride)
    }
    pub fn plane_width(&self, i: usize) -> u32 {
        self.planes.get(i).map_or(0, |p| p.width)
    }
    pub fn plane_height(&self, i: usize) -> u32 {
        self.planes.get(i).map_or(0, |p| p.height)
    }
    pub fn data(&self, i: usize) -> &[u8] {
        self.planes.get(i).map_or(&[], |p| &p.data)
    }
    pub fn data_mut(&mut self, i: usize) -> &mut [u8] {
        &mut self.planes[i].data
    }
    #[allow(dead_code)]
    pub fn pts(&self) -> Option<i64> {
        self.pts
    }
    pub fn set_pts(&mut self, pts: Option<i64>) {
        self.pts = pts;
        self.best_effort_timestamp = pts;
    }
    pub fn timestamp(&self) -> Option<i64> {
        self.best_effort_timestamp.or(self.pts)
    }
    pub fn aspect_ratio(&self) -> (i32, i32) {
        self.sar
    }
    pub fn set_aspect_ratio(&mut self, sar: (i32, i32)) {
        self.sar = sar;
    }
    pub fn is_interlaced(&self) -> bool {
        self.interlaced
    }
    pub fn top_field_first(&self) -> bool {
        self.top_field_first
    }
    pub fn set_interlaced(&mut self, interlaced: bool, tff: bool) {
        self.interlaced = interlaced;
        self.top_field_first = tff;
    }
    pub fn color_space(&self) -> Space {
        self.color_space
    }
    pub fn set_color_space(&mut self, s: Space) {
        self.color_space = s;
    }
    pub fn color_range(&self) -> Range {
        self.color_range
    }
    pub fn set_color_range(&mut self, r: Range) {
        self.color_range = r;
    }
    pub fn color_transfer_characteristic(&self) -> TransferCharacteristic {
        self.color_trc
    }
    pub fn set_color_transfer_characteristic(&mut self, t: TransferCharacteristic) {
        self.color_trc = t;
    }
    pub fn color_primaries(&self) -> Primaries {
        self.color_primaries
    }
    pub fn set_color_primaries(&mut self, p: Primaries) {
        self.color_primaries = p;
    }

    /// Reads one pixel at `(x, y)` as 8-bit `(Y, U, V)` or `(R, G, B, A)` depending on format.
    fn sample_yuv8(&self, x: u32, y: u32) -> (u8, u8, u8) {
        let x = x.min(self.width.saturating_sub(1)) as usize;
        let y = y.min(self.height.saturating_sub(1)) as usize;
        match self.format {
            Pixel::YUV420P | Pixel::YUVJ420P | Pixel::YUVA420P => {
                let yv = self.planes[0].data[y * self.planes[0].stride + x];
                let uv_x = x / 2;
                let uv_y = y / 2;
                let u = self.planes[1].data[uv_y * self.planes[1].stride + uv_x];
                let v = self.planes[2].data[uv_y * self.planes[2].stride + uv_x];
                (yv, u, v)
            }
            Pixel::YUV422P | Pixel::YUVJ422P => {
                let yv = self.planes[0].data[y * self.planes[0].stride + x];
                let uv_x = x / 2;
                let u = self.planes[1].data[y * self.planes[1].stride + uv_x];
                let v = self.planes[2].data[y * self.planes[2].stride + uv_x];
                (yv, u, v)
            }
            Pixel::YUV444P | Pixel::YUVJ444P => {
                let yv = self.planes[0].data[y * self.planes[0].stride + x];
                let u = self.planes[1].data[y * self.planes[1].stride + x];
                let v = self.planes[2].data[y * self.planes[2].stride + x];
                (yv, u, v)
            }
            Pixel::NV12 => {
                let yv = self.planes[0].data[y * self.planes[0].stride + x];
                let uv_x = (x / 2) * 2;
                let uv_y = y / 2;
                let base = uv_y * self.planes[1].stride + uv_x;
                let u = self.planes[1].data[base];
                let v = self.planes[1].data[base + 1];
                (yv, u, v)
            }
            Pixel::GRAY8 => {
                let yv = self.planes[0].data[y * self.planes[0].stride + x];
                (yv, 128, 128)
            }
            Pixel::YUV420P10LE | Pixel::YUV422P10LE | Pixel::YUV444P10LE => {
                let read10 = |p: &PlaneBuf, px: usize, py: usize| -> u8 {
                    let idx = py * p.stride + px * 2;
                    let v = u16::from_le_bytes([p.data[idx], p.data[idx + 1]]);
                    (v >> 2).min(255) as u8
                };
                let (cx, cy) = match self.format {
                    Pixel::YUV420P10LE => (x / 2, y / 2),
                    Pixel::YUV422P10LE => (x / 2, y),
                    _ => (x, y),
                };
                (read10(&self.planes[0], x, y), read10(&self.planes[1], cx, cy), read10(&self.planes[2], cx, cy))
            }
            Pixel::YUV420P10BE => {
                let read10 = |p: &PlaneBuf, px: usize, py: usize| -> u8 {
                    let idx = py * p.stride + px * 2;
                    let v = u16::from_be_bytes([p.data[idx], p.data[idx + 1]]);
                    (v >> 2).min(255) as u8
                };
                (read10(&self.planes[0], x, y), read10(&self.planes[1], x / 2, y / 2), read10(&self.planes[2], x / 2, y / 2))
            }
            Pixel::YUV420P12LE | Pixel::YUV422P12LE | Pixel::YUV444P12LE => {
                let read12 = |p: &PlaneBuf, px: usize, py: usize| -> u8 {
                    let idx = py * p.stride + px * 2;
                    let v = u16::from_le_bytes([p.data[idx], p.data[idx + 1]]);
                    (v >> 4).min(255) as u8
                };
                let (cx, cy) = match self.format {
                    Pixel::YUV420P12LE => (x / 2, y / 2),
                    Pixel::YUV422P12LE => (x / 2, y),
                    _ => (x, y),
                };
                (read12(&self.planes[0], x, y), read12(&self.planes[1], cx, cy), read12(&self.planes[2], cx, cy))
            }
            Pixel::P010LE | Pixel::P016LE => {
                let y_idx = y * self.planes[0].stride + x * 2;
                let yv = u16::from_le_bytes([self.planes[0].data[y_idx], self.planes[0].data[y_idx + 1]]) >> 8;
                let uv_idx = (y / 2) * self.planes[1].stride + (x / 2) * 4;
                let u = u16::from_le_bytes([self.planes[1].data[uv_idx], self.planes[1].data[uv_idx + 1]]) >> 8;
                let v = u16::from_le_bytes([self.planes[1].data[uv_idx + 2], self.planes[1].data[uv_idx + 3]]) >> 8;
                (yv as u8, u as u8, v as u8)
            }
            Pixel::RGBA | Pixel::RGB24 | Pixel::PAL8 => {
                let [r, g, b, _] = self.sample_rgba(x as u32, y as u32, ColorInfo { matrix: Matrix::Bt709, full_range: true, transfer: Transfer::Sdr, gamut: GamutKind::Bt709 });
                let rf = f32::from(r);
                let gf = f32::from(g);
                let bf = f32::from(b);
                let yv = (16.0 + (65.481 * rf + 128.553 * gf + 24.966 * bf) / 255.0).round().clamp(16.0, 235.0) as u8;
                let u = (128.0 + (-37.797 * rf - 74.203 * gf + 112.0 * bf) / 255.0).round().clamp(16.0, 240.0) as u8;
                let v = (128.0 + (112.0 * rf - 93.786 * gf - 18.214 * bf) / 255.0).round().clamp(16.0, 240.0) as u8;
                (yv, u, v)
            }
        }
    }

    /// Reads one pixel at `(x, y)` and converts it to RGBA8 using `color`.
    pub fn sample_rgba(&self, x: u32, y: u32, color: ColorInfo) -> [u8; 4] {
        let x = x.min(self.width.saturating_sub(1)) as usize;
        let y = y.min(self.height.saturating_sub(1)) as usize;
        match self.format {
            Pixel::RGBA => {
                let idx = y * self.planes[0].stride + x * 4;
                let d = &self.planes[0].data[idx..idx + 4];
                [d[0], d[1], d[2], d[3]]
            }
            Pixel::RGB24 => {
                let idx = y * self.planes[0].stride + x * 3;
                let d = &self.planes[0].data[idx..idx + 3];
                [d[0], d[1], d[2], 255]
            }
            Pixel::PAL8 => {
                let idx = self.planes[0].data[y * self.planes[0].stride + x] as usize * 4;
                let pal = &self.planes[1].data;
                if idx + 4 <= pal.len() {
                    [pal[idx], pal[idx + 1], pal[idx + 2], 255]
                } else {
                    [0, 0, 0, 255]
                }
            }
            _ => {
                let (yv, u, v) = self.sample_yuv8(x as u32, y as u32);
                yuv_to_rgba(yv, u, v, color)
            }
        }
    }

    /// Converts this frame into `dst` pixel format at the same dimensions.
    pub fn convert_to(&self, dst: Pixel) -> Option<RawVideoFrame> {
        let (w, h) = (self.width, self.height);
        if w == 0 || h == 0 {
            return None;
        }
        let mut out = RawVideoFrame::new(dst, w, h);
        out.pts = self.pts;
        out.best_effort_timestamp = self.best_effort_timestamp;
        out.sar = self.sar;
        out.color_space = self.color_space;
        out.color_range = self.color_range;
        out.color_trc = self.color_trc;
        out.color_primaries = self.color_primaries;
        out.interlaced = self.interlaced;
        out.top_field_first = self.top_field_first;

        let color = color_info(self.color_space, self.color_range, self.color_trc, self.color_primaries, w, h, false);
        match dst {
            Pixel::RGBA => {
                let stride = out.stride(0);
                for y in 0..h {
                    for x in 0..w {
                        let px = self.sample_rgba(x, y, color);
                        let off = y as usize * stride + x as usize * 4;
                        out.planes[0].data[off..off + 4].copy_from_slice(&px);
                    }
                }
                Some(out)
            }
            Pixel::YUV420P => {
                let (s0, s1, s2) = (out.stride(0), out.stride(1), out.stride(2));
                for y in 0..h {
                    for x in 0..w {
                        let (yv, u, v) = self.sample_yuv8(x, y);
                        out.planes[0].data[y as usize * s0 + x as usize] = yv;
                        if y.is_multiple_of(2) && x.is_multiple_of(2) {
                            let cx = (x / 2) as usize;
                            let cy = (y / 2) as usize;
                            out.planes[1].data[cy * s1 + cx] = u;
                            out.planes[2].data[cy * s2 + cx] = v;
                        }
                    }
                }
                Some(out)
            }
            Pixel::YUV420P10LE => {
                let (s0, s1, s2) = (out.stride(0), out.stride(1), out.stride(2));
                for y in 0..h {
                    for x in 0..w {
                        let (yv, u, v) = self.sample_yuv8(x, y);
                        let y10 = (u16::from(yv) << 2).to_le_bytes();
                        let off0 = y as usize * s0 + x as usize * 2;
                        out.planes[0].data[off0..off0 + 2].copy_from_slice(&y10);
                        if y.is_multiple_of(2) && x.is_multiple_of(2) {
                            let cx = (x / 2) as usize;
                            let cy = (y / 2) as usize;
                            let u10 = (u16::from(u) << 2).to_le_bytes();
                            let v10 = (u16::from(v) << 2).to_le_bytes();
                            let off1 = cy * s1 + cx * 2;
                            let off2 = cy * s2 + cx * 2;
                            out.planes[1].data[off1..off1 + 2].copy_from_slice(&u10);
                            out.planes[2].data[off2..off2 + 2].copy_from_slice(&v10);
                        }
                    }
                }
                Some(out)
            }
            _ => None,
        }
    }

    /// Scales the frame to `tw` × `th` RGBA8 using area/box averaging.
    pub fn scale_to_rgba(&self, tw: u32, th: u32, color: ColorInfo) -> Option<RgbaImage> {
        if self.width == 0 || self.height == 0 || tw == 0 || th == 0 {
            return None;
        }
        let mut data = vec![0u8; (tw * th * 4) as usize];
        for ty in 0..th {
            let y0 = (u64::from(ty) * u64::from(self.height) / u64::from(th)) as u32;
            let y1 = ((u64::from(ty + 1) * u64::from(self.height)).div_ceil(u64::from(th)) as u32).max(y0 + 1).min(self.height);
            let sy = y0 + (y1 - y0) / 2;
            for tx in 0..tw {
                let x0 = (u64::from(tx) * u64::from(self.width) / u64::from(tw)) as u32;
                let x1 = ((u64::from(tx + 1) * u64::from(self.width)).div_ceil(u64::from(tw)) as u32).max(x0 + 1).min(self.width);
                let sx = x0 + (x1 - x0) / 2;
                let px = self.sample_rgba(sx, sy, color);
                let off = ((ty * tw + tx) * 4) as usize;
                data[off..off + 4].copy_from_slice(&[px[0], px[1], px[2], 255]);
            }
        }
        Some(RgbaImage { width: tw, height: th, data })
    }
}

fn yuv_to_rgba(y: u8, u: u8, v: u8, color: ColorInfo) -> [u8; 4] {
    let (yf, uf, vf) = if color.full_range {
        (f32::from(y) / 255.0, (f32::from(u) - 128.0) / 255.0, (f32::from(v) - 128.0) / 255.0)
    } else {
        (((f32::from(y) - 16.0) / 219.0).clamp(0.0, 1.0), (f32::from(u) - 128.0) / 224.0, (f32::from(v) - 128.0) / 224.0)
    };
    let (kr, kb) = match color.matrix {
        Matrix::Bt601 => (0.299f32, 0.114f32),
        Matrix::Bt709 => (0.2126f32, 0.0722f32),
        Matrix::Bt2020 => (0.2627f32, 0.0593f32),
        Matrix::Smpte240 => (0.212f32, 0.087f32),
    };
    let kg = 1.0 - kr - kb;
    let r = yf + 2.0 * (1.0 - kr) * vf;
    let b = yf + 2.0 * (1.0 - kb) * uf;
    let g = yf - (2.0 * kr * (1.0 - kr) / kg) * vf - (2.0 * kb * (1.0 - kb) / kg) * uf;
    let to_u8 = |c: f32| (c.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    [to_u8(r), to_u8(g), to_u8(b), 255]
}

/// A decoded picture ready for presentation. Cheap to share (`Arc`) between threads.
pub struct VideoFrame {
    /// Presentation time in seconds from the start of the media.
    pub pts: f64,
    /// Nominal display duration in seconds.
    pub duration: f64,
    pub width: u32,
    pub height: u32,
    /// Sample (pixel) aspect ratio, `1.0` for square pixels.
    pub sar: f32,
    pub layout: PixelLayout,
    /// Significant bits per sample (8, 10, 12 or 16).
    pub bit_depth: u8,
    pub color: ColorInfo,
    pub interlaced: bool,
    /// Unique, increasing identifier of the decoded frame.
    pub serial: u64,
    frame: RawVideoFrame,
}

impl VideoFrame {
    pub(crate) fn new(frame: RawVideoFrame, pts: f64, duration: f64, sar: f32, native: Native, color: ColorInfo, serial: u64) -> Self {
        let interlaced = frame.is_interlaced();
        Self { pts, duration, width: frame.width(), height: frame.height(), sar, layout: native.layout, bit_depth: native.bit_depth, color, interlaced, serial, frame }
    }

    /// Plane `i` (`0 .. layout.planes()`).
    pub fn plane(&self, i: usize) -> Plane<'_> {
        let f = &self.frame;
        // For NV12 / P010 the UV plane reports `ceil(w / 2)` — exactly the number of UV pairs.
        Plane { data: f.data(i), stride: f.stride(i), width: f.plane_width(i), height: f.plane_height(i) }
    }

    /// Display aspect ratio (width / height after applying the pixel aspect ratio).
    pub fn display_aspect(&self) -> f32 {
        (self.width as f32 * self.sar) / self.height.max(1) as f32
    }

    /// Approximate memory held by the frame.
    pub fn byte_size(&self) -> usize {
        (0..self.layout.planes()).map(|i| self.frame.stride(i) * self.frame.plane_height(i) as usize).sum()
    }
}

impl std::fmt::Debug for VideoFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VideoFrame")
            .field("pts", &self.pts)
            .field("size", &(self.width, self.height))
            .field("layout", &self.layout)
            .field("bit_depth", &self.bit_depth)
            .field("serial", &self.serial)
            .finish()
    }
}

/// An 8-bit RGBA picture (thumbnails, cover art, screenshots).
#[derive(Clone, Debug, Default)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    /// `width * height * 4` bytes, tightly packed, straight alpha.
    pub data: Vec<u8>,
}

impl RgbaImage {
    /// Average colour (cheap, for ambient tinting).
    pub fn average_color(&self) -> [u8; 3] {
        if self.data.is_empty() {
            return [128, 128, 128];
        }
        let mut acc = [0u64; 3];
        let mut n = 0u64;
        for px in self.data.chunks_exact(4).step_by(7) {
            acc[0] += u64::from(px[0]);
            acc[1] += u64::from(px[1]);
            acc[2] += u64::from(px[2]);
            n += 1;
        }
        let n = n.max(1);
        [(acc[0] / n) as u8, (acc[1] / n) as u8, (acc[2] / n) as u8]
    }
}

// ---------------------------------------------------------------------------------------------
// Pixel-format policy
// ---------------------------------------------------------------------------------------------

/// A pixel format the renderer can consume directly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Native {
    pub layout: PixelLayout,
    pub bit_depth: u8,
    /// `true` for the deprecated full-range "YUVJ" formats.
    pub full_range: bool,
}

/// What to do with a decoded frame before it reaches the renderer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plan {
    /// Use the frame as is (zero copy).
    Direct(Native),
    /// Convert in pure Rust to this pixel format first.
    Convert { target: Pixel, native: Native },
}

fn n(layout: PixelLayout, bit_depth: u8, full_range: bool) -> Option<Native> {
    Some(Native { layout, bit_depth, full_range })
}

/// Native layout of `fmt`, if the renderer supports it. High bit depth formats require
/// `allow_16bit` (GPU support for 16-bit normalised textures).
pub fn native_layout(fmt: Pixel, allow_16bit: bool) -> Option<Native> {
    use PixelLayout::*;
    match fmt {
        Pixel::YUV420P | Pixel::YUV422P | Pixel::YUV444P => n(Planar8, 8, false),
        Pixel::YUVJ420P | Pixel::YUVJ422P | Pixel::YUVJ444P => n(Planar8, 8, true),
        Pixel::NV12 => n(SemiPlanar8, 8, false),
        Pixel::YUV420P10LE | Pixel::YUV422P10LE | Pixel::YUV444P10LE if allow_16bit => n(Planar16, 10, false),
        Pixel::YUV420P12LE | Pixel::YUV422P12LE | Pixel::YUV444P12LE if allow_16bit => n(Planar16, 12, false),
        Pixel::P010LE if allow_16bit => n(SemiPlanar16, 10, false),
        Pixel::P016LE if allow_16bit => n(SemiPlanar16, 16, false),
        Pixel::RGBA => n(Rgba8, 8, true),
        _ => None,
    }
}

pub(crate) fn descriptor_info(fmt: Pixel) -> (bool, bool, u8) {
    // (is_rgb_or_palette, has_alpha, component depth)
    match fmt {
        Pixel::RGBA => (true, true, 8),
        Pixel::RGB24 | Pixel::PAL8 => (true, false, 8),
        Pixel::YUVA420P => (false, true, 8),
        Pixel::YUV420P10LE | Pixel::YUV422P10LE | Pixel::YUV444P10LE | Pixel::YUV420P10BE | Pixel::P010LE => (false, false, 10),
        Pixel::YUV420P12LE | Pixel::YUV422P12LE | Pixel::YUV444P12LE => (false, false, 12),
        Pixel::P016LE => (false, false, 16),
        _ => (false, false, 8),
    }
}

/// Decides how a decoded frame of format `fmt` reaches the GPU.
pub fn plan(fmt: Pixel, allow_16bit: bool) -> Plan {
    if let Some(native) = native_layout(fmt, allow_16bit) {
        return Plan::Direct(native);
    }
    let (is_rgb, _alpha, depth) = descriptor_info(fmt);
    if is_rgb {
        return Plan::Convert { target: Pixel::RGBA, native: Native { layout: PixelLayout::Rgba8, bit_depth: 8, full_range: true } };
    }
    if allow_16bit && depth > 8 {
        return Plan::Convert { target: Pixel::YUV420P10LE, native: Native { layout: PixelLayout::Planar16, bit_depth: 10, full_range: false } };
    }
    Plan::Convert { target: Pixel::YUV420P, native: Native { layout: PixelLayout::Planar8, bit_depth: 8, full_range: false } }
}

/// Derives the colour description of a frame, filling in sensible defaults for untagged video
/// (BT.601 for SD, BT.709 for HD — the same heuristic every serious player uses).
pub fn color_info(space: Space, range: Range, trc: TransferCharacteristic, primaries: Primaries, width: u32, height: u32, fmt_full_range: bool) -> ColorInfo {
    let matrix = match space {
        Space::BT709 => Matrix::Bt709,
        Space::BT470BG | Space::SMPTE170M | Space::FCC => Matrix::Bt601,
        Space::BT2020NCL | Space::BT2020CL => Matrix::Bt2020,
        Space::SMPTE240M => Matrix::Smpte240,
        _ => {
            if width >= 1280 || height > 576 {
                Matrix::Bt709
            } else {
                Matrix::Bt601
            }
        }
    };
    let full_range = match range {
        Range::JPEG => true,
        Range::MPEG => false,
        _ => fmt_full_range,
    };
    let transfer = match trc {
        TransferCharacteristic::SMPTE2084 => Transfer::Pq,
        TransferCharacteristic::ARIB_STD_B67 => Transfer::Hlg,
        _ => Transfer::Sdr,
    };
    let gamut = if primaries == Primaries::BT2020 || matches!(space, Space::BT2020NCL | Space::BT2020CL) { GamutKind::Bt2020 } else { GamutKind::Bt709 };
    ColorInfo { matrix, full_range, transfer, gamut }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_formats() {
        assert_eq!(native_layout(Pixel::YUV420P, false).unwrap().layout, PixelLayout::Planar8);
        assert!(native_layout(Pixel::YUVJ420P, false).unwrap().full_range);
        assert_eq!(native_layout(Pixel::NV12, false).unwrap().layout, PixelLayout::SemiPlanar8);
        assert!(native_layout(Pixel::YUV420P10LE, false).is_none(), "needs 16-bit texture support");
        assert_eq!(native_layout(Pixel::YUV420P10LE, true).unwrap().bit_depth, 10);
        assert_eq!(native_layout(Pixel::P010LE, true).unwrap().layout, PixelLayout::SemiPlanar16);
    }

    #[test]
    fn conversion_plans() {
        assert!(matches!(plan(Pixel::YUV420P, false), Plan::Direct(_)));
        assert!(matches!(plan(Pixel::YUV420P10LE, false), Plan::Convert { target: Pixel::YUV420P, .. }));
        assert!(matches!(plan(Pixel::YUV420P10BE, true), Plan::Convert { target: Pixel::YUV420P10LE, .. }));
        assert!(matches!(plan(Pixel::RGB24, true), Plan::Convert { target: Pixel::RGBA, .. }));
        assert!(matches!(plan(Pixel::PAL8, false), Plan::Convert { target: Pixel::RGBA, .. }));
        assert!(matches!(plan(Pixel::GRAY8, false), Plan::Convert { target: Pixel::YUV420P, .. }));
        assert!(matches!(plan(Pixel::YUVA420P, false), Plan::Convert { target: Pixel::YUV420P, .. }));
    }

    #[test]
    fn colour_defaults() {
        let sd = color_info(Space::Unspecified, Range::Unspecified, TransferCharacteristic::Unspecified, Primaries::Unspecified, 720, 576, false);
        assert_eq!(sd.matrix, Matrix::Bt601);
        assert!(!sd.full_range);
        let hd = color_info(Space::Unspecified, Range::Unspecified, TransferCharacteristic::Unspecified, Primaries::Unspecified, 1920, 1080, false);
        assert_eq!(hd.matrix, Matrix::Bt709);
        let hdr = color_info(Space::BT2020NCL, Range::MPEG, TransferCharacteristic::SMPTE2084, Primaries::BT2020, 3840, 2160, false);
        assert_eq!(hdr.transfer, Transfer::Pq);
        assert_eq!(hdr.gamut, GamutKind::Bt2020);
        let jpeg = color_info(Space::Unspecified, Range::Unspecified, TransferCharacteristic::Unspecified, Primaries::Unspecified, 100, 100, true);
        assert!(jpeg.full_range);
    }

    #[test]
    fn average_colour() {
        let img = RgbaImage { width: 2, height: 1, data: vec![255, 0, 0, 255, 255, 0, 0, 255] };
        assert_eq!(img.average_color(), [255, 0, 0]);
        assert_eq!(RgbaImage::default().average_color(), [128, 128, 128]);
    }
}

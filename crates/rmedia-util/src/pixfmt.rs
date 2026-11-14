//! Pixel format definitions and descriptors (AVPixelFormat reproduction).

use std::fmt;
use std::str::FromStr;
use crate::error::{Result, RmediaError};

/// Pixel format enumeration matching standard FFmpeg formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PixelFormat {
    #[default]
    None = -1,
    // Planar YUV
    Yuv420p = 0,
    Yuv422p,
    Yuv444p,
    Yuv410p,
    Yuv411p,
    // Planar YUV with Alpha
    Yuva420p,
    Yuva422p,
    Yuva444p,
    // Semi-planar YUV
    Nv12,
    Nv21,
    P010le,
    P010be,
    P012le,
    P012be,
    P016le,
    P016be,
    // Packed YUV
    Yuyv422,
    Uyvy422,
    // Packed RGB / BGR (8-bit)
    Rgb24,
    Bgr24,
    Rgba,
    Bgra,
    Argb,
    Abgr,
    // Packed RGB (16-bit)
    Rgb48le,
    Rgb48be,
    Rgba64le,
    Rgba64be,
    // Planar GBR
    Gbrp,
    Gbrap,
    // Grayscale
    Gray8,
    Gray9le,
    Gray10le,
    Gray12le,
    Gray14le,
    Gray16le,
    Gray16be,
    // Paletted
    Pal8,
}

/// Description of the layout and properties of a pixel format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelFormatDescriptor {
    pub name: &'static str,
    pub num_components: u8,
    pub log2_chroma_w: u8,
    pub log2_chroma_h: u8,
    pub bit_depth: u8,
    pub is_planar: bool,
    pub is_rgb: bool,
    pub has_alpha: bool,
}

impl PixelFormat {
    /// Return the metadata descriptor for this pixel format.
    pub fn descriptor(self) -> Option<PixelFormatDescriptor> {
        match self {
            Self::None => None,
            Self::Yuv420p => Some(PixelFormatDescriptor {
                name: "yuv420p",
                num_components: 3,
                log2_chroma_w: 1,
                log2_chroma_h: 1,
                bit_depth: 8,
                is_planar: true,
                is_rgb: false,
                has_alpha: false,
            }),
            Self::Yuv422p => Some(PixelFormatDescriptor {
                name: "yuv422p",
                num_components: 3,
                log2_chroma_w: 1,
                log2_chroma_h: 0,
                bit_depth: 8,
                is_planar: true,
                is_rgb: false,
                has_alpha: false,
            }),
            Self::Yuv444p => Some(PixelFormatDescriptor {
                name: "yuv444p",
                num_components: 3,
                log2_chroma_w: 0,
                log2_chroma_h: 0,
                bit_depth: 8,
                is_planar: true,
                is_rgb: false,
                has_alpha: false,
            }),
            Self::Yuv410p => Some(PixelFormatDescriptor {
                name: "yuv410p",
                num_components: 3,
                log2_chroma_w: 2,
                log2_chroma_h: 2,
                bit_depth: 8,
                is_planar: true,
                is_rgb: false,
                has_alpha: false,
            }),
            Self::Yuv411p => Some(PixelFormatDescriptor {
                name: "yuv411p",
                num_components: 3,
                log2_chroma_w: 2,
                log2_chroma_h: 0,
                bit_depth: 8,
                is_planar: true,
                is_rgb: false,
                has_alpha: false,
            }),
            Self::Yuva420p => Some(PixelFormatDescriptor {
                name: "yuva420p",
                num_components: 4,
                log2_chroma_w: 1,
                log2_chroma_h: 1,
                bit_depth: 8,
                is_planar: true,
                is_rgb: false,
                has_alpha: true,
            }),
            Self::Yuva422p => Some(PixelFormatDescriptor {
                name: "yuva422p",
                num_components: 4,
                log2_chroma_w: 1,
                log2_chroma_h: 0,
                bit_depth: 8,
                is_planar: true,
                is_rgb: false,
                has_alpha: true,
            }),
            Self::Yuva444p => Some(PixelFormatDescriptor {
                name: "yuva444p",
                num_components: 4,
                log2_chroma_w: 0,
                log2_chroma_h: 0,
                bit_depth: 8,
                is_planar: true,
                is_rgb: false,
                has_alpha: true,
            }),
            Self::Nv12 => Some(PixelFormatDescriptor {
                name: "nv12",
                num_components: 3,
                log2_chroma_w: 1,
                log2_chroma_h: 1,
                bit_depth: 8,
                is_planar: false,
                is_rgb: false,
                has_alpha: false,
            }),
            Self::Nv21 => Some(PixelFormatDescriptor {
                name: "nv21",
                num_components: 3,
                log2_chroma_w: 1,
                log2_chroma_h: 1,
                bit_depth: 8,
                is_planar: false,
                is_rgb: false,
                has_alpha: false,
            }),
            Self::P010le => Some(PixelFormatDescriptor {
                name: "p010le",
                num_components: 3,
                log2_chroma_w: 1,
                log2_chroma_h: 1,
                bit_depth: 10,
                is_planar: false,
                is_rgb: false,
                has_alpha: false,
            }),
            Self::P010be => Some(PixelFormatDescriptor {
                name: "p010be",
                num_components: 3,
                log2_chroma_w: 1,
                log2_chroma_h: 1,
                bit_depth: 10,
                is_planar: false,
                is_rgb: false,
                has_alpha: false,
            }),
            Self::P012le => Some(PixelFormatDescriptor {
                name: "p012le",
                num_components: 3,
                log2_chroma_w: 1,
                log2_chroma_h: 1,
                bit_depth: 12,
                is_planar: false,
                is_rgb: false,
                has_alpha: false,
            }),
            Self::P012be => Some(PixelFormatDescriptor {
                name: "p012be",
                num_components: 3,
                log2_chroma_w: 1,
                log2_chroma_h: 1,
                bit_depth: 12,
                is_planar: false,
                is_rgb: false,
                has_alpha: false,
            }),
            Self::P016le => Some(PixelFormatDescriptor {
                name: "p016le",
                num_components: 3,
                log2_chroma_w: 1,
                log2_chroma_h: 1,
                bit_depth: 16,
                is_planar: false,
                is_rgb: false,
                has_alpha: false,
            }),
            Self::P016be => Some(PixelFormatDescriptor {
                name: "p016be",
                num_components: 3,
                log2_chroma_w: 1,
                log2_chroma_h: 1,
                bit_depth: 16,
                is_planar: false,
                is_rgb: false,
                has_alpha: false,
            }),
            Self::Yuyv422 => Some(PixelFormatDescriptor {
                name: "yuyv422",
                num_components: 3,
                log2_chroma_w: 1,
                log2_chroma_h: 0,
                bit_depth: 8,
                is_planar: false,
                is_rgb: false,
                has_alpha: false,
            }),
            Self::Uyvy422 => Some(PixelFormatDescriptor {
                name: "uyvy422",
                num_components: 3,
                log2_chroma_w: 1,
                log2_chroma_h: 0,
                bit_depth: 8,
                is_planar: false,
                is_rgb: false,
                has_alpha: false,
            }),
            Self::Rgb24 => Some(PixelFormatDescriptor {
                name: "rgb24",
                num_components: 3,
                log2_chroma_w: 0,
                log2_chroma_h: 0,
                bit_depth: 8,
                is_planar: false,
                is_rgb: true,
                has_alpha: false,
            }),
            Self::Bgr24 => Some(PixelFormatDescriptor {
                name: "bgr24",
                num_components: 3,
                log2_chroma_w: 0,
                log2_chroma_h: 0,
                bit_depth: 8,
                is_planar: false,
                is_rgb: true,
                has_alpha: false,
            }),
            Self::Rgba => Some(PixelFormatDescriptor {
                name: "rgba",
                num_components: 4,
                log2_chroma_w: 0,
                log2_chroma_h: 0,
                bit_depth: 8,
                is_planar: false,
                is_rgb: true,
                has_alpha: true,
            }),
            Self::Bgra => Some(PixelFormatDescriptor {
                name: "bgra",
                num_components: 4,
                log2_chroma_w: 0,
                log2_chroma_h: 0,
                bit_depth: 8,
                is_planar: false,
                is_rgb: true,
                has_alpha: true,
            }),
            Self::Argb => Some(PixelFormatDescriptor {
                name: "argb",
                num_components: 4,
                log2_chroma_w: 0,
                log2_chroma_h: 0,
                bit_depth: 8,
                is_planar: false,
                is_rgb: true,
                has_alpha: true,
            }),
            Self::Abgr => Some(PixelFormatDescriptor {
                name: "abgr",
                num_components: 4,
                log2_chroma_w: 0,
                log2_chroma_h: 0,
                bit_depth: 8,
                is_planar: false,
                is_rgb: true,
                has_alpha: true,
            }),
            Self::Rgb48le => Some(PixelFormatDescriptor {
                name: "rgb48le",
                num_components: 3,
                log2_chroma_w: 0,
                log2_chroma_h: 0,
                bit_depth: 16,
                is_planar: false,
                is_rgb: true,
                has_alpha: false,
            }),
            Self::Rgb48be => Some(PixelFormatDescriptor {
                name: "rgb48be",
                num_components: 3,
                log2_chroma_w: 0,
                log2_chroma_h: 0,
                bit_depth: 16,
                is_planar: false,
                is_rgb: true,
                has_alpha: false,
            }),
            Self::Rgba64le => Some(PixelFormatDescriptor {
                name: "rgba64le",
                num_components: 4,
                log2_chroma_w: 0,
                log2_chroma_h: 0,
                bit_depth: 16,
                is_planar: false,
                is_rgb: true,
                has_alpha: true,
            }),
            Self::Rgba64be => Some(PixelFormatDescriptor {
                name: "rgba64be",
                num_components: 4,
                log2_chroma_w: 0,
                log2_chroma_h: 0,
                bit_depth: 16,
                is_planar: false,
                is_rgb: true,
                has_alpha: true,
            }),
            Self::Gbrp => Some(PixelFormatDescriptor {
                name: "gbrp",
                num_components: 3,
                log2_chroma_w: 0,
                log2_chroma_h: 0,
                bit_depth: 8,
                is_planar: true,
                is_rgb: true,
                has_alpha: false,
            }),
            Self::Gbrap => Some(PixelFormatDescriptor {
                name: "gbrap",
                num_components: 4,
                log2_chroma_w: 0,
                log2_chroma_h: 0,
                bit_depth: 8,
                is_planar: true,
                is_rgb: true,
                has_alpha: true,
            }),
            Self::Gray8 => Some(PixelFormatDescriptor {
                name: "gray8",
                num_components: 1,
                log2_chroma_w: 0,
                log2_chroma_h: 0,
                bit_depth: 8,
                is_planar: true,
                is_rgb: false,
                has_alpha: false,
            }),
            Self::Gray9le => Some(PixelFormatDescriptor {
                name: "gray9le",
                num_components: 1,
                log2_chroma_w: 0,
                log2_chroma_h: 0,
                bit_depth: 9,
                is_planar: true,
                is_rgb: false,
                has_alpha: false,
            }),
            Self::Gray10le => Some(PixelFormatDescriptor {
                name: "gray10le",
                num_components: 1,
                log2_chroma_w: 0,
                log2_chroma_h: 0,
                bit_depth: 10,
                is_planar: true,
                is_rgb: false,
                has_alpha: false,
            }),
            Self::Gray12le => Some(PixelFormatDescriptor {
                name: "gray12le",
                num_components: 1,
                log2_chroma_w: 0,
                log2_chroma_h: 0,
                bit_depth: 12,
                is_planar: true,
                is_rgb: false,
                has_alpha: false,
            }),
            Self::Gray14le => Some(PixelFormatDescriptor {
                name: "gray14le",
                num_components: 1,
                log2_chroma_w: 0,
                log2_chroma_h: 0,
                bit_depth: 14,
                is_planar: true,
                is_rgb: false,
                has_alpha: false,
            }),
            Self::Gray16le => Some(PixelFormatDescriptor {
                name: "gray16le",
                num_components: 1,
                log2_chroma_w: 0,
                log2_chroma_h: 0,
                bit_depth: 16,
                is_planar: true,
                is_rgb: false,
                has_alpha: false,
            }),
            Self::Gray16be => Some(PixelFormatDescriptor {
                name: "gray16be",
                num_components: 1,
                log2_chroma_w: 0,
                log2_chroma_h: 0,
                bit_depth: 16,
                is_planar: true,
                is_rgb: false,
                has_alpha: false,
            }),
            Self::Pal8 => Some(PixelFormatDescriptor {
                name: "pal8",
                num_components: 1,
                log2_chroma_w: 0,
                log2_chroma_h: 0,
                bit_depth: 8,
                is_planar: false,
                is_rgb: false,
                has_alpha: false,
            }),
        }
    }

    /// Calculate buffer size needed for an image of given dimensions and format.
    pub fn image_buffer_size(self, width: usize, height: usize, align: usize) -> Result<usize> {
        let desc = self.descriptor().ok_or_else(|| {
            RmediaError::InvalidArgument(format!("Unknown pixel format {:?}", self))
        })?;

        let align_val = if align == 0 { 1 } else { align };

        let total_size = match self {
            Self::Yuv420p | Self::Yuv422p | Self::Yuv444p | Self::Yuv410p | Self::Yuv411p => {
                let y_stride = (width + align_val - 1) & !(align_val - 1);
                let uv_width = width >> desc.log2_chroma_w;
                let uv_height = height >> desc.log2_chroma_h;
                let uv_stride = (uv_width + align_val - 1) & !(align_val - 1);
                y_stride * height + uv_stride * uv_height * 2
            }
            Self::Yuva420p | Self::Yuva422p | Self::Yuva444p => {
                let y_stride = (width + align_val - 1) & !(align_val - 1);
                let uv_width = width >> desc.log2_chroma_w;
                let uv_height = height >> desc.log2_chroma_h;
                let uv_stride = (uv_width + align_val - 1) & !(align_val - 1);
                y_stride * height * 2 + uv_stride * uv_height * 2
            }
            Self::Nv12 | Self::Nv21 => {
                let y_stride = (width + align_val - 1) & !(align_val - 1);
                let uv_stride = y_stride;
                let uv_height = height >> desc.log2_chroma_h;
                y_stride * height + uv_stride * uv_height
            }
            Self::P010le | Self::P010be | Self::P012le | Self::P012be | Self::P016le | Self::P016be => {
                let y_stride = ((width * 2) + align_val - 1) & !(align_val - 1);
                let uv_stride = y_stride;
                let uv_height = height >> desc.log2_chroma_h;
                y_stride * height + uv_stride * uv_height
            }
            Self::Yuyv422 | Self::Uyvy422 => {
                let stride = ((width * 2) + align_val - 1) & !(align_val - 1);
                stride * height
            }
            Self::Rgb24 | Self::Bgr24 => {
                let stride = ((width * 3) + align_val - 1) & !(align_val - 1);
                stride * height
            }
            Self::Rgba | Self::Bgra | Self::Argb | Self::Abgr => {
                let stride = ((width * 4) + align_val - 1) & !(align_val - 1);
                stride * height
            }
            Self::Rgb48le | Self::Rgb48be => {
                let stride = ((width * 6) + align_val - 1) & !(align_val - 1);
                stride * height
            }
            Self::Rgba64le | Self::Rgba64be => {
                let stride = ((width * 8) + align_val - 1) & !(align_val - 1);
                stride * height
            }
            Self::Gbrp => {
                let stride = (width + align_val - 1) & !(align_val - 1);
                stride * height * 3
            }
            Self::Gbrap => {
                let stride = (width + align_val - 1) & !(align_val - 1);
                stride * height * 4
            }
            Self::Gray8 | Self::Pal8 => {
                let stride = (width + align_val - 1) & !(align_val - 1);
                stride * height
            }
            Self::Gray9le | Self::Gray10le | Self::Gray12le | Self::Gray14le | Self::Gray16le | Self::Gray16be => {
                let stride = ((width * 2) + align_val - 1) & !(align_val - 1);
                stride * height
            }
            Self::None => return Err(RmediaError::InvalidArgument("Invalid pixel format None".into())),
        };

        Ok(total_size)
    }
}

impl fmt::Display for PixelFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(desc) = self.descriptor() {
            write!(f, "{}", desc.name)
        } else {
            write!(f, "none")
        }
    }
}

impl FromStr for PixelFormat {
    type Err = RmediaError;
    fn from_str(s: &str) -> Result<Self> {
        let name = s.to_ascii_lowercase();
        match name.as_str() {
            "yuv420p" => Ok(Self::Yuv420p),
            "yuv422p" => Ok(Self::Yuv422p),
            "yuv444p" => Ok(Self::Yuv444p),
            "yuv410p" => Ok(Self::Yuv410p),
            "yuv411p" => Ok(Self::Yuv411p),
            "yuva420p" => Ok(Self::Yuva420p),
            "yuva422p" => Ok(Self::Yuva422p),
            "yuva444p" => Ok(Self::Yuva444p),
            "nv12" => Ok(Self::Nv12),
            "nv21" => Ok(Self::Nv21),
            "p010le" | "p010" => Ok(Self::P010le),
            "p010be" => Ok(Self::P010be),
            "p012le" | "p012" => Ok(Self::P012le),
            "p012be" => Ok(Self::P012be),
            "p016le" | "p016" => Ok(Self::P016le),
            "p016be" => Ok(Self::P016be),
            "yuyv422" => Ok(Self::Yuyv422),
            "uyvy422" => Ok(Self::Uyvy422),
            "rgb24" => Ok(Self::Rgb24),
            "bgr24" => Ok(Self::Bgr24),
            "rgba" => Ok(Self::Rgba),
            "bgra" => Ok(Self::Bgra),
            "argb" => Ok(Self::Argb),
            "abgr" => Ok(Self::Abgr),
            "rgb48le" | "rgb48" => Ok(Self::Rgb48le),
            "rgb48be" => Ok(Self::Rgb48be),
            "rgba64le" | "rgba64" => Ok(Self::Rgba64le),
            "rgba64be" => Ok(Self::Rgba64be),
            "gbrp" => Ok(Self::Gbrp),
            "gbrap" => Ok(Self::Gbrap),
            "gray8" | "gray" => Ok(Self::Gray8),
            "gray9le" | "gray9" => Ok(Self::Gray9le),
            "gray10le" | "gray10" => Ok(Self::Gray10le),
            "gray12le" | "gray12" => Ok(Self::Gray12le),
            "gray14le" | "gray14" => Ok(Self::Gray14le),
            "gray16le" | "gray16" => Ok(Self::Gray16le),
            "gray16be" => Ok(Self::Gray16be),
            "pal8" => Ok(Self::Pal8),
            "none" => Ok(Self::None),
            _ => Err(RmediaError::InvalidArgument(format!("Unknown pixel format string: {}", s))),
        }
    }
}

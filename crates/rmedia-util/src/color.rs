//! Colorimetry, color spaces, and HDR metadata (AVColorSpace, AVColorRange, etc.).

use crate::rational::Rational;

/// Color primaries (AVColorPrimaries).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ColorPrimaries {
    Reserved0 = 0,
    Bt709 = 1,
    #[default]
    Unspecified = 2,
    Reserved = 3,
    Bt470M = 4,
    Bt470Bg = 5,
    Smpte170M = 6,
    Smpte240M = 7,
    Film = 8,
    Bt2020 = 9,
    Smpte428 = 10,
    Smpte431 = 11, // DCI-P3
    Smpte432 = 12, // Display-P3
    JedecP22 = 22,
}

/// Color transfer characteristics / gamma curves (AVColorTransferCharacteristic).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ColorTransferCharacteristic {
    Reserved0 = 0,
    Bt709 = 1,
    #[default]
    Unspecified = 2,
    Reserved = 3,
    Gamma22 = 4,
    Gamma28 = 5,
    Smpte170M = 6,
    Smpte240M = 7,
    Linear = 8,
    Log = 9,
    LogSqrt = 10,
    Iec61966_2_4 = 11,
    Bt1361 = 12,
    Iec61966_2_1 = 13, // sRGB
    Bt2020_10 = 14,
    Bt2020_12 = 15,
    Smpte2084 = 16, // PQ (HDR10)
    Smpte428 = 17,
    AribStdB67 = 18, // HLG
}

/// Matrix coefficients / YCbCr transformation matrix (AVColorSpace).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ColorSpace {
    Rgb = 0,
    Bt709 = 1,
    #[default]
    Unspecified = 2,
    Reserved = 3,
    Fcc = 4,
    Bt470Bg = 5,
    Smpte170M = 6,
    Smpte240M = 7,
    Ycgco = 8,
    Bt2020Ncl = 9,
    Bt2020Cl = 10,
    Smpte2085 = 11,
    ChromaDerivedNcl = 12,
    ChromaDerivedCl = 13,
    Ictcp = 14,
}

/// Color range (AVColorRange): Limited/TV (16..235) vs Full/PC (0..255).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ColorRange {
    #[default]
    Unspecified = 0,
    /// Limited range: Y: 16-235, UV: 16-240 (MPEG / Broadcast TV).
    Limited = 1,
    /// Full range: YUV: 0-255 (JPEG / PC).
    Full = 2,
}

/// Chroma sample location (AVChromaLocation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ChromaLocation {
    #[default]
    Unspecified = 0,
    /// Left chroma sample location (MPEG-2, H.264 default).
    Left = 1,
    /// Center chroma sample location (MPEG-1, JPEG).
    Center = 2,
    /// Top-left chroma sample location (HEVC, AV1 default).
    TopLeft = 3,
    Top = 4,
    BottomLeft = 5,
    Bottom = 6,
}

/// SMPTE ST 2086 Mastering Display Color Volume metadata (HDR10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MasteringDisplayMetadata {
    /// Display primaries chromaticity coordinates (x, y) * 50000.
    pub display_primaries: [[Rational; 2]; 3],
    /// White point chromaticity coordinates (x, y) * 50000.
    pub white_point: [Rational; 2],
    /// Maximum display luminance in cd/m^2 (nits) * 10000.
    pub min_luminance: Rational,
    /// Minimum display luminance in cd/m^2 (nits) * 10000.
    pub max_luminance: Rational,
    pub has_primaries: bool,
    pub has_luminance: bool,
}

/// Content Light Level Information metadata (CTA 861.3 / HDR10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ContentLightLevel {
    /// Maximum Content Light Level (MaxCLL) in cd/m^2 (nits).
    pub max_cll: u32,
    /// Maximum Frame-Average Light Level (MaxFALL) in cd/m^2 (nits).
    pub max_fall: u32,
}

/// 3x3 Color Matrix coefficients for YCbCr to RGB conversion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColorMatrixCoeffs {
    pub kr: f64,
    pub kg: f64,
    pub kb: f64,
}

impl ColorMatrixCoeffs {
    pub const BT601: Self = Self {
        kr: 0.299,
        kg: 0.587,
        kb: 0.114,
    };

    pub const BT709: Self = Self {
        kr: 0.2126,
        kg: 0.7152,
        kb: 0.0722,
    };

    pub const BT2020: Self = Self {
        kr: 0.2627,
        kg: 0.6780,
        kb: 0.0593,
    };

    pub const SMPTE240M: Self = Self {
        kr: 0.212,
        kg: 0.701,
        kb: 0.087,
    };

    /// Get standard color matrix coefficients from ColorSpace.
    pub fn for_space(space: ColorSpace) -> Self {
        match space {
            ColorSpace::Bt709 => Self::BT709,
            ColorSpace::Bt2020Ncl | ColorSpace::Bt2020Cl => Self::BT2020,
            ColorSpace::Smpte240M => Self::SMPTE240M,
            _ => Self::BT601,
        }
    }
}

//! Audio sample format definitions and properties (AVSampleFormat reproduction).

use std::fmt;
use std::str::FromStr;
use crate::error::{Result, RmediaError};

/// Audio sample formats matching standard FFmpeg conventions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SampleFormat {
    #[default]
    None = -1,
    /// Unsigned 8-bit, interleaved.
    U8 = 0,
    /// Signed 16-bit, interleaved.
    S16,
    /// Signed 32-bit, interleaved.
    S32,
    /// Float 32-bit, interleaved.
    Flt,
    /// Double 64-bit, interleaved.
    Dbl,
    /// Signed 64-bit, interleaved.
    S64,
    /// Unsigned 8-bit, planar.
    U8P,
    /// Signed 16-bit, planar.
    S16P,
    /// Signed 32-bit, planar.
    S32P,
    /// Float 32-bit, planar.
    FltP,
    /// Double 64-bit, planar.
    DblP,
    /// Signed 64-bit, planar.
    S64P,
}

impl SampleFormat {
    /// Return the number of bytes per sample for a single channel.
    pub const fn bytes_per_sample(self) -> usize {
        match self {
            Self::U8 | Self::U8P => 1,
            Self::S16 | Self::S16P => 2,
            Self::S32 | Self::S32P | Self::Flt | Self::FltP => 4,
            Self::Dbl | Self::DblP | Self::S64 | Self::S64P => 8,
            Self::None => 0,
        }
    }

    /// Check if the format is planar.
    pub const fn is_planar(self) -> bool {
        matches!(
            self,
            Self::U8P | Self::S16P | Self::S32P | Self::FltP | Self::DblP | Self::S64P
        )
    }

    /// Get corresponding packed/interleaved format.
    pub const fn get_packed(self) -> Self {
        match self {
            Self::U8 | Self::U8P => Self::U8,
            Self::S16 | Self::S16P => Self::S16,
            Self::S32 | Self::S32P => Self::S32,
            Self::Flt | Self::FltP => Self::Flt,
            Self::Dbl | Self::DblP => Self::Dbl,
            Self::S64 | Self::S64P => Self::S64,
            Self::None => Self::None,
        }
    }

    /// Get corresponding planar format.
    pub const fn get_planar(self) -> Self {
        match self {
            Self::U8 | Self::U8P => Self::U8P,
            Self::S16 | Self::S16P => Self::S16P,
            Self::S32 | Self::S32P => Self::S32P,
            Self::Flt | Self::FltP => Self::FltP,
            Self::Dbl | Self::DblP => Self::DblP,
            Self::S64 | Self::S64P => Self::S64P,
            Self::None => Self::None,
        }
    }

    /// Calculate buffer size in bytes for given channel count and sample count.
    pub fn buffer_size(self, nb_channels: usize, nb_samples: usize) -> usize {
        self.bytes_per_sample() * nb_channels * nb_samples
    }

    /// Return string name of the sample format.
    pub const fn name(self) -> &'static str {
        match self {
            Self::U8 => "u8",
            Self::S16 => "s16",
            Self::S32 => "s32",
            Self::Flt => "flt",
            Self::Dbl => "dbl",
            Self::S64 => "s64",
            Self::U8P => "u8p",
            Self::S16P => "s16p",
            Self::S32P => "s32p",
            Self::FltP => "fltp",
            Self::DblP => "dblp",
            Self::S64P => "s64p",
            Self::None => "none",
        }
    }
}

impl fmt::Display for SampleFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

impl FromStr for SampleFormat {
    type Err = RmediaError;
    fn from_str(s: &str) -> Result<Self> {
        let name = s.to_ascii_lowercase();
        match name.as_str() {
            "u8" => Ok(Self::U8),
            "s16" => Ok(Self::S16),
            "s32" => Ok(Self::S32),
            "flt" | "float" => Ok(Self::Flt),
            "dbl" | "double" => Ok(Self::Dbl),
            "s64" => Ok(Self::S64),
            "u8p" => Ok(Self::U8P),
            "s16p" => Ok(Self::S16P),
            "s32p" => Ok(Self::S32P),
            "fltp" | "floatp" => Ok(Self::FltP),
            "dblp" | "doublep" => Ok(Self::DblP),
            "s64p" => Ok(Self::S64P),
            "none" => Ok(Self::None),
            _ => Err(RmediaError::InvalidArgument(format!("Unknown sample format string: {}", s))),
        }
    }
}

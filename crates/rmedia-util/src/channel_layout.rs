//! Audio channel layout definitions and masks (AVChannelLayout reproduction).

use std::fmt;
use std::str::FromStr;
use crate::error::{Result, RmediaError};

/// Channel bitmask constants.
pub const AV_CH_FRONT_LEFT: u64 = 1 << 0;
pub const AV_CH_FRONT_RIGHT: u64 = 1 << 1;
pub const AV_CH_FRONT_CENTER: u64 = 1 << 2;
pub const AV_CH_LOW_FREQUENCY: u64 = 1 << 3;
pub const AV_CH_BACK_LEFT: u64 = 1 << 4;
pub const AV_CH_BACK_RIGHT: u64 = 1 << 5;
pub const AV_CH_FRONT_LEFT_OF_CENTER: u64 = 1 << 6;
pub const AV_CH_FRONT_RIGHT_OF_CENTER: u64 = 1 << 7;
pub const AV_CH_BACK_CENTER: u64 = 1 << 8;
pub const AV_CH_SIDE_LEFT: u64 = 1 << 9;
pub const AV_CH_SIDE_RIGHT: u64 = 1 << 10;
pub const AV_CH_TOP_CENTER: u64 = 1 << 11;
pub const AV_CH_TOP_FRONT_LEFT: u64 = 1 << 12;
pub const AV_CH_TOP_FRONT_CENTER: u64 = 1 << 13;
pub const AV_CH_TOP_FRONT_RIGHT: u64 = 1 << 14;
pub const AV_CH_TOP_BACK_LEFT: u64 = 1 << 15;
pub const AV_CH_TOP_BACK_CENTER: u64 = 1 << 16;
pub const AV_CH_TOP_BACK_RIGHT: u64 = 1 << 17;
pub const AV_CH_TOP_SIDE_LEFT: u64 = 1 << 18;
pub const AV_CH_TOP_SIDE_RIGHT: u64 = 1 << 19;
pub const AV_CH_BOTTOM_FRONT_CENTER: u64 = 1 << 20;
pub const AV_CH_BOTTOM_FRONT_LEFT: u64 = 1 << 21;
pub const AV_CH_BOTTOM_FRONT_RIGHT: u64 = 1 << 22;

/// Standard audio channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Channel {
    FrontLeft,
    FrontRight,
    FrontCenter,
    LowFrequency,
    BackLeft,
    BackRight,
    FrontLeftOfCenter,
    FrontRightOfCenter,
    BackCenter,
    SideLeft,
    SideRight,
    TopCenter,
    TopFrontLeft,
    TopFrontCenter,
    TopFrontRight,
    TopBackLeft,
    TopBackCenter,
    TopBackRight,
    TopSideLeft,
    TopSideRight,
    BottomFrontCenter,
    BottomFrontLeft,
    BottomFrontRight,
    Unknown,
}

impl Channel {
    /// Return the channel's single-bit mask.
    pub const fn mask(self) -> u64 {
        match self {
            Self::FrontLeft => AV_CH_FRONT_LEFT,
            Self::FrontRight => AV_CH_FRONT_RIGHT,
            Self::FrontCenter => AV_CH_FRONT_CENTER,
            Self::LowFrequency => AV_CH_LOW_FREQUENCY,
            Self::BackLeft => AV_CH_BACK_LEFT,
            Self::BackRight => AV_CH_BACK_RIGHT,
            Self::FrontLeftOfCenter => AV_CH_FRONT_LEFT_OF_CENTER,
            Self::FrontRightOfCenter => AV_CH_FRONT_RIGHT_OF_CENTER,
            Self::BackCenter => AV_CH_BACK_CENTER,
            Self::SideLeft => AV_CH_SIDE_LEFT,
            Self::SideRight => AV_CH_SIDE_RIGHT,
            Self::TopCenter => AV_CH_TOP_CENTER,
            Self::TopFrontLeft => AV_CH_TOP_FRONT_LEFT,
            Self::TopFrontCenter => AV_CH_TOP_FRONT_CENTER,
            Self::TopFrontRight => AV_CH_TOP_FRONT_RIGHT,
            Self::TopBackLeft => AV_CH_TOP_BACK_LEFT,
            Self::TopBackCenter => AV_CH_TOP_BACK_CENTER,
            Self::TopBackRight => AV_CH_TOP_BACK_RIGHT,
            Self::TopSideLeft => AV_CH_TOP_SIDE_LEFT,
            Self::TopSideRight => AV_CH_TOP_SIDE_RIGHT,
            Self::BottomFrontCenter => AV_CH_BOTTOM_FRONT_CENTER,
            Self::BottomFrontLeft => AV_CH_BOTTOM_FRONT_LEFT,
            Self::BottomFrontRight => AV_CH_BOTTOM_FRONT_RIGHT,
            Self::Unknown => 0,
        }
    }
}

/// Complete representation of an audio channel layout (AVChannelLayout).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ChannelLayout {
    pub nb_channels: u32,
    pub mask: u64,
}

impl ChannelLayout {
    pub const MONO: Self = Self {
        nb_channels: 1,
        mask: AV_CH_FRONT_CENTER,
    };

    pub const STEREO: Self = Self {
        nb_channels: 2,
        mask: AV_CH_FRONT_LEFT | AV_CH_FRONT_RIGHT,
    };

    pub const LAYOUT_2POINT1: Self = Self {
        nb_channels: 3,
        mask: AV_CH_FRONT_LEFT | AV_CH_FRONT_RIGHT | AV_CH_LOW_FREQUENCY,
    };

    pub const LAYOUT_2_1: Self = Self {
        nb_channels: 3,
        mask: AV_CH_FRONT_LEFT | AV_CH_FRONT_RIGHT | AV_CH_BACK_CENTER,
    };

    pub const SURROUND: Self = Self {
        nb_channels: 3,
        mask: AV_CH_FRONT_LEFT | AV_CH_FRONT_RIGHT | AV_CH_FRONT_CENTER,
    };

    pub const LAYOUT_3POINT1: Self = Self {
        nb_channels: 4,
        mask: AV_CH_FRONT_LEFT | AV_CH_FRONT_RIGHT | AV_CH_FRONT_CENTER | AV_CH_LOW_FREQUENCY,
    };

    pub const LAYOUT_4POINT0: Self = Self {
        nb_channels: 4,
        mask: AV_CH_FRONT_LEFT | AV_CH_FRONT_RIGHT | AV_CH_FRONT_CENTER | AV_CH_BACK_CENTER,
    };

    pub const QUAD: Self = Self {
        nb_channels: 4,
        mask: AV_CH_FRONT_LEFT | AV_CH_FRONT_RIGHT | AV_CH_BACK_LEFT | AV_CH_BACK_RIGHT,
    };

    pub const LAYOUT_5POINT0: Self = Self {
        nb_channels: 5,
        mask: AV_CH_FRONT_LEFT
            | AV_CH_FRONT_RIGHT
            | AV_CH_FRONT_CENTER
            | AV_CH_SIDE_LEFT
            | AV_CH_SIDE_RIGHT,
    };

    pub const LAYOUT_5POINT1: Self = Self {
        nb_channels: 6,
        mask: AV_CH_FRONT_LEFT
            | AV_CH_FRONT_RIGHT
            | AV_CH_FRONT_CENTER
            | AV_CH_LOW_FREQUENCY
            | AV_CH_SIDE_LEFT
            | AV_CH_SIDE_RIGHT,
    };

    pub const LAYOUT_5POINT1_BACK: Self = Self {
        nb_channels: 6,
        mask: AV_CH_FRONT_LEFT
            | AV_CH_FRONT_RIGHT
            | AV_CH_FRONT_CENTER
            | AV_CH_LOW_FREQUENCY
            | AV_CH_BACK_LEFT
            | AV_CH_BACK_RIGHT,
    };

    pub const LAYOUT_6POINT1: Self = Self {
        nb_channels: 7,
        mask: AV_CH_FRONT_LEFT
            | AV_CH_FRONT_RIGHT
            | AV_CH_FRONT_CENTER
            | AV_CH_LOW_FREQUENCY
            | AV_CH_BACK_CENTER
            | AV_CH_SIDE_LEFT
            | AV_CH_SIDE_RIGHT,
    };

    pub const LAYOUT_7POINT1: Self = Self {
        nb_channels: 8,
        mask: AV_CH_FRONT_LEFT
            | AV_CH_FRONT_RIGHT
            | AV_CH_FRONT_CENTER
            | AV_CH_LOW_FREQUENCY
            | AV_CH_BACK_LEFT
            | AV_CH_BACK_RIGHT
            | AV_CH_SIDE_LEFT
            | AV_CH_SIDE_RIGHT,
    };

    pub const LAYOUT_7POINT1_4: Self = Self {
        nb_channels: 12,
        mask: AV_CH_FRONT_LEFT
            | AV_CH_FRONT_RIGHT
            | AV_CH_FRONT_CENTER
            | AV_CH_LOW_FREQUENCY
            | AV_CH_BACK_LEFT
            | AV_CH_BACK_RIGHT
            | AV_CH_SIDE_LEFT
            | AV_CH_SIDE_RIGHT
            | AV_CH_TOP_FRONT_LEFT
            | AV_CH_TOP_FRONT_RIGHT
            | AV_CH_TOP_BACK_LEFT
            | AV_CH_TOP_BACK_RIGHT,
    };

    /// Create from channel count with unspecified mask.
    pub const fn from_channels(channels: u32) -> Self {
        match channels {
            1 => Self::MONO,
            2 => Self::STEREO,
            3 => Self::LAYOUT_2POINT1,
            4 => Self::QUAD,
            5 => Self::LAYOUT_5POINT0,
            6 => Self::LAYOUT_5POINT1,
            7 => Self::LAYOUT_6POINT1,
            8 => Self::LAYOUT_7POINT1,
            12 => Self::LAYOUT_7POINT1_4,
            n => Self {
                nb_channels: n,
                mask: 0,
            },
        }
    }

    /// Create from explicit bitmask.
    pub const fn from_mask(mask: u64) -> Self {
        Self {
            nb_channels: mask.count_ones(),
            mask,
        }
    }

    /// Check if this layout includes a specific channel.
    pub fn has_channel(&self, ch: Channel) -> bool {
        let m = ch.mask();
        m != 0 && (self.mask & m) == m
    }

    /// Return channel index (0-based) for a channel present in this layout.
    pub fn channel_index(&self, ch: Channel) -> Option<usize> {
        let m = ch.mask();
        if m == 0 || (self.mask & m) == 0 {
            return None;
        }
        let lower = self.mask & (m - 1);
        Some(lower.count_ones() as usize)
    }

    /// Get channel at 0-based index.
    pub fn channel_at_index(&self, index: usize) -> Option<Channel> {
        if index >= self.nb_channels as usize || self.mask == 0 {
            return None;
        }
        let mut count = 0;
        for i in 0..64 {
            let m = 1u64 << i;
            if (self.mask & m) != 0 {
                if count == index {
                    return Some(match m {
                        AV_CH_FRONT_LEFT => Channel::FrontLeft,
                        AV_CH_FRONT_RIGHT => Channel::FrontRight,
                        AV_CH_FRONT_CENTER => Channel::FrontCenter,
                        AV_CH_LOW_FREQUENCY => Channel::LowFrequency,
                        AV_CH_BACK_LEFT => Channel::BackLeft,
                        AV_CH_BACK_RIGHT => Channel::BackRight,
                        AV_CH_FRONT_LEFT_OF_CENTER => Channel::FrontLeftOfCenter,
                        AV_CH_FRONT_RIGHT_OF_CENTER => Channel::FrontRightOfCenter,
                        AV_CH_BACK_CENTER => Channel::BackCenter,
                        AV_CH_SIDE_LEFT => Channel::SideLeft,
                        AV_CH_SIDE_RIGHT => Channel::SideRight,
                        AV_CH_TOP_CENTER => Channel::TopCenter,
                        AV_CH_TOP_FRONT_LEFT => Channel::TopFrontLeft,
                        AV_CH_TOP_FRONT_CENTER => Channel::TopFrontCenter,
                        AV_CH_TOP_FRONT_RIGHT => Channel::TopFrontRight,
                        AV_CH_TOP_BACK_LEFT => Channel::TopBackLeft,
                        AV_CH_TOP_BACK_CENTER => Channel::TopBackCenter,
                        AV_CH_TOP_BACK_RIGHT => Channel::TopBackRight,
                        _ => Channel::Unknown,
                    });
                }
                count += 1;
            }
        }
        None
    }
}

impl fmt::Display for ChannelLayout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if *self == Self::MONO {
            write!(f, "mono")
        } else if *self == Self::STEREO {
            write!(f, "stereo")
        } else if *self == Self::LAYOUT_2POINT1 {
            write!(f, "2.1")
        } else if *self == Self::LAYOUT_5POINT1 || *self == Self::LAYOUT_5POINT1_BACK {
            write!(f, "5.1")
        } else if *self == Self::LAYOUT_7POINT1 {
            write!(f, "7.1")
        } else if *self == Self::LAYOUT_7POINT1_4 {
            write!(f, "7.1.4")
        } else {
            write!(f, "{} channels", self.nb_channels)
        }
    }
}

impl FromStr for ChannelLayout {
    type Err = RmediaError;
    fn from_str(s: &str) -> Result<Self> {
        let name = s.to_ascii_lowercase();
        match name.as_str() {
            "mono" | "1" => Ok(Self::MONO),
            "stereo" | "2" => Ok(Self::STEREO),
            "2.1" => Ok(Self::LAYOUT_2POINT1),
            "4.0" => Ok(Self::LAYOUT_4POINT0),
            "quad" => Ok(Self::QUAD),
            "5.0" => Ok(Self::LAYOUT_5POINT0),
            "5.1" => Ok(Self::LAYOUT_5POINT1),
            "6.1" => Ok(Self::LAYOUT_6POINT1),
            "7.1" => Ok(Self::LAYOUT_7POINT1),
            "7.1.4" => Ok(Self::LAYOUT_7POINT1_4),
            _ => {
                if let Ok(ch) = s.parse::<u32>() {
                    Ok(Self::from_channels(ch))
                } else {
                    Err(RmediaError::InvalidArgument(format!("Unknown channel layout: {}", s)))
                }
            }
        }
    }
}

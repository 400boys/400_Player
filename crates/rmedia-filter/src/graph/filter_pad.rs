//! Filter pad and format negotiation definitions (AVFilterPad reproduction).

use rmedia_codec::codec_id::MediaType;
use rmedia_util::channel_layout::ChannelLayout;
use rmedia_util::pixfmt::PixelFormat;
use rmedia_util::rational::Rational;
use rmedia_util::samplefmt::SampleFormat;

/// Direction of a filter pad.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadDirection {
    Input,
    Output,
}

/// Filter connection port (AVFilterPad).
#[derive(Debug, Clone, PartialEq)]
pub struct FilterPad {
    pub name: String,
    pub media_type: MediaType,
    pub direction: PadDirection,
    pub time_base: Rational,
    pub width: usize,
    pub height: usize,
    pub pix_fmt: PixelFormat,
    pub sample_rate: u32,
    pub sample_fmt: SampleFormat,
    pub ch_layout: ChannelLayout,
}

impl FilterPad {
    /// Create a video input or output pad.
    pub fn video(name: &str, dir: PadDirection) -> Self {
        Self {
            name: name.to_string(),
            media_type: MediaType::Video,
            direction: dir,
            time_base: Rational::new(1, 1000),
            width: 0,
            height: 0,
            pix_fmt: PixelFormat::None,
            sample_rate: 0,
            sample_fmt: SampleFormat::None,
            ch_layout: ChannelLayout::default(),
        }
    }

    /// Create an audio input or output pad.
    pub fn audio(name: &str, dir: PadDirection) -> Self {
        Self {
            name: name.to_string(),
            media_type: MediaType::Audio,
            direction: dir,
            time_base: Rational::new(1, 1000),
            width: 0,
            height: 0,
            pix_fmt: PixelFormat::None,
            sample_rate: 0,
            sample_fmt: SampleFormat::None,
            ch_layout: ChannelLayout::default(),
        }
    }
}

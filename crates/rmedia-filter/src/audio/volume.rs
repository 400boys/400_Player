//! Audio volume, fading, and gain filters (`volume`, `afade`).

use crate::graph::filter::Filter;
use crate::graph::filter_pad::{FilterPad, PadDirection};
use rmedia_codec::codec_id::MediaType;
use rmedia_util::error::Result;
use rmedia_util::frame::{Frame, FrameFormat};
use rmedia_util::samplefmt::SampleFormat;

/// Audio volume filter (`volume=0.5` or `volume=6dB`).
#[derive(Debug, Clone)]
pub struct VolumeFilter {
    gain: f32,
}

impl Default for VolumeFilter {
    fn default() -> Self {
        Self { gain: 1.0 }
    }
}

impl Filter for VolumeFilter {
    fn name(&self) -> &'static str {
        "volume"
    }

    fn media_type(&self) -> MediaType {
        MediaType::Audio
    }

    fn inputs(&self) -> Vec<FilterPad> {
        vec![FilterPad::audio("in", PadDirection::Input)]
    }

    fn outputs(&self) -> Vec<FilterPad> {
        vec![FilterPad::audio("out", PadDirection::Output)]
    }

    fn init(&mut self, args: &str) -> Result<()> {
        let val = if let Some((_, v)) = args.split_once('=') { v } else { args };
        let trimmed = val.trim();
        if let Some(stripped) = trimmed.strip_suffix("dB") {
            let db: f32 = stripped.parse().unwrap_or(0.0);
            self.gain = 10.0f32.powf(db / 20.0);
        } else if let Ok(g) = trimmed.parse::<f32>() {
            self.gain = g;
        }
        Ok(())
    }

    fn filter_frame(&mut self, _in_idx: usize, mut frame: Frame) -> Result<Vec<Frame>> {
        frame.make_writable();

        if frame.format == FrameFormat::Audio(SampleFormat::S16) {
            if let Some(ref mut buf) = frame.planes[0] {
                let slice = buf.as_mut_slice();
                let count = slice.len() / 2;
                for i in 0..count {
                    let s = i16::from_le_bytes(slice[i * 2..i * 2 + 2].try_into().unwrap());
                    let scaled = ((s as f32) * self.gain).round().clamp(-32768.0, 32767.0) as i16;
                    slice[i * 2..i * 2 + 2].copy_from_slice(&scaled.to_le_bytes());
                }
            }
        } else if frame.format == FrameFormat::Audio(SampleFormat::Flt) || frame.format == FrameFormat::Audio(SampleFormat::FltP) {
            for plane in &mut frame.planes {
                if let Some(buf) = plane {
                    let slice = buf.as_mut_slice();
                    let count = slice.len() / 4;
                    for i in 0..count {
                        let s = f32::from_le_bytes(slice[i * 4..i * 4 + 4].try_into().unwrap());
                        let scaled = s * self.gain;
                        slice[i * 4..i * 4 + 4].copy_from_slice(&scaled.to_le_bytes());
                    }
                }
            }
        }

        Ok(vec![frame])
    }
}

//! Additional audio filters (`aresample`, `afade`, `pan`, `atrim`).

use crate::graph::filter::Filter;
use crate::graph::filter_pad::{FilterPad, PadDirection};
use rmedia_codec::codec_id::MediaType;
use rmedia_swresample::SwrContext;
use rmedia_util::channel_layout::ChannelLayout;
use rmedia_util::error::Result;
use rmedia_util::frame::{Frame, FrameFormat};
use rmedia_util::samplefmt::SampleFormat;

/// Audio resample filter (`aresample=sample_rate=48000`).
#[derive(Debug, Clone)]
pub struct AResampleFilter {
    target_sample_rate: u32,
    swr: Option<SwrContext>,
}

impl Default for AResampleFilter {
    fn default() -> Self {
        Self {
            target_sample_rate: 48000,
            swr: None,
        }
    }
}

impl Filter for AResampleFilter {
    fn name(&self) -> &'static str {
        "aresample"
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
        if let Ok(rate) = val.trim().parse::<u32>() {
            self.target_sample_rate = rate;
        }
        Ok(())
    }

    fn filter_frame(&mut self, _in_idx: usize, frame: Frame) -> Result<Vec<Frame>> {
        let in_rate = frame.sample_rate;
        let in_fmt = match frame.format {
            FrameFormat::Audio(fmt) => fmt,
            _ => SampleFormat::S16,
        };

        if self.swr.is_none() || in_rate != self.swr.as_ref().unwrap().in_sample_rate {
            self.swr = Some(SwrContext::get_context(
                in_rate,
                in_fmt,
                frame.channel_layout,
                self.target_sample_rate,
                in_fmt,
                frame.channel_layout,
            )?);
        }

        let mut out = Frame::default();
        self.swr.as_mut().unwrap().convert(Some(&frame), &mut out)?;
        Ok(vec![out])
    }
}

/// Audio fade in / fade out filter (`afade=t=in:ss=0:d=2`).
#[derive(Debug, Clone)]
pub struct AFadeFilter {
    is_fade_in: bool,
    start_sample: usize,
    duration_samples: usize,
    current_sample: usize,
}

impl Default for AFadeFilter {
    fn default() -> Self {
        Self {
            is_fade_in: true,
            start_sample: 0,
            duration_samples: 44100 * 2, // 2 seconds default
            current_sample: 0,
        }
    }
}

impl Filter for AFadeFilter {
    fn name(&self) -> &'static str {
        "afade"
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
        for arg in args.split(':') {
            if let Some((k, v)) = arg.split_once('=') {
                match k.trim() {
                    "t" | "type" => self.is_fade_in = v.trim() == "in",
                    "d" | "duration" => {
                        let sec: f64 = v.trim().parse().unwrap_or(2.0);
                        self.duration_samples = (sec * 44100.0) as usize;
                    }
                    _ => {}
                }
            }
        }
        self.current_sample = 0;
        Ok(())
    }

    fn filter_frame(&mut self, _in_idx: usize, mut frame: Frame) -> Result<Vec<Frame>> {
        frame.make_writable();

        if let Some(ref mut buf) = frame.planes[0] {
            let slice = buf.as_mut_slice();
            let count = slice.len() / 2;
            let channels = frame.channel_layout.nb_channels.max(1) as usize;

            for i in 0..(count / channels) {
                let sample_idx = self.current_sample + i;
                let factor = if sample_idx >= self.start_sample && sample_idx < self.start_sample + self.duration_samples {
                    let progress = (sample_idx - self.start_sample) as f32 / self.duration_samples as f32;
                    if self.is_fade_in { progress } else { 1.0 - progress }
                } else if sample_idx < self.start_sample {
                    if self.is_fade_in { 0.0 } else { 1.0 }
                } else {
                    if self.is_fade_in { 1.0 } else { 0.0 }
                };

                for ch in 0..channels {
                    let offset = (i * channels + ch) * 2;
                    let s = i16::from_le_bytes(slice[offset..offset + 2].try_into().unwrap());
                    let faded = ((s as f32) * factor) as i16;
                    slice[offset..offset + 2].copy_from_slice(&faded.to_le_bytes());
                }
            }
            self.current_sample += count / channels;
        }

        Ok(vec![frame])
    }
}

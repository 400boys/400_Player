//! SwrContext resampling, format conversion, and channel mixing coordinator.

use crate::converter::{bytes_to_f32, f32_to_bytes};
use crate::rematrix::ChannelMatrix;
use crate::resampler::ResamplerChannel;
use rmedia_util::channel_layout::ChannelLayout;
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::frame::{Frame, FrameFormat};
use rmedia_util::samplefmt::SampleFormat;

/// Audio resampler, format converter, and rematrixing context (AVSwrContext).
#[derive(Debug, Clone)]
pub struct SwrContext {
    pub in_sample_rate: u32,
    pub out_sample_rate: u32,
    pub in_sample_fmt: SampleFormat,
    pub out_sample_fmt: SampleFormat,
    pub in_ch_layout: ChannelLayout,
    pub out_ch_layout: ChannelLayout,

    rematrix: ChannelMatrix,
    resamplers: Vec<ResamplerChannel>,
}

impl SwrContext {
    /// Create and initialize an audio resampling context.
    pub fn get_context(
        in_sample_rate: u32,
        in_sample_fmt: SampleFormat,
        in_ch_layout: ChannelLayout,
        out_sample_rate: u32,
        out_sample_fmt: SampleFormat,
        out_ch_layout: ChannelLayout,
    ) -> Result<Self> {
        if in_sample_rate == 0 || out_sample_rate == 0 {
            return Err(RmediaError::InvalidArgument("Sample rates must be positive".into()));
        }

        let rematrix = ChannelMatrix::build(in_ch_layout, out_ch_layout);
        let out_channels = out_ch_layout.nb_channels as usize;
        let mut resamplers = Vec::with_capacity(out_channels);

        for _ in 0..out_channels {
            resamplers.push(ResamplerChannel::new(in_sample_rate, out_sample_rate));
        }

        Ok(Self {
            in_sample_rate,
            out_sample_rate,
            in_sample_fmt,
            out_sample_fmt,
            in_ch_layout,
            out_ch_layout,
            rematrix,
            resamplers,
        })
    }

    /// Convert audio frame samples according to configured sample rate, channels, and format.
    pub fn convert(&mut self, in_frame: Option<&Frame>, out_frame: &mut Frame) -> Result<()> {
        let Some(inf) = in_frame else {
            return Ok(());
        };

        let in_samples = inf.nb_samples;
        let in_ch = inf.channel_layout.nb_channels as usize;
        let out_ch = self.out_ch_layout.nb_channels as usize;

        // Step 1: Decode input into planar normalized f32
        let mut in_f32: Vec<Vec<f32>> = vec![vec![0.0f32; in_samples]; in_ch];
        if inf.format == FrameFormat::Audio(self.in_sample_fmt) {
            if self.in_sample_fmt.is_planar() {
                for ch in 0..in_ch {
                    if let Some(ref buf) = inf.planes[ch] {
                        bytes_to_f32(buf.as_slice(), self.in_sample_fmt, &mut in_f32[ch]);
                    }
                }
            } else if let Some(ref buf) = inf.planes[0] {
                let bytes_per_sample = self.in_sample_fmt.bytes_per_sample();
                let slice = buf.as_slice();
                for s in 0..in_samples {
                    for ch in 0..in_ch {
                        let offset = (s * in_ch + ch) * bytes_per_sample;
                        let mut single = [0.0f32; 1];
                        bytes_to_f32(&slice[offset..offset + bytes_per_sample], self.in_sample_fmt, &mut single);
                        in_f32[ch][s] = single[0];
                    }
                }
            }
        }

        // Step 2: Rematrix channels to output layout
        let in_slices: Vec<&[f32]> = in_f32.iter().map(|v| v.as_slice()).collect();
        let mut rematrixed: Vec<Vec<f32>> = vec![vec![0.0f32; in_samples]; out_ch];
        {
            let mut out_slices: Vec<&mut [f32]> = rematrixed.iter_mut().map(|v| v.as_mut_slice()).collect();
            self.rematrix.process_f32(&in_slices, &mut out_slices, in_samples);
        }

        // Step 3: Resample if sample rates differ
        let out_samples = if self.in_sample_rate != self.out_sample_rate {
            let est_out = ((in_samples as u64 * self.out_sample_rate as u64) / self.in_sample_rate as u64) as usize;
            let mut resampled: Vec<Vec<f32>> = vec![vec![0.0f32; est_out + 64]; out_ch];
            let mut final_count = est_out;

            for ch in 0..out_ch {
                let count = self.resamplers[ch].process(&rematrixed[ch], &mut resampled[ch]);
                final_count = count;
            }

            for ch in 0..out_ch {
                resampled[ch].truncate(final_count);
            }
            rematrixed = resampled;
            final_count
        } else {
            in_samples
        };

        // Step 4: Encode to output frame format
        *out_frame = Frame::alloc_audio(
            out_samples,
            self.out_sample_rate,
            self.out_ch_layout,
            self.out_sample_fmt,
        )?;
        out_frame.pts = inf.pts;

        if self.out_sample_fmt.is_planar() {
            for ch in 0..out_ch {
                if let Some(ref mut buf) = out_frame.planes[ch] {
                    f32_to_bytes(&rematrixed[ch], self.out_sample_fmt, buf.as_mut_slice());
                }
            }
        } else if let Some(ref mut buf) = out_frame.planes[0] {
            let bytes_per_sample = self.out_sample_fmt.bytes_per_sample();
            let slice = buf.as_mut_slice();
            for s in 0..out_samples {
                for ch in 0..out_ch {
                    let offset = (s * out_ch + ch) * bytes_per_sample;
                    let single = [rematrixed[ch][s]];
                    f32_to_bytes(&single, self.out_sample_fmt, &mut slice[offset..offset + bytes_per_sample]);
                }
            }
        }

        Ok(())
    }
}

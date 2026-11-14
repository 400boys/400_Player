//! Rmedia Swresample (`libswresample` reproduction in 100% pure Rust).
//!
//! Audio resampling, sample format conversion (U8, S16, S32, FLT, DBL), and channel rematrixing.

pub mod context;
pub mod converter;
pub mod rematrix;
pub mod resampler;

pub use context::SwrContext;
pub use converter::{bytes_to_f32, f32_to_bytes};
pub use rematrix::ChannelMatrix;
pub use resampler::ResamplerChannel;

#[cfg(test)]
mod tests {
    use super::*;
    use rmedia_util::channel_layout::ChannelLayout;
    use rmedia_util::frame::Frame;
    use rmedia_util::samplefmt::SampleFormat;

    #[test]
    fn test_swr_channel_rematrix() {
        let mut swr = SwrContext::get_context(
            44100, SampleFormat::S16, ChannelLayout::STEREO,
            44100, SampleFormat::S16, ChannelLayout::MONO,
        ).unwrap();

        let in_frame = Frame::alloc_audio(256, 44100, ChannelLayout::STEREO, SampleFormat::S16).unwrap();
        let mut out_frame = Frame::default();

        swr.convert(Some(&in_frame), &mut out_frame).unwrap();
        assert_eq!(out_frame.channel_layout, ChannelLayout::MONO);
        assert_eq!(out_frame.nb_samples, 256);
    }

    #[test]
    fn test_swr_resample_rate() {
        let mut swr = SwrContext::get_context(
            48000, SampleFormat::FltP, ChannelLayout::STEREO,
            44100, SampleFormat::FltP, ChannelLayout::STEREO,
        ).unwrap();

        let in_frame = Frame::alloc_audio(480, 48000, ChannelLayout::STEREO, SampleFormat::FltP).unwrap();
        let mut out_frame = Frame::default();

        swr.convert(Some(&in_frame), &mut out_frame).unwrap();
        assert_eq!(out_frame.sample_rate, 44100);
        assert!(out_frame.nb_samples > 400 && out_frame.nb_samples < 500);
    }
}

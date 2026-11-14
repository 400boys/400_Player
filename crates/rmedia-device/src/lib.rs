//! Rmedia Device (`libavdevice` reproduction in 100% pure Rust).
//!
//! Virtual audio and video devices, lavfi filtergraph input device, and headless playback/render sinks.

pub mod lavfi;
pub mod virtual_device;

pub use lavfi::LavfiDevice;
pub use virtual_device::{generate_smpte_bars_yuv420p, HeadlessAudioSink, HeadlessVideoSink};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_smpte_bars_generator() {
        let frame = generate_smpte_bars_yuv420p(320, 240).unwrap();
        assert_eq!(frame.width, 320);
        assert_eq!(frame.height, 240);
        assert!(frame.planes[0].is_some());
    }

    #[test]
    fn test_headless_sinks() {
        let mut audio_sink = HeadlessAudioSink::new(48000, 2);
        let audio_frame = rmedia_util::frame::Frame::alloc_audio(
            1024, 48000,
            rmedia_util::channel_layout::ChannelLayout::STEREO,
            rmedia_util::samplefmt::SampleFormat::S16,
        ).unwrap();
        audio_sink.write_frame(&audio_frame).unwrap();
        assert_eq!(audio_sink.total_samples, 1024);

        let mut video_sink = HeadlessVideoSink::new(640, 480);
        let video_frame = generate_smpte_bars_yuv420p(640, 480).unwrap();
        video_sink.render_frame(&video_frame).unwrap();
        assert_eq!(video_sink.total_frames, 1);
        assert_ne!(video_sink.last_frame_hash, 0);
    }
}

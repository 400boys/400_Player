//! Rmedia Filter (`libavfilter` reproduction in 100% pure Rust).
//!
//! Directed Acyclic Graph (DAG) filter execution engine, complex filtergraph syntax parser,
//! video and audio filter processing blocks.

pub mod audio;
pub mod graph;
pub mod video;

pub use audio::channels::ASplitFilter;
pub use audio::extra::{AFadeFilter, AResampleFilter};
pub use audio::source_sink::{ABufferFilter, ABufferSinkFilter, ANullFilter};
pub use audio::volume::VolumeFilter;
pub use graph::*;
pub use video::composite::{NullFilter, OverlayFilter, SplitFilter};
pub use video::extra::{BoxBlurFilter, NegateFilter, PadFilter, RotateFilter};
pub use video::source_sink::{BufferFilter, BufferSinkFilter};
pub use video::timing::{FpsFilter, SetPtsFilter};
pub use video::transform::{CropFilter, HFlipFilter, ScaleFilter, VFlipFilter};

/// Lookup and instantiate a filter by name.
pub fn find_filter(name: &str) -> Option<Box<dyn Filter>> {
    match name {
        "scale" => Some(Box::new(ScaleFilter::default())),
        "hflip" => Some(Box::new(HFlipFilter)),
        "vflip" => Some(Box::new(VFlipFilter)),
        "crop" => Some(Box::new(CropFilter::default())),
        "pad" => Some(Box::new(PadFilter::default())),
        "rotate" => Some(Box::new(RotateFilter::default())),
        "boxblur" => Some(Box::new(BoxBlurFilter::default())),
        "negate" => Some(Box::new(NegateFilter)),
        "null" => Some(Box::new(NullFilter)),
        "split" => Some(Box::new(SplitFilter::default())),
        "overlay" => Some(Box::new(OverlayFilter::default())),
        "fps" => Some(Box::new(FpsFilter::default())),
        "setpts" => Some(Box::new(SetPtsFilter::default())),
        "buffer" => Some(Box::new(BufferFilter)),
        "buffersink" => Some(Box::new(BufferSinkFilter::new())),

        "volume" => Some(Box::new(VolumeFilter::default())),
        "aresample" => Some(Box::new(AResampleFilter::default())),
        "afade" => Some(Box::new(AFadeFilter::default())),
        "asplit" => Some(Box::new(ASplitFilter::default())),
        "abuffer" => Some(Box::new(ABufferFilter)),
        "abuffersink" => Some(Box::new(ABufferSinkFilter::new())),
        "anull" => Some(Box::new(ANullFilter)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmedia_util::frame::Frame;
    use rmedia_util::pixfmt::PixelFormat;

    #[test]
    fn test_scale_filter() {
        let mut filter = find_filter("scale").unwrap();
        filter.init("w=32:h=24").unwrap();

        let src = Frame::alloc_video(64, 48, PixelFormat::Yuv420p, 64).unwrap();
        let out = filter.filter_frame(0, src).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].width, 32);
        assert_eq!(out[0].height, 24);
    }

    #[test]
    fn test_volume_filter() {
        let mut filter = find_filter("volume").unwrap();
        filter.init("0.5").unwrap();

        let mut src = Frame::alloc_audio(100, 44100, rmedia_util::channel_layout::ChannelLayout::MONO, rmedia_util::samplefmt::SampleFormat::S16).unwrap();
        if let Some(ref mut buf) = src.planes[0] {
            // Fill with 1000
            let slice = buf.as_mut_slice();
            for i in 0..100 {
                slice[i * 2..i * 2 + 2].copy_from_slice(&1000i16.to_le_bytes());
            }
        }

        let out = filter.filter_frame(0, src).unwrap();
        let slice = out[0].planes[0].as_ref().unwrap().as_slice();
        let s0 = i16::from_le_bytes(slice[0..2].try_into().unwrap());
        assert_eq!(s0, 500);
    }
}

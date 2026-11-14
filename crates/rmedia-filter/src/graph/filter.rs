//! Filter trait interface (AVFilter reproduction).

use super::filter_pad::FilterPad;
use rmedia_codec::codec_id::MediaType;
use rmedia_util::error::Result;
use rmedia_util::frame::Frame;

/// Filter trait interface implemented by all audio and video filters (AVFilter).
pub trait Filter: Send + Sync {
    /// Return the filter name (e.g. "scale", "volume", "overlay").
    fn name(&self) -> &'static str;

    /// Return primary media type of the filter.
    fn media_type(&self) -> MediaType;

    /// List input pads.
    fn inputs(&self) -> Vec<FilterPad>;

    /// List output pads.
    fn outputs(&self) -> Vec<FilterPad>;

    /// Initialize the filter with string parameters (e.g. "w=1280:h=720" or "volume=0.5").
    fn init(&mut self, args: &str) -> Result<()>;

    /// Process an incoming frame on the given input pad index; returns output frames.
    fn filter_frame(&mut self, in_idx: usize, frame: Frame) -> Result<Vec<Frame>>;

    /// Flush remaining cached frames.
    fn flush(&mut self) -> Result<Vec<Frame>> {
        Ok(Vec::new())
    }
}

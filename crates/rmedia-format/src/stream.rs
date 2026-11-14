//! Media stream representation (AVStream reproduction).

use rmedia_codec::codec_id::{CodecId, MediaType};
use rmedia_codec::context::CodecContext;
use rmedia_util::dict::Dictionary;
use rmedia_util::rational::{Rational, NOPTS_VALUE};

/// Elementary audio, video, subtitle, or data stream inside a container (AVStream).
#[derive(Debug, Clone, PartialEq)]
pub struct Stream {
    pub index: usize,
    pub id: i32,
    pub codec_id: CodecId,
    pub media_type: MediaType,
    pub time_base: Rational,
    pub start_time: i64,
    pub duration: i64,
    pub nb_frames: i64,
    pub sample_aspect_ratio: Rational,
    pub avg_frame_rate: Rational,
    pub r_frame_rate: Rational,
    pub metadata: Dictionary,
    pub codec_context: CodecContext,
}

impl Stream {
    /// Create a new stream with the specified index and codec ID.
    pub fn new(index: usize, codec_id: CodecId) -> Self {
        let media_type = codec_id.media_type();
        Self {
            index,
            id: index as i32,
            codec_id,
            media_type,
            time_base: Rational::new(1, 1000),
            start_time: NOPTS_VALUE,
            duration: NOPTS_VALUE,
            nb_frames: 0,
            sample_aspect_ratio: Rational::new(1, 1),
            avg_frame_rate: Rational::new(25, 1),
            r_frame_rate: Rational::new(25, 1),
            metadata: Dictionary::new(),
            codec_context: CodecContext::new(codec_id),
        }
    }
}

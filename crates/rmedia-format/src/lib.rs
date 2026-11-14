//! Rmedia Format (`libavformat` reproduction in 100% pure Rust).
//!
//! I/O streams, protocols, container format probing, muxing and demuxing.

pub mod containers;
pub mod format_context;
pub mod io;
pub mod probe;
pub mod stream;

pub use containers::*;
pub use format_context::{Chapter, Demuxer, FormatContext, Muxer};
pub use io::{CustomStream, IoContext, StreamBackend};
pub use probe::{find_demuxer, find_muxer, probe_input_format};
pub use stream::Stream;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_probe() {
        assert_eq!(probe_input_format(b"RIFF\x00\x00\x00\x00WAVE"), Some("wav"));
        assert_eq!(probe_input_format(b"YUV4MPEG2 W10 H10 F25:1\n"), Some("yuv4mpegpipe"));
        assert_eq!(probe_input_format(b"DKIF\x00\x00"), Some("ivf"));
        assert_eq!(probe_input_format(b"\x00\x00\x00\x20ftypisom"), Some("mp4"));
        assert_eq!(probe_input_format(b"\x1A\x45\xDF\xA3"), Some("matroska"));
        assert_eq!(probe_input_format(b"FLV\x01"), Some("flv"));
        assert_eq!(probe_input_format(b"WEBVTT\n\n"), Some("webvtt"));
    }
}

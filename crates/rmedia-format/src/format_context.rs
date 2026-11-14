//! Format context and Demuxer/Muxer traits (AVFormatContext reproduction).

use crate::io::io_context::IoContext;
use crate::stream::Stream;
use rmedia_codec::codec_id::CodecId;
use rmedia_util::dict::Dictionary;
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::packet::Packet;
use rmedia_util::rational::{Rational, NOPTS_VALUE};

/// Chapter metadata structure (AVChapter).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chapter {
    pub id: i64,
    pub time_base: Rational,
    pub start: i64,
    pub end: i64,
    pub metadata: Dictionary,
}

/// Multimedia container format context (AVFormatContext).
pub struct FormatContext {
    pub io: Option<IoContext>,
    pub streams: Vec<Stream>,
    pub metadata: Dictionary,
    pub duration: i64,
    pub start_time: i64,
    pub bit_rate: i64,
    pub chapters: Vec<Chapter>,
    pub url: String,
}

impl Default for FormatContext {
    fn default() -> Self {
        Self {
            io: None,
            streams: Vec::new(),
            metadata: Dictionary::new(),
            duration: NOPTS_VALUE,
            start_time: NOPTS_VALUE,
            bit_rate: 0,
            chapters: Vec::new(),
            url: String::new(),
        }
    }
}

impl FormatContext {
    /// Create a new empty format context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Open an input file and return an initialized FormatContext with an IoContext attached.
    pub fn open_input_file(path: &str) -> Result<Self> {
        let io = IoContext::open_file_read(path)?;
        let mut ctx = Self::default();
        ctx.io = Some(io);
        ctx.url = path.to_string();
        Ok(ctx)
    }

    /// Create an output file and return a FormatContext with an IoContext attached.
    pub fn open_output_file(path: &str) -> Result<Self> {
        let io = IoContext::open_file_write(path)?;
        let mut ctx = Self::default();
        ctx.io = Some(io);
        ctx.url = path.to_string();
        Ok(ctx)
    }

    /// Add a new stream to the context and return its index.
    pub fn new_stream(&mut self, codec_id: CodecId) -> usize {
        let idx = self.streams.len();
        self.streams.push(Stream::new(idx, codec_id));
        idx
    }

    /// Access stream by index.
    pub fn stream(&self, idx: usize) -> Option<&Stream> {
        self.streams.get(idx)
    }

    /// Mutable access to stream by index.
    pub fn stream_mut(&mut self, idx: usize) -> Option<&mut Stream> {
        self.streams.get_mut(idx)
    }
}

/// Abstract demuxer interface (AVInputFormat reproduction).
pub trait Demuxer: Send + Sync {
    /// Demuxer format short name.
    fn name(&self) -> &'static str;

    /// Read container headers, metadata, and stream declarations.
    fn read_header(&mut self, ctx: &mut FormatContext) -> Result<()>;

    /// Read next compressed packet.
    fn read_packet(&mut self, ctx: &mut FormatContext, pkt: &mut Packet) -> Result<()>;

    /// Seek to specified timestamp in a stream.
    fn seek(&mut self, ctx: &mut FormatContext, stream_idx: i32, timestamp: i64, flags: u32) -> Result<()>;
}

/// Abstract muxer interface (AVOutputFormat reproduction).
pub trait Muxer: Send + Sync {
    /// Muxer format short name.
    fn name(&self) -> &'static str;

    /// Write container file header.
    fn write_header(&mut self, ctx: &mut FormatContext) -> Result<()>;

    /// Write compressed packet into container.
    fn write_packet(&mut self, ctx: &mut FormatContext, pkt: &mut Packet) -> Result<()>;

    /// Write container trailer and finalize index/metadata.
    fn write_trailer(&mut self, ctx: &mut FormatContext) -> Result<()>;
}

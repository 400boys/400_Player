//! Codec Context and Decoder/Encoder traits (AVCodecContext reproduction).

use rmedia_util::channel_layout::ChannelLayout;
use rmedia_util::dict::Dictionary;
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::frame::Frame;
use rmedia_util::packet::Packet;
use rmedia_util::pixfmt::PixelFormat;
use rmedia_util::rational::Rational;
use rmedia_util::samplefmt::SampleFormat;
use super::codec_id::{CodecId, MediaType};

/// Codec flags matching FFmpeg AV_CODEC_FLAG_*.
pub const AV_CODEC_FLAG_UNALIGNED: u32 = 1 << 0;
pub const AV_CODEC_FLAG_QSCALE: u32 = 1 << 1;
pub const AV_CODEC_FLAG_4MV: u32 = 1 << 2;
pub const AV_CODEC_FLAG_OUTPUT_CORRUPT: u32 = 1 << 3;
pub const AV_CODEC_FLAG_QPEL: u32 = 1 << 4;
pub const AV_CODEC_FLAG_DROPCHANGED: u32 = 1 << 5;
pub const AV_CODEC_FLAG_PASS1: u32 = 1 << 9;
pub const AV_CODEC_FLAG_PASS2: u32 = 1 << 10;
pub const AV_CODEC_FLAG_LOOP_FILTER: u32 = 1 << 11;
pub const AV_CODEC_FLAG_GRAY: u32 = 1 << 13;
pub const AV_CODEC_FLAG_PSNR: u32 = 1 << 15;
pub const AV_CODEC_FLAG_INTERLACED_DCT: u32 = 1 << 18;
pub const AV_CODEC_FLAG_LOW_DELAY: u32 = 1 << 19;
pub const AV_CODEC_FLAG_GLOBAL_HEADER: u32 = 1 << 22;

/// Central codec execution context (AVCodecContext).
#[derive(Debug, Clone, PartialEq)]
pub struct CodecContext {
    pub codec_id: CodecId,
    pub media_type: MediaType,
    pub bit_rate: i64,
    pub width: usize,
    pub height: usize,
    pub pix_fmt: PixelFormat,
    pub sample_rate: u32,
    pub sample_fmt: SampleFormat,
    pub ch_layout: ChannelLayout,
    pub time_base: Rational,
    pub framerate: Rational,
    pub extradata: Vec<u8>,
    pub gop_size: i32,
    pub max_b_frames: i32,
    pub qmin: i32,
    pub qmax: i32,
    pub flags: u32,
    pub options: Dictionary,
}

impl Default for CodecContext {
    fn default() -> Self {
        Self {
            codec_id: CodecId::None,
            media_type: MediaType::Unknown,
            bit_rate: 0,
            width: 0,
            height: 0,
            pix_fmt: PixelFormat::None,
            sample_rate: 0,
            sample_fmt: SampleFormat::None,
            ch_layout: ChannelLayout::default(),
            time_base: Rational::new(1, 1000),
            framerate: Rational::new(25, 1),
            extradata: Vec::new(),
            gop_size: 12,
            max_b_frames: 0,
            qmin: 2,
            qmax: 31,
            flags: 0,
            options: Dictionary::new(),
        }
    }
}

impl CodecContext {
    /// Create a new CodecContext configured for a specific codec.
    pub fn new(codec_id: CodecId) -> Self {
        let mut ctx = Self::default();
        ctx.codec_id = codec_id;
        ctx.media_type = codec_id.media_type();
        ctx
    }
}

/// Abstract decoder interface (avcodec_send_packet / avcodec_receive_frame).
pub trait Decoder: Send + Sync {
    /// Return the CodecId supported by this decoder.
    fn codec_id(&self) -> CodecId;

    /// Supply a raw packet to the decoder. Pass None to flush.
    fn send_packet(&mut self, pkt: Option<&Packet>) -> Result<()>;

    /// Retrieve a decoded uncompressed frame.
    fn receive_frame(&mut self, frame: &mut Frame) -> Result<()>;

    /// Flush the decoder internal buffers.
    fn flush(&mut self);
}

/// Abstract encoder interface (avcodec_send_frame / avcodec_receive_packet).
pub trait Encoder: Send + Sync {
    /// Return the CodecId produced by this encoder.
    fn codec_id(&self) -> CodecId;

    /// Supply an uncompressed frame to the encoder. Pass None to flush.
    fn send_frame(&mut self, frame: Option<&Frame>) -> Result<()>;

    /// Retrieve a compressed packet from the encoder.
    fn receive_packet(&mut self, pkt: &mut Packet) -> Result<()>;

    /// Flush the encoder internal buffers.
    fn flush(&mut self);
}

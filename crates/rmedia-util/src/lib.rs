//! Rmedia Util (`libavutil` reproduction in 100% pure Rust).
//!
//! Provides core data structures, rational arithmetic, memory buffers, frame representations,
//! pixel/sample formats, colorimetry, options, logging, cryptography, and fast DSP transforms.

pub mod buffer;
pub mod channel_layout;
pub mod color;
pub mod crypto;
pub mod dict;
pub mod error;
pub mod frame;
pub mod log;
pub mod opt;
pub mod packet;
pub mod pixfmt;
pub mod rational;
pub mod samplefmt;
pub mod tx;

pub use buffer::{AlignedBuffer, BufferPool, BufferRef, AV_INPUT_BUFFER_PADDING_SIZE};
pub use channel_layout::{Channel, ChannelLayout};
pub use color::{
    ChromaLocation, ColorMatrixCoeffs, ColorPrimaries, ColorRange, ColorSpace,
    ColorTransferCharacteristic, ContentLightLevel, MasteringDisplayMetadata,
};
pub use dict::{Dictionary, AV_DICT_APPEND, AV_DICT_DONT_OVERWRITE, AV_DICT_MATCH_CASE};
pub use error::{Result, RmediaError, AVERROR_EOF, AVERROR_EAGAIN, AVERROR_EINVAL, AVERROR_ENOMEM};
pub use frame::{Frame, FrameFormat, FrameSideData, FrameSideDataType, PictureType};
pub use log::{get_log_level, log, set_log_callback, set_log_level, LogLevel};
pub use opt::{AvClass, AvOption, OptionContext, OptionType};
pub use packet::{Packet, PacketSideData, PacketSideDataType, AV_PKT_FLAG_KEY};
pub use pixfmt::{PixelFormat, PixelFormatDescriptor};
pub use rational::{compare_ts, rescale, rescale_q, rescale_rnd, Rational, Rounding, NOPTS_VALUE};
pub use samplefmt::SampleFormat;

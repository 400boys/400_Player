//! Error handling module for Rmedia (libavutil error reproduction).

use std::fmt;

/// Predefined AV-compatible error tags.
pub const AVERROR_EOF: i32 = -libc_eof();
pub const AVERROR_EAGAIN: i32 = -11;
pub const AVERROR_EINVAL: i32 = -22;
pub const AVERROR_ENOMEM: i32 = -12;
pub const AVERROR_ENOENT: i32 = -2;
pub const AVERROR_EIO: i32 = -5;
pub const AVERROR_ENOSYS: i32 = -38;
pub const AVERROR_INVALIDDATA: i32 = -make_error_tag(b'I', b'N', b'D', b'A');
pub const AVERROR_DECODER_NOT_FOUND: i32 = -make_error_tag(0xF8, b'D', b'E', b'C');
pub const AVERROR_DEMUXER_NOT_FOUND: i32 = -make_error_tag(0xF8, b'D', b'E', b'M');
pub const AVERROR_ENCODER_NOT_FOUND: i32 = -make_error_tag(0xF8, b'E', b'N', b'C');
pub const AVERROR_MUXER_NOT_FOUND: i32 = -make_error_tag(0xF8, b'M', b'U', b'X');
pub const AVERROR_FILTER_NOT_FOUND: i32 = -make_error_tag(0xF8, b'F', b'I', b'L');
pub const AVERROR_BSF_NOT_FOUND: i32 = -make_error_tag(0xF8, b'B', b'S', b'F');
pub const AVERROR_PROTOCOL_NOT_FOUND: i32 = -make_error_tag(0xF8, b'P', b'R', b'O');

const fn make_error_tag(a: u8, b: u8, c: u8, d: u8) -> i32 {
    (a as i32) | ((b as i32) << 8) | ((c as i32) << 16) | ((d as i32) << 24)
}

const fn libc_eof() -> i32 {
    make_error_tag(b'E', b'O', b'F', b' ')
}

/// Result type alias for Rmedia operations.
pub type Result<T> = std::result::Result<T, RmediaError>;

/// Unified error enum for the entire Rmedia ecosystem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RmediaError {
    /// Resource temporarily unavailable, try again later (AVERROR(EAGAIN)).
    TryAgain,
    /// End of stream or file reached (AVERROR_EOF).
    EndOfFile,
    /// Invalid argument supplied (AVERROR(EINVAL)).
    InvalidArgument(String),
    /// Out of memory allocation failure (AVERROR(ENOMEM)).
    OutOfMemory,
    /// Requested entity, file or stream not found (AVERROR(ENOENT)).
    NotFound(String),
    /// Input/output error on device or network (AVERROR(EIO)).
    Io(String),
    /// Operation or feature not implemented (AVERROR(ENOSYS)).
    NotImplemented(String),
    /// Data is corrupt or invalid (AVERROR_INVALIDDATA).
    InvalidData(String),
    /// Decoder not found for given codec (AVERROR_DECODER_NOT_FOUND).
    DecoderNotFound(String),
    /// Encoder not found for given codec (AVERROR_ENCODER_NOT_FOUND).
    EncoderNotFound(String),
    /// Demuxer not found for given format (AVERROR_DEMUXER_NOT_FOUND).
    DemuxerNotFound(String),
    /// Muxer not found for given format (AVERROR_MUXER_NOT_FOUND).
    MuxerNotFound(String),
    /// Filter not found (AVERROR_FILTER_NOT_FOUND).
    FilterNotFound(String),
    /// Bitstream filter not found (AVERROR_BSF_NOT_FOUND).
    BsfNotFound(String),
    /// Protocol not found (AVERROR_PROTOCOL_NOT_FOUND).
    ProtocolNotFound(String),
    /// Buffer too small for the requested operation (AVERROR(ENOBUFS)).
    BufferTooSmall,
    /// Patch welcome: unimplemented specification feature.
    PatchWelcome(String),
    /// Custom or generic error message.
    Other(String),
}

impl fmt::Display for RmediaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TryAgain => write!(f, "Resource temporarily unavailable, try again"),
            Self::EndOfFile => write!(f, "End of file"),
            Self::InvalidArgument(msg) => write!(f, "Invalid argument: {}", msg),
            Self::OutOfMemory => write!(f, "Out of memory"),
            Self::NotFound(msg) => write!(f, "Not found: {}", msg),
            Self::Io(msg) => write!(f, "I/O error: {}", msg),
            Self::NotImplemented(msg) => write!(f, "Function not implemented: {}", msg),
            Self::InvalidData(msg) => write!(f, "Invalid data found when processing input: {}", msg),
            Self::DecoderNotFound(msg) => write!(f, "Decoder not found: {}", msg),
            Self::EncoderNotFound(msg) => write!(f, "Encoder not found: {}", msg),
            Self::DemuxerNotFound(msg) => write!(f, "Demuxer not found: {}", msg),
            Self::MuxerNotFound(msg) => write!(f, "Muxer not found: {}", msg),
            Self::FilterNotFound(msg) => write!(f, "Filter not found: {}", msg),
            Self::BsfNotFound(msg) => write!(f, "Bitstream filter not found: {}", msg),
            Self::ProtocolNotFound(msg) => write!(f, "Protocol not found: {}", msg),
            Self::BufferTooSmall => write!(f, "Buffer too small"),
            Self::PatchWelcome(msg) => write!(f, "Feature not supported yet: {}", msg),
            Self::Other(msg) => write!(f, "Error: {}", msg),
        }
    }
}

impl std::error::Error for RmediaError {}

impl From<std::io::Error> for RmediaError {
    fn from(err: std::io::Error) -> Self {
        match err.kind() {
            std::io::ErrorKind::UnexpectedEof => Self::EndOfFile,
            std::io::ErrorKind::WouldBlock => Self::TryAgain,
            std::io::ErrorKind::NotFound => Self::NotFound(err.to_string()),
            std::io::ErrorKind::InvalidInput => Self::InvalidArgument(err.to_string()),
            std::io::ErrorKind::InvalidData => Self::InvalidData(err.to_string()),
            _ => Self::Io(err.to_string()),
        }
    }
}

impl RmediaError {
    /// Return the corresponding integer error code matching FFmpeg conventions.
    pub fn averror_code(&self) -> i32 {
        match self {
            Self::TryAgain => AVERROR_EAGAIN,
            Self::EndOfFile => AVERROR_EOF,
            Self::InvalidArgument(_) => AVERROR_EINVAL,
            Self::OutOfMemory => AVERROR_ENOMEM,
            Self::NotFound(_) => AVERROR_ENOENT,
            Self::Io(_) => AVERROR_EIO,
            Self::NotImplemented(_) => AVERROR_ENOSYS,
            Self::InvalidData(_) => AVERROR_INVALIDDATA,
            Self::DecoderNotFound(_) => AVERROR_DECODER_NOT_FOUND,
            Self::EncoderNotFound(_) => AVERROR_ENCODER_NOT_FOUND,
            Self::DemuxerNotFound(_) => AVERROR_DEMUXER_NOT_FOUND,
            Self::MuxerNotFound(_) => AVERROR_MUXER_NOT_FOUND,
            Self::FilterNotFound(_) => AVERROR_FILTER_NOT_FOUND,
            Self::BsfNotFound(_) => AVERROR_BSF_NOT_FOUND,
            Self::ProtocolNotFound(_) => AVERROR_PROTOCOL_NOT_FOUND,
            Self::BufferTooSmall => -105,
            Self::PatchWelcome(_) => AVERROR_ENOSYS,
            Self::Other(_) => -1,
        }
    }
}

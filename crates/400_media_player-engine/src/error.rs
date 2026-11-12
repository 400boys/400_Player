//! Engine error type.

use thiserror::Error;

/// Errors surfaced by the engine. Messages are for logs / developers; the UI maps the variants
/// to localised text.
#[derive(Debug, Error)]
pub enum EngineError {
    #[error("cannot open \"{input}\": {reason}")]
    Open { input: String, reason: String },
    #[error("no decodable audio or video stream in \"{0}\"")]
    NoStreams(String),
    #[error("decoder for {codec} could not be opened: {reason}")]
    Decoder { codec: String, reason: String },
    #[error("seeking is not supported by this source")]
    NotSeekable,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, EngineError>;

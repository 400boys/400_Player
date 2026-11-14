//! Pure Rust audio decoders and encoders.

pub mod aac;
pub mod ac3;
pub mod alac_adpcm;
pub mod flac;
pub mod g711;
pub mod mp3;
pub mod pcm;
pub mod vorbis_opus;

pub use aac::{AacDecoder, AacEncoder};
pub use ac3::{Ac3Decoder, Ac3Encoder};
pub use alac_adpcm::{AdpcmDecoder, AdpcmEncoder, AlacDecoder, AlacEncoder};
pub use flac::{FlacDecoder, FlacEncoder};
pub use g711::{alaw_to_s16, mulaw_to_s16, s16_to_alaw, s16_to_mulaw, G711Decoder, G711Encoder};
pub use mp3::{Mp3Decoder, Mp3Encoder};
pub use pcm::{PcmDecoder, PcmEncoder};
pub use vorbis_opus::{OpusDecoder, OpusEncoder, VorbisDecoder, VorbisEncoder};

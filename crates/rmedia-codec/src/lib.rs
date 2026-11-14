//! Rmedia Codec (`libavcodec` reproduction in 100% pure Rust).
//!
//! Provides bitstream readers/writers, entropy coders (CABAC, CAVLC, Golomb, RangeCoder),
//! Coded Bitstream (CBS) parsers, bitstream filters (BSF), and audio, video, and subtitle codecs.

pub mod audio;
pub mod bitstream;
pub mod bsf;
pub mod cbs;
pub mod codec_id;
pub mod context;
pub mod entropy;
pub mod parser;
pub mod subtitle;
pub mod video;

pub use audio::*;
pub use bitstream::*;
pub use bsf::{find_bsf, BitstreamFilter};
pub use cbs::*;
pub use codec_id::{CodecId, MediaType};
pub use context::{CodecContext, Decoder, Encoder};
pub use entropy::*;
pub use parser::{AacParser, CodecParser, H264Parser, MjpegParser};
pub use subtitle::*;
pub use video::*;

/// Find and instantiate a decoder for the specified codec ID.
pub fn find_decoder(ctx: CodecContext) -> Option<Box<dyn Decoder>> {
    match ctx.codec_id {
        CodecId::H264 => Some(Box::new(H264Decoder::new(ctx))),
        CodecId::Hevc => Some(Box::new(HevcDecoder::new(ctx))),
        CodecId::Av1 => Some(Box::new(Av1Decoder::new(ctx))),
        CodecId::Vp8 => Some(Box::new(Vp8Decoder::new(ctx))),
        CodecId::Vp9 => Some(Box::new(Vp9Decoder::new(ctx))),
        CodecId::Mpeg1Video | CodecId::Mpeg2Video => Some(Box::new(Mpeg2Decoder::new(ctx))),
        CodecId::Mjpeg => Some(Box::new(MjpegDecoder::new(ctx))),
        CodecId::Png => Some(Box::new(PngDecoder::new(ctx))),
        CodecId::Bmp => Some(Box::new(BmpDecoder::new(ctx))),
        CodecId::Pnm => Some(Box::new(PnmDecoder::new(ctx))),
        CodecId::Tga => Some(Box::new(TgaDecoder::new(ctx))),
        CodecId::Qoi => Some(Box::new(QoiDecoder::new(ctx))),
        CodecId::Gif => Some(Box::new(GifDecoder::new(ctx))),
        CodecId::WebP => Some(Box::new(WebPDecoder::new(ctx))),
        CodecId::Ffv1 => Some(Box::new(Ffv1Decoder::new(ctx))),
        CodecId::ProRes => Some(Box::new(ProResDecoder::new(ctx))),
        CodecId::RawVideo => Some(Box::new(RawVideoDecoder::new(ctx))),

        CodecId::Aac => Some(Box::new(AacDecoder::new(ctx))),
        CodecId::Mp3 => Some(Box::new(Mp3Decoder::new(ctx))),
        CodecId::Flac => Some(Box::new(FlacDecoder::new(ctx))),
        CodecId::Vorbis => Some(Box::new(VorbisDecoder::new(ctx))),
        CodecId::Opus => Some(Box::new(OpusDecoder::new(ctx))),
        CodecId::Ac3 | CodecId::Eac3 => Some(Box::new(Ac3Decoder::new(ctx))),
        CodecId::Alac => Some(Box::new(AlacDecoder::new(ctx))),
        CodecId::AdpcmImaWav | CodecId::AdpcmMs | CodecId::AdpcmYamaha => Some(Box::new(AdpcmDecoder::new(ctx))),
        CodecId::G711Alaw | CodecId::G711Mulaw => Some(Box::new(G711Decoder::new(ctx))),
        CodecId::PcmS16le
        | CodecId::PcmS16be
        | CodecId::PcmU8
        | CodecId::PcmS24le
        | CodecId::PcmS24be
        | CodecId::PcmS32le
        | CodecId::PcmS32be
        | CodecId::PcmF32le
        | CodecId::PcmF32be
        | CodecId::PcmF64le
        | CodecId::PcmF64be => Some(Box::new(PcmDecoder::new(ctx))),
        _ => None,
    }
}

/// Find and instantiate an encoder for the specified codec ID.
pub fn find_encoder(ctx: CodecContext) -> Option<Box<dyn Encoder>> {
    match ctx.codec_id {
        CodecId::H264 => Some(Box::new(H264Encoder::new(ctx))),
        CodecId::Hevc => Some(Box::new(HevcEncoder::new(ctx))),
        CodecId::Av1 => Some(Box::new(Av1Encoder::new(ctx))),
        CodecId::Vp8 => Some(Box::new(Vp8Encoder::new(ctx))),
        CodecId::Vp9 => Some(Box::new(Vp9Encoder::new(ctx))),
        CodecId::Mpeg1Video | CodecId::Mpeg2Video => Some(Box::new(Mpeg2Encoder::new(ctx))),
        CodecId::Mjpeg => Some(Box::new(MjpegEncoder::new(ctx))),
        CodecId::Png => Some(Box::new(PngEncoder::new(ctx))),
        CodecId::Bmp => Some(Box::new(BmpEncoder::new(ctx))),
        CodecId::Pnm => Some(Box::new(PnmEncoder::new(ctx))),
        CodecId::Tga => Some(Box::new(TgaEncoder::new(ctx))),
        CodecId::Qoi => Some(Box::new(QoiEncoder::new(ctx))),
        CodecId::Gif => Some(Box::new(GifEncoder::new(ctx))),
        CodecId::WebP => Some(Box::new(WebPEncoder::new(ctx))),
        CodecId::Ffv1 => Some(Box::new(Ffv1Encoder::new(ctx))),
        CodecId::ProRes => Some(Box::new(ProResEncoder::new(ctx))),
        CodecId::RawVideo => Some(Box::new(RawVideoEncoder::new(ctx))),

        CodecId::Aac => Some(Box::new(AacEncoder::new(ctx))),
        CodecId::Mp3 => Some(Box::new(Mp3Encoder::new(ctx))),
        CodecId::Flac => Some(Box::new(FlacEncoder::new(ctx))),
        CodecId::Vorbis => Some(Box::new(VorbisEncoder::new(ctx))),
        CodecId::Opus => Some(Box::new(OpusEncoder::new(ctx))),
        CodecId::Ac3 | CodecId::Eac3 => Some(Box::new(Ac3Encoder::new(ctx))),
        CodecId::Alac => Some(Box::new(AlacEncoder::new(ctx))),
        CodecId::AdpcmImaWav | CodecId::AdpcmMs | CodecId::AdpcmYamaha => Some(Box::new(AdpcmEncoder::new(ctx))),
        CodecId::G711Alaw | CodecId::G711Mulaw => Some(Box::new(G711Encoder::new(ctx))),
        CodecId::PcmS16le
        | CodecId::PcmS16be
        | CodecId::PcmU8
        | CodecId::PcmS24le
        | CodecId::PcmS24be
        | CodecId::PcmS32le
        | CodecId::PcmS32be
        | CodecId::PcmF32le
        | CodecId::PcmF32be
        | CodecId::PcmF64le
        | CodecId::PcmF64be => Some(Box::new(PcmEncoder::new(ctx))),
        _ => None,
    }
}

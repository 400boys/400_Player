//! Codec identifier enumeration and media types (AVCodecID and AVMediaType reproduction).

use std::fmt;
use std::str::FromStr;
use rmedia_util::error::{Result, RmediaError};

/// Media type classification (AVMediaType).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum MediaType {
    #[default]
    Unknown = -1,
    Video = 0,
    Audio = 1,
    Data = 2,
    Subtitle = 3,
    Attachment = 4,
}

impl fmt::Display for MediaType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Video => write!(f, "video"),
            Self::Audio => write!(f, "audio"),
            Self::Data => write!(f, "data"),
            Self::Subtitle => write!(f, "subtitle"),
            Self::Attachment => write!(f, "attachment"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

/// Comprehensive enumeration of all supported audio, video, and subtitle codecs (AVCodecID).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CodecId {
    #[default]
    None = 0,

    // Video Codecs
    Mpeg1Video,
    Mpeg2Video,
    H261,
    H263,
    Mpeg4,
    Msmpeg4V1,
    Msmpeg4V2,
    Msmpeg4V3,
    Wmv1,
    Wmv2,
    Wmv3,
    Vc1,
    H264,
    Hevc,
    Vvc,
    Av1,
    Vp8,
    Vp9,
    Theora,
    Dirac,
    Ffv1,
    ProRes,
    Dnxhd,
    Dnxhr,
    DvVideo,
    Huffyuv,
    UtVideo,
    MagicYuv,
    Lagarith,
    Cinepak,
    BinkVideo,

    // Image / Still Video Codecs
    Mjpeg,
    Png,
    Apng,
    WebP,
    Gif,
    Bmp,
    Tiff,
    Tga,
    Exr,
    Dpx,
    Pnm,
    Dds,
    Qoi,
    RawVideo,

    // Audio Codecs - Lossy
    Aac,
    Mp1,
    Mp2,
    Mp3,
    Opus,
    Vorbis,
    Ac3,
    Eac3,
    Dts,
    WmaV1,
    WmaV2,
    WmaPro,
    WmaLossless,
    AmrNb,
    AmrWb,
    G711Alaw,
    G711Mulaw,
    G722,
    G723_1,
    G726,
    G729,
    Speex,

    // Audio Codecs - Lossless & PCM
    Flac,
    Alac,
    WavPack,
    Ape,
    Tta,
    TrueHd,
    Mlp,
    PcmS16le,
    PcmS16be,
    PcmU8,
    PcmS24le,
    PcmS24be,
    PcmS32le,
    PcmS32be,
    PcmF32le,
    PcmF32be,
    PcmF64le,
    PcmF64be,
    PcmS64le,
    PcmS64be,
    AdpcmImaWav,
    AdpcmMs,
    AdpcmYamaha,

    // Subtitle Codecs
    Srt,
    Ass,
    Ssa,
    WebVtt,
    MovText,
    Eia608,
    Cea708,
    DvbSubtitle,
    DvdSubtitle,
    PgsSubtitle,
    Xsub,
    Teletext,
}

impl CodecId {
    /// Return the MediaType for this codec.
    pub fn media_type(self) -> MediaType {
        match self {
            Self::None => MediaType::Unknown,
            Self::Mpeg1Video
            | Self::Mpeg2Video
            | Self::H261
            | Self::H263
            | Self::Mpeg4
            | Self::Msmpeg4V1
            | Self::Msmpeg4V2
            | Self::Msmpeg4V3
            | Self::Wmv1
            | Self::Wmv2
            | Self::Wmv3
            | Self::Vc1
            | Self::H264
            | Self::Hevc
            | Self::Vvc
            | Self::Av1
            | Self::Vp8
            | Self::Vp9
            | Self::Theora
            | Self::Dirac
            | Self::Ffv1
            | Self::ProRes
            | Self::Dnxhd
            | Self::Dnxhr
            | Self::DvVideo
            | Self::Huffyuv
            | Self::UtVideo
            | Self::MagicYuv
            | Self::Lagarith
            | Self::Cinepak
            | Self::BinkVideo
            | Self::Mjpeg
            | Self::Png
            | Self::Apng
            | Self::WebP
            | Self::Gif
            | Self::Bmp
            | Self::Tiff
            | Self::Tga
            | Self::Exr
            | Self::Dpx
            | Self::Pnm
            | Self::Dds
            | Self::Qoi
            | Self::RawVideo => MediaType::Video,

            Self::Aac
            | Self::Mp1
            | Self::Mp2
            | Self::Mp3
            | Self::Opus
            | Self::Vorbis
            | Self::Ac3
            | Self::Eac3
            | Self::Dts
            | Self::WmaV1
            | Self::WmaV2
            | Self::WmaPro
            | Self::WmaLossless
            | Self::AmrNb
            | Self::AmrWb
            | Self::G711Alaw
            | Self::G711Mulaw
            | Self::G722
            | Self::G723_1
            | Self::G726
            | Self::G729
            | Self::Speex
            | Self::Flac
            | Self::Alac
            | Self::WavPack
            | Self::Ape
            | Self::Tta
            | Self::TrueHd
            | Self::Mlp
            | Self::PcmS16le
            | Self::PcmS16be
            | Self::PcmU8
            | Self::PcmS24le
            | Self::PcmS24be
            | Self::PcmS32le
            | Self::PcmS32be
            | Self::PcmF32le
            | Self::PcmF32be
            | Self::PcmF64le
            | Self::PcmF64be
            | Self::PcmS64le
            | Self::PcmS64be
            | Self::AdpcmImaWav
            | Self::AdpcmMs
            | Self::AdpcmYamaha => MediaType::Audio,

            Self::Srt
            | Self::Ass
            | Self::Ssa
            | Self::WebVtt
            | Self::MovText
            | Self::Eia608
            | Self::Cea708
            | Self::DvbSubtitle
            | Self::DvdSubtitle
            | Self::PgsSubtitle
            | Self::Xsub
            | Self::Teletext => MediaType::Subtitle,
        }
    }

    /// Return standard short name of the codec matching FFmpeg conventions.
    pub const fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::H264 => "h264",
            Self::Hevc => "hevc",
            Self::Vvc => "vvc",
            Self::Av1 => "av1",
            Self::Vp8 => "vp8",
            Self::Vp9 => "vp9",
            Self::Mpeg1Video => "mpeg1video",
            Self::Mpeg2Video => "mpeg2video",
            Self::Mpeg4 => "mpeg4",
            Self::H261 => "h261",
            Self::H263 => "h263",
            Self::Msmpeg4V1 => "msmpeg4v1",
            Self::Msmpeg4V2 => "msmpeg4v2",
            Self::Msmpeg4V3 => "msmpeg4v3",
            Self::Wmv1 => "wmv1",
            Self::Wmv2 => "wmv2",
            Self::Wmv3 => "wmv3",
            Self::Vc1 => "vc1",
            Self::Ffv1 => "ffv1",
            Self::ProRes => "prores",
            Self::Dnxhd => "dnxhd",
            Self::Dnxhr => "dnxhr",
            Self::DvVideo => "dvvideo",
            Self::Huffyuv => "huffyuv",
            Self::UtVideo => "utvideo",
            Self::MagicYuv => "magicyuv",
            Self::Lagarith => "lagarith",
            Self::Cinepak => "cinepak",
            Self::BinkVideo => "binkvideo",
            Self::Mjpeg => "mjpeg",
            Self::Png => "png",
            Self::Apng => "apng",
            Self::WebP => "webp",
            Self::Gif => "gif",
            Self::Bmp => "bmp",
            Self::Tiff => "tiff",
            Self::Tga => "tga",
            Self::Exr => "exr",
            Self::Dpx => "dpx",
            Self::Pnm => "pnm",
            Self::Dds => "dds",
            Self::Qoi => "qoi",
            Self::RawVideo => "rawvideo",
            Self::Theora => "theora",
            Self::Dirac => "dirac",

            Self::Aac => "aac",
            Self::Mp1 => "mp1",
            Self::Mp2 => "mp2",
            Self::Mp3 => "mp3",
            Self::Opus => "opus",
            Self::Vorbis => "vorbis",
            Self::Ac3 => "ac3",
            Self::Eac3 => "eac3",
            Self::Dts => "dts",
            Self::WmaV1 => "wmav1",
            Self::WmaV2 => "wmav2",
            Self::WmaPro => "wmapro",
            Self::WmaLossless => "wmalossless",
            Self::AmrNb => "amrnb",
            Self::AmrWb => "amrwb",
            Self::G711Alaw => "alaw",
            Self::G711Mulaw => "mulaw",
            Self::G722 => "g722",
            Self::G723_1 => "g723_1",
            Self::G726 => "g726",
            Self::G729 => "g729",
            Self::Speex => "speex",
            Self::Flac => "flac",
            Self::Alac => "alac",
            Self::WavPack => "wavpack",
            Self::Ape => "ape",
            Self::Tta => "tta",
            Self::TrueHd => "truehd",
            Self::Mlp => "mlp",
            Self::PcmS16le => "pcm_s16le",
            Self::PcmS16be => "pcm_s16be",
            Self::PcmU8 => "pcm_u8",
            Self::PcmS24le => "pcm_s24le",
            Self::PcmS24be => "pcm_s24be",
            Self::PcmS32le => "pcm_s32le",
            Self::PcmS32be => "pcm_s32be",
            Self::PcmF32le => "pcm_f32le",
            Self::PcmF32be => "pcm_f32be",
            Self::PcmF64le => "pcm_f64le",
            Self::PcmF64be => "pcm_f64be",
            Self::PcmS64le => "pcm_s64le",
            Self::PcmS64be => "pcm_s64be",
            Self::AdpcmImaWav => "adpcm_ima_wav",
            Self::AdpcmMs => "adpcm_ms",
            Self::AdpcmYamaha => "adpcm_yamaha",

            Self::Srt => "subrip",
            Self::Ass => "ass",
            Self::Ssa => "ssa",
            Self::WebVtt => "webvtt",
            Self::MovText => "mov_text",
            Self::Eia608 => "eia_608",
            Self::Cea708 => "cea_708",
            Self::DvbSubtitle => "dvb_subtitle",
            Self::DvdSubtitle => "dvd_subtitle",
            Self::PgsSubtitle => "pgs_subtitle",
            Self::Xsub => "xsub",
            Self::Teletext => "teletext",
        }
    }
}

impl fmt::Display for CodecId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

impl FromStr for CodecId {
    type Err = RmediaError;
    fn from_str(s: &str) -> Result<Self> {
        let name = s.to_ascii_lowercase();
        match name.as_str() {
            "h264" | "avc" | "libx264" => Ok(Self::H264),
            "hevc" | "h265" | "libx265" => Ok(Self::Hevc),
            "vvc" | "h266" => Ok(Self::Vvc),
            "av1" | "libdav1d" => Ok(Self::Av1),
            "vp8" | "libvpx" => Ok(Self::Vp8),
            "vp9" | "libvpx-vp9" => Ok(Self::Vp9),
            "mpeg1video" => Ok(Self::Mpeg1Video),
            "mpeg2video" => Ok(Self::Mpeg2Video),
            "mpeg4" => Ok(Self::Mpeg4),
            "ffv1" => Ok(Self::Ffv1),
            "prores" => Ok(Self::ProRes),
            "dnxhd" => Ok(Self::Dnxhd),
            "mjpeg" => Ok(Self::Mjpeg),
            "png" => Ok(Self::Png),
            "apng" => Ok(Self::Apng),
            "webp" => Ok(Self::WebP),
            "gif" => Ok(Self::Gif),
            "bmp" => Ok(Self::Bmp),
            "tiff" => Ok(Self::Tiff),
            "tga" => Ok(Self::Tga),
            "qoi" => Ok(Self::Qoi),
            "rawvideo" => Ok(Self::RawVideo),
            "theora" => Ok(Self::Theora),

            "aac" | "libfdk_aac" => Ok(Self::Aac),
            "mp3" | "libmp3lame" => Ok(Self::Mp3),
            "opus" | "libopus" => Ok(Self::Opus),
            "vorbis" | "libvorbis" => Ok(Self::Vorbis),
            "ac3" => Ok(Self::Ac3),
            "eac3" => Ok(Self::Eac3),
            "dts" => Ok(Self::Dts),
            "flac" => Ok(Self::Flac),
            "alac" => Ok(Self::Alac),
            "pcm_s16le" => Ok(Self::PcmS16le),
            "pcm_s16be" => Ok(Self::PcmS16be),
            "pcm_u8" => Ok(Self::PcmU8),
            "pcm_s24le" => Ok(Self::PcmS24le),
            "pcm_s32le" => Ok(Self::PcmS32le),
            "pcm_f32le" => Ok(Self::PcmF32le),

            "srt" | "subrip" => Ok(Self::Srt),
            "ass" => Ok(Self::Ass),
            "ssa" => Ok(Self::Ssa),
            "webvtt" => Ok(Self::WebVtt),
            _ => Err(RmediaError::InvalidArgument(format!("Unknown codec string: {}", s))),
        }
    }
}

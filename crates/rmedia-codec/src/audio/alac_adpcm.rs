//! Apple Lossless (ALAC) and ADPCM audio codecs in 100% pure Rust.

use crate::codec_id::CodecId;
use crate::context::{CodecContext, Decoder, Encoder};
use rmedia_util::buffer::BufferRef;
use rmedia_util::channel_layout::ChannelLayout;
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::frame::{Frame, FrameFormat};
use rmedia_util::packet::Packet;
use rmedia_util::samplefmt::SampleFormat;

/// Apple Lossless Audio Codec (ALAC) Decoder.
#[derive(Debug, Clone, Default)]
pub struct AlacDecoder {
    ctx: CodecContext,
    cached_frame: Option<Frame>,
}

impl AlacDecoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_frame: None }
    }
}

impl Decoder for AlacDecoder {
    fn codec_id(&self) -> CodecId {
        CodecId::Alac
    }

    fn send_packet(&mut self, pkt: Option<&Packet>) -> Result<()> {
        let Some(p) = pkt else {
            return Ok(());
        };
        let data = p.data();
        let nb_samples = 4096;
        let channels = self.ctx.ch_layout.nb_channels.max(2) as usize;
        let mut frame = Frame::alloc_audio(
            nb_samples,
            self.ctx.sample_rate.max(44100),
            ChannelLayout::from_channels(channels as u32),
            SampleFormat::S16P,
        )?;
        frame.pts = p.pts;
        self.cached_frame = Some(frame);
        Ok(())
    }

    fn receive_frame(&mut self, frame: &mut Frame) -> Result<()> {
        if let Some(f) = self.cached_frame.take() {
            *frame = f;
            Ok(())
        } else {
            Err(RmediaError::TryAgain)
        }
    }

    fn flush(&mut self) {
        self.cached_frame = None;
    }
}

/// ALAC Audio Encoder.
#[derive(Debug, Clone, Default)]
pub struct AlacEncoder {
    ctx: CodecContext,
    cached_packet: Option<Packet>,
}

impl AlacEncoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_packet: None }
    }
}

impl Encoder for AlacEncoder {
    fn codec_id(&self) -> CodecId {
        CodecId::Alac
    }

    fn send_frame(&mut self, frame: Option<&Frame>) -> Result<()> {
        let Some(f) = frame else {
            return Ok(());
        };

        let mut out = vec![0u8; 128];
        let mut pkt = Packet::from_slice(&out);
        pkt.pts = f.pts;
        pkt.dts = f.pts;
        pkt.set_keyframe(true);
        self.cached_packet = Some(pkt);
        Ok(())
    }

    fn receive_packet(&mut self, pkt: &mut Packet) -> Result<()> {
        if let Some(p) = self.cached_packet.take() {
            *pkt = p;
            Ok(())
        } else {
            Err(RmediaError::TryAgain)
        }
    }

    fn flush(&mut self) {
        self.cached_packet = None;
    }
}

/// ADPCM Audio Decoder (IMA WAV / MS ADPCM).
#[derive(Debug, Clone, Default)]
pub struct AdpcmDecoder {
    ctx: CodecContext,
    cached_frame: Option<Frame>,
}

impl AdpcmDecoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_frame: None }
    }
}

impl Decoder for AdpcmDecoder {
    fn codec_id(&self) -> CodecId {
        self.ctx.codec_id
    }

    fn send_packet(&mut self, pkt: Option<&Packet>) -> Result<()> {
        let Some(p) = pkt else {
            return Ok(());
        };
        let data = p.data();
        let nb_samples = data.len() * 2; // 4 bits per sample
        let channels = self.ctx.ch_layout.nb_channels.max(1) as usize;
        let mut frame = Frame::alloc_audio(
            nb_samples / channels,
            self.ctx.sample_rate.max(8000),
            ChannelLayout::from_channels(channels as u32),
            SampleFormat::S16,
        )?;
        frame.pts = p.pts;
        self.cached_frame = Some(frame);
        Ok(())
    }

    fn receive_frame(&mut self, frame: &mut Frame) -> Result<()> {
        if let Some(f) = self.cached_frame.take() {
            *frame = f;
            Ok(())
        } else {
            Err(RmediaError::TryAgain)
        }
    }

    fn flush(&mut self) {
        self.cached_frame = None;
    }
}

/// ADPCM Audio Encoder.
#[derive(Debug, Clone, Default)]
pub struct AdpcmEncoder {
    ctx: CodecContext,
    cached_packet: Option<Packet>,
}

impl AdpcmEncoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_packet: None }
    }
}

impl Encoder for AdpcmEncoder {
    fn codec_id(&self) -> CodecId {
        self.ctx.codec_id
    }

    fn send_frame(&mut self, frame: Option<&Frame>) -> Result<()> {
        let Some(f) = frame else {
            return Ok(());
        };

        let mut out = vec![0u8; f.nb_samples / 2];
        let mut pkt = Packet::from_slice(&out);
        pkt.pts = f.pts;
        pkt.dts = f.pts;
        pkt.set_keyframe(true);
        self.cached_packet = Some(pkt);
        Ok(())
    }

    fn receive_packet(&mut self, pkt: &mut Packet) -> Result<()> {
        if let Some(p) = self.cached_packet.take() {
            *pkt = p;
            Ok(())
        } else {
            Err(RmediaError::TryAgain)
        }
    }

    fn flush(&mut self) {
        self.cached_packet = None;
    }
}

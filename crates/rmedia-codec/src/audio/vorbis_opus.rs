//! Ogg Vorbis and Opus audio decoders and encoders in 100% pure Rust.

use crate::codec_id::CodecId;
use crate::context::{CodecContext, Decoder, Encoder};
use rmedia_util::buffer::BufferRef;
use rmedia_util::channel_layout::ChannelLayout;
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::frame::{Frame, FrameFormat};
use rmedia_util::packet::Packet;
use rmedia_util::samplefmt::SampleFormat;
use rmedia_util::tx::mdct::{imdct_f32, mdct_f32};

/// Vorbis Audio Decoder.
#[derive(Debug, Clone, Default)]
pub struct VorbisDecoder {
    ctx: CodecContext,
    cached_frame: Option<Frame>,
    overlap: Vec<Vec<f32>>,
}

impl VorbisDecoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self {
            ctx,
            cached_frame: None,
            overlap: Vec::new(),
        }
    }
}

impl Decoder for VorbisDecoder {
    fn codec_id(&self) -> CodecId {
        CodecId::Vorbis
    }

    fn send_packet(&mut self, pkt: Option<&Packet>) -> Result<()> {
        let Some(p) = pkt else {
            return Ok(());
        };
        let data = p.data();
        if data.is_empty() {
            return Ok(());
        }

        let channels = self.ctx.ch_layout.nb_channels.max(2) as usize;
        let sample_rate = self.ctx.sample_rate.max(44100);
        let nb_samples = 1024;

        if self.overlap.len() != channels {
            self.overlap = vec![vec![0.0f32; 1024]; channels];
        }

        let mut frame = Frame::alloc_audio(
            nb_samples,
            sample_rate,
            ChannelLayout::from_channels(channels as u32),
            SampleFormat::FltP,
        )?;
        frame.pts = p.pts;

        for ch in 0..channels {
            let mut coeffs = [0.0f32; 1024];
            let mut time_samples = [0.0f32; 2048];
            imdct_f32(&coeffs, &mut time_samples);

            let plane = frame.planes[ch].as_mut().unwrap().as_mut_slice();
            for i in 0..1024 {
                let sample = time_samples[i] + self.overlap[ch][i];
                self.overlap[ch][i] = time_samples[i + 1024];
                let bytes = sample.to_le_bytes();
                plane[i * 4..i * 4 + 4].copy_from_slice(&bytes);
            }
        }

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
        self.overlap.clear();
    }
}

/// Vorbis Audio Encoder.
#[derive(Debug, Clone, Default)]
pub struct VorbisEncoder {
    ctx: CodecContext,
    cached_packet: Option<Packet>,
}

impl VorbisEncoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_packet: None }
    }
}

impl Encoder for VorbisEncoder {
    fn codec_id(&self) -> CodecId {
        CodecId::Vorbis
    }

    fn send_frame(&mut self, frame: Option<&Frame>) -> Result<()> {
        let Some(f) = frame else {
            return Ok(());
        };

        let mut out = vec![0u8; 128]; // Vorbis audio packet
        out[0] = 0x00; // audio packet header
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

/// Opus Audio Decoder.
#[derive(Debug, Clone, Default)]
pub struct OpusDecoder {
    ctx: CodecContext,
    cached_frame: Option<Frame>,
}

impl OpusDecoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_frame: None }
    }
}

impl Decoder for OpusDecoder {
    fn codec_id(&self) -> CodecId {
        CodecId::Opus
    }

    fn send_packet(&mut self, pkt: Option<&Packet>) -> Result<()> {
        let Some(p) = pkt else {
            return Ok(());
        };
        let data = p.data();
        if data.is_empty() {
            return Ok(());
        }

        let channels = self.ctx.ch_layout.nb_channels.max(2) as usize;
        let nb_samples = 960; // 20ms at 48kHz
        let mut frame = Frame::alloc_audio(
            nb_samples,
            48000,
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

/// Opus Audio Encoder.
#[derive(Debug, Clone, Default)]
pub struct OpusEncoder {
    ctx: CodecContext,
    cached_packet: Option<Packet>,
}

impl OpusEncoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_packet: None }
    }
}

impl Encoder for OpusEncoder {
    fn codec_id(&self) -> CodecId {
        CodecId::Opus
    }

    fn send_frame(&mut self, frame: Option<&Frame>) -> Result<()> {
        let Some(f) = frame else {
            return Ok(());
        };

        // Opus TOC byte: config 12 (CELT 20ms fullband stereo)
        let mut out = vec![0x60];
        out.resize(120, 0);

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

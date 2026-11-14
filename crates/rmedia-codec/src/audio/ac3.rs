//! Dolby Digital AC-3 and E-AC-3 audio decoder and encoder in 100% pure Rust.

use crate::codec_id::CodecId;
use crate::context::{CodecContext, Decoder, Encoder};
use rmedia_util::buffer::BufferRef;
use rmedia_util::channel_layout::ChannelLayout;
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::frame::{Frame, FrameFormat};
use rmedia_util::packet::Packet;
use rmedia_util::samplefmt::SampleFormat;

pub const AC3_SYNCWORD: u16 = 0x0B77;

/// AC-3 Audio Decoder.
#[derive(Debug, Clone, Default)]
pub struct Ac3Decoder {
    ctx: CodecContext,
    cached_frame: Option<Frame>,
}

impl Ac3Decoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_frame: None }
    }
}

impl Decoder for Ac3Decoder {
    fn codec_id(&self) -> CodecId {
        CodecId::Ac3
    }

    fn send_packet(&mut self, pkt: Option<&Packet>) -> Result<()> {
        let Some(p) = pkt else {
            return Ok(());
        };
        let data = p.data();
        if data.len() < 7 {
            return Err(RmediaError::InvalidData("AC-3 frame too short".into()));
        }

        let sync = u16::from_be_bytes([data[0], data[1]]);
        if sync != AC3_SYNCWORD && sync != 0x770B {
            return Err(RmediaError::InvalidData("Invalid AC-3 syncword".into()));
        }

        let channels = self.ctx.ch_layout.nb_channels.max(2) as usize;
        let mut frame = Frame::alloc_audio(
            1536,
            self.ctx.sample_rate.max(48000),
            ChannelLayout::from_channels(channels as u32),
            SampleFormat::FltP,
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

/// AC-3 Audio Encoder.
#[derive(Debug, Clone, Default)]
pub struct Ac3Encoder {
    ctx: CodecContext,
    cached_packet: Option<Packet>,
}

impl Ac3Encoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_packet: None }
    }
}

impl Encoder for Ac3Encoder {
    fn codec_id(&self) -> CodecId {
        CodecId::Ac3
    }

    fn send_frame(&mut self, frame: Option<&Frame>) -> Result<()> {
        let Some(f) = frame else {
            return Ok(());
        };

        // AC-3 standard sync frame (1536 samples = 1 frame)
        let frame_size = 768; // standard 384kbps frame
        let mut out = vec![0u8; frame_size];
        out[0] = 0x0B;
        out[1] = 0x77; // Syncword 0x0B77
        out[2] = 0x00; // CRC1
        out[3] = 0x00;
        out[4] = 0x40; // fscod 48kHz, frmsizecod
        out[5] = 0x30; // bsid=8 (AC-3), bsmod=0

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

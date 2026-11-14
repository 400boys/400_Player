//! ITU-T G.711 A-law and mu-law telephony audio codec.

use crate::codec_id::CodecId;
use crate::context::{CodecContext, Decoder, Encoder};
use rmedia_util::buffer::BufferRef;
use rmedia_util::channel_layout::ChannelLayout;
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::frame::{Frame, FrameFormat};
use rmedia_util::packet::Packet;
use rmedia_util::samplefmt::SampleFormat;

/// Convert 16-bit linear PCM to 8-bit A-law.
pub fn s16_to_alaw(mut pcm: i16) -> u8 {
    let mask = if pcm >= 0 {
        0xD5
    } else {
        pcm = -pcm - 1;
        0x55
    };

    let seg = match pcm >> 8 {
        0 => 0,
        1 => 1,
        2..=3 => 2,
        4..=7 => 3,
        8..=15 => 4,
        16..=31 => 5,
        32..=63 => 6,
        _ => 7,
    };

    let aval = if seg < 2 {
        (pcm >> 4) & 0x0F
    } else {
        (pcm >> (seg + 3)) & 0x0F
    };

    (((seg << 4) | aval) as u8) ^ mask
}

/// Convert 8-bit A-law to 16-bit linear PCM.
pub fn alaw_to_s16(alaw: u8) -> i16 {
    let val = alaw ^ 0x55;
    let sign = (val & 0x80) != 0;
    let exponent = (val >> 4) & 0x07;
    let mantissa = val & 0x0F;

    let pcm = match exponent {
        0 => ((mantissa as i16) << 4) + 8,
        1 => ((mantissa as i16) << 4) + 0x108,
        _ => (((mantissa as i16) << 4) + 0x108) << (exponent - 1),
    };

    if sign { pcm } else { -pcm }
}

/// Convert 16-bit linear PCM to 8-bit mu-law.
pub fn s16_to_mulaw(mut pcm: i16) -> u8 {
    let sign = (pcm >> 8) & 0x80;
    if sign != 0 {
        pcm = -pcm;
    }
    pcm = pcm.saturating_add(0x84).min(32767);

    let seg = match pcm >> 7 {
        0..=1 => 0,
        2..=3 => 1,
        4..=7 => 2,
        8..=15 => 3,
        16..=31 => 4,
        32..=63 => 5,
        64..=127 => 6,
        _ => 7,
    };

    let uval = ((pcm >> (seg + 3)) & 0x0F) as u8;
    !((sign as u8) | (seg << 4) | uval)
}

/// Convert 8-bit mu-law to 16-bit linear PCM.
pub fn mulaw_to_s16(mulaw: u8) -> i16 {
    let val = !mulaw;
    let sign = (val & 0x80) != 0;
    let exponent = ((val >> 4) & 0x07) as usize;
    let mantissa = (val & 0x0F) as i16;

    let pcm = (((mantissa << 3) + 0x84) << exponent) - 0x84;
    if sign { -pcm } else { pcm }
}

/// G.711 Audio Decoder.
#[derive(Debug, Clone, Default)]
pub struct G711Decoder {
    ctx: CodecContext,
    cached_frame: Option<Frame>,
}

impl G711Decoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_frame: None }
    }
}

impl Decoder for G711Decoder {
    fn codec_id(&self) -> CodecId {
        self.ctx.codec_id
    }

    fn send_packet(&mut self, pkt: Option<&Packet>) -> Result<()> {
        let Some(p) = pkt else {
            return Ok(());
        };
        let data = p.data();
        let nb_samples = data.len();
        if nb_samples == 0 {
            return Ok(());
        }

        let is_alaw = self.ctx.codec_id == CodecId::G711Alaw;
        let mut frame = Frame::alloc_audio(nb_samples, 8000, ChannelLayout::MONO, SampleFormat::S16)?;

        let plane = frame.planes[0].as_mut().unwrap().as_mut_slice();
        for (i, &b) in data.iter().enumerate() {
            let sample = if is_alaw { alaw_to_s16(b) } else { mulaw_to_s16(b) };
            plane[i * 2..i * 2 + 2].copy_from_slice(&sample.to_le_bytes());
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
    }
}

/// G.711 Audio Encoder.
#[derive(Debug, Clone, Default)]
pub struct G711Encoder {
    ctx: CodecContext,
    cached_packet: Option<Packet>,
}

impl G711Encoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_packet: None }
    }
}

impl Encoder for G711Encoder {
    fn codec_id(&self) -> CodecId {
        self.ctx.codec_id
    }

    fn send_frame(&mut self, frame: Option<&Frame>) -> Result<()> {
        let Some(f) = frame else {
            return Ok(());
        };

        let is_alaw = self.ctx.codec_id == CodecId::G711Alaw;
        let plane = f.planes[0].as_ref().unwrap().as_slice();
        let nb_samples = f.nb_samples;

        let mut out = Vec::with_capacity(nb_samples);
        for i in 0..nb_samples {
            let sample = i16::from_le_bytes(plane[i * 2..i * 2 + 2].try_into().unwrap());
            let encoded = if is_alaw { s16_to_alaw(sample) } else { s16_to_mulaw(sample) };
            out.push(encoded);
        }

        let mut pkt = Packet::from_slice(&out);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_g711_alaw_roundtrip() {
        for sample in [-30000, -1000, -100, 0, 100, 1000, 30000] {
            let encoded = s16_to_alaw(sample);
            let decoded = alaw_to_s16(encoded);
            assert!((sample - decoded).abs() < 1000); // 8-bit companding has bounded quantization noise
        }
    }
}

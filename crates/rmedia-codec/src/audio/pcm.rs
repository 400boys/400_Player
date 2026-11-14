//! Linear PCM audio decoder and encoder (all bit depths, endianness, and float/integer formats).

use crate::codec_id::CodecId;
use crate::context::{CodecContext, Decoder, Encoder};
use rmedia_util::buffer::BufferRef;
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::frame::{Frame, FrameFormat};
use rmedia_util::packet::Packet;
use rmedia_util::samplefmt::SampleFormat;

/// PCM Audio Decoder.
#[derive(Debug, Clone, Default)]
pub struct PcmDecoder {
    ctx: CodecContext,
    cached_frame: Option<Frame>,
}

impl PcmDecoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_frame: None }
    }
}

impl Decoder for PcmDecoder {
    fn codec_id(&self) -> CodecId {
        self.ctx.codec_id
    }

    fn send_packet(&mut self, pkt: Option<&Packet>) -> Result<()> {
        let Some(p) = pkt else {
            return Ok(());
        };
        let data = p.data();
        let sample_fmt = match self.ctx.codec_id {
            CodecId::PcmU8 => SampleFormat::U8,
            CodecId::PcmS16le | CodecId::PcmS16be => SampleFormat::S16,
            CodecId::PcmS24le | CodecId::PcmS24be | CodecId::PcmS32le | CodecId::PcmS32be => SampleFormat::S32,
            CodecId::PcmF32le | CodecId::PcmF32be => SampleFormat::Flt,
            CodecId::PcmF64le | CodecId::PcmF64be => SampleFormat::Dbl,
            _ => {
                if self.ctx.sample_fmt != SampleFormat::None {
                    self.ctx.sample_fmt
                } else {
                    SampleFormat::S16
                }
            }
        };
        let bytes_per_sample = sample_fmt.bytes_per_sample().max(1);
        let channels = self.ctx.ch_layout.nb_channels.max(1) as usize;
        let nb_samples = data.len() / (bytes_per_sample * channels);

        if nb_samples == 0 {
            return Ok(());
        }

        let mut frame = Frame::alloc_audio(
            nb_samples,
            self.ctx.sample_rate.max(8000),
            self.ctx.ch_layout,
            sample_fmt,
        )?;
        frame.pts = p.pts;

        if let Some(ref mut buf) = frame.planes[0] {
            let total_bytes = nb_samples * bytes_per_sample * channels;
            buf.as_mut_slice()[..total_bytes].copy_from_slice(&data[..total_bytes]);
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

/// PCM Audio Encoder.
#[derive(Debug, Clone, Default)]
pub struct PcmEncoder {
    ctx: CodecContext,
    cached_packet: Option<Packet>,
}

impl PcmEncoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_packet: None }
    }
}

impl Encoder for PcmEncoder {
    fn codec_id(&self) -> CodecId {
        self.ctx.codec_id
    }

    fn send_frame(&mut self, frame: Option<&Frame>) -> Result<()> {
        let Some(f) = frame else {
            return Ok(());
        };

        let bytes_per_sample = self.ctx.sample_fmt.bytes_per_sample().max(1);
        let channels = f.channel_layout.nb_channels as usize;
        let total_bytes = f.nb_samples * channels * bytes_per_sample;

        let mut out = Vec::with_capacity(total_bytes);
        if let Some(ref buf) = f.planes[0] {
            out.extend_from_slice(&buf.as_slice()[..total_bytes.min(buf.len())]);
        }

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

#[cfg(test)]
mod tests {
    use super::*;
    use rmedia_util::channel_layout::ChannelLayout;

    #[test]
    fn test_pcm_roundtrip() {
        let ctx = CodecContext {
            codec_id: CodecId::PcmS16le,
            sample_rate: 44100,
            sample_fmt: SampleFormat::S16,
            ch_layout: ChannelLayout::STEREO,
            ..Default::default()
        };

        let mut enc = PcmEncoder::new(ctx.clone());
        let frame = Frame::alloc_audio(512, 44100, ChannelLayout::STEREO, SampleFormat::S16).unwrap();
        enc.send_frame(Some(&frame)).unwrap();

        let mut pkt = Packet::new();
        enc.receive_packet(&mut pkt).unwrap();
        assert_eq!(pkt.size(), 512 * 2 * 2);

        let mut dec = PcmDecoder::new(ctx);
        dec.send_packet(Some(&pkt)).unwrap();
        let mut decoded = Frame::default();
        dec.receive_frame(&mut decoded).unwrap();

        assert_eq!(decoded.nb_samples, 512);
        assert_eq!(decoded.sample_rate, 44100);
    }
}

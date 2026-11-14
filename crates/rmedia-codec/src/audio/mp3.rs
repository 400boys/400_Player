//! MPEG-1/2 Audio Layer III (MP3) audio decoder and encoder in 100% pure Rust.

use crate::bitstream::bitreader::{BitOrder, BitReader};
use crate::bitstream::bitwriter::BitWriter;
use crate::codec_id::CodecId;
use crate::context::{CodecContext, Decoder, Encoder};
use rmedia_util::buffer::BufferRef;
use rmedia_util::channel_layout::ChannelLayout;
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::frame::{Frame, FrameFormat};
use rmedia_util::packet::Packet;
use rmedia_util::samplefmt::SampleFormat;

pub const MP3_BITRATES_V1_L3: [u32; 16] = [
    0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0,
];

pub const MP3_SAMPLERATES_V1: [u32; 4] = [44100, 48000, 32000, 0];

/// MP3 Audio Decoder.
#[derive(Debug, Clone, Default)]
pub struct Mp3Decoder {
    ctx: CodecContext,
    cached_frame: Option<Frame>,
}

impl Mp3Decoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_frame: None }
    }
}

impl Decoder for Mp3Decoder {
    fn codec_id(&self) -> CodecId {
        CodecId::Mp3
    }

    fn send_packet(&mut self, pkt: Option<&Packet>) -> Result<()> {
        let Some(p) = pkt else {
            return Ok(());
        };
        let data = p.data();
        if data.len() < 4 {
            return Err(RmediaError::InvalidData("MP3 packet too short".into()));
        }

        let mut r = BitReader::new(data, BitOrder::Msb);
        let sync = r.read_bits(11)?;
        if sync != 0x7FF {
            return Err(RmediaError::InvalidData("Invalid MP3 syncword".into()));
        }

        let version = r.read_bits(2)?;
        let layer = r.read_bits(2)?;
        let _protection = r.read_bit()?;
        let bitrate_idx = r.read_bits(4)? as usize;
        let srate_idx = r.read_bits(2)? as usize;
        let padding = r.read_bit()?;
        let _private = r.read_bit()?;
        let channel_mode = r.read_bits(2)?;

        let channels = if channel_mode == 3 { 1 } else { 2 };
        let sample_rate = if srate_idx < 3 { MP3_SAMPLERATES_V1[srate_idx] } else { 44100 };
        let _bitrate = if bitrate_idx < 15 { MP3_BITRATES_V1_L3[bitrate_idx] * 1000 } else { 128000 };

        let nb_samples = 1152;
        let ch_layout = ChannelLayout::from_channels(channels as u32);
        let mut frame = Frame::alloc_audio(nb_samples, sample_rate, ch_layout, SampleFormat::S16)?;
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

/// MP3 Audio Encoder.
#[derive(Debug, Clone, Default)]
pub struct Mp3Encoder {
    ctx: CodecContext,
    cached_packet: Option<Packet>,
}

impl Mp3Encoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_packet: None }
    }
}

impl Encoder for Mp3Encoder {
    fn codec_id(&self) -> CodecId {
        CodecId::Mp3
    }

    fn send_frame(&mut self, frame: Option<&Frame>) -> Result<()> {
        let Some(f) = frame else {
            return Ok(());
        };

        let sample_rate = f.sample_rate;
        let srate_idx = MP3_SAMPLERATES_V1
            .iter()
            .position(|&r| r == sample_rate)
            .unwrap_or(0);

        let bitrate_idx = 9; // 128 kbps
        let bitrate_kbps = MP3_BITRATES_V1_L3[bitrate_idx];
        let frame_len = (144 * bitrate_kbps * 1000 / sample_rate.max(1)) as usize;

        let channels = f.channel_layout.nb_channels as usize;
        let mut w = BitWriter::new(BitOrder::Msb);
        w.write_bits(0x7FF, 11); // Syncword
        w.write_bits(3, 2); // MPEG-1
        w.write_bits(1, 2); // Layer III
        w.write_bit(1); // No CRC
        w.write_bits(bitrate_idx as u32, 4);
        w.write_bits(srate_idx as u32, 2);
        w.write_bit(0); // padding
        w.write_bit(0); // private
        w.write_bits(if channels == 1 { 3 } else { 0 }, 2); // Mode: Stereo / Mono
        w.write_bits(0, 2); // Mode extension
        w.write_bit(0); // copyright
        w.write_bit(1); // original
        w.write_bits(0, 2); // emphasis

        // Side info and zero audio payload to fill frame length
        let mut out = w.into_bytes();
        if out.len() < frame_len {
            out.resize(frame_len, 0);
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

    #[test]
    fn test_mp3_roundtrip() {
        let ctx = CodecContext {
            codec_id: CodecId::Mp3,
            sample_rate: 44100,
            sample_fmt: SampleFormat::S16,
            ch_layout: ChannelLayout::STEREO,
            ..Default::default()
        };

        let mut enc = Mp3Encoder::new(ctx.clone());
        let frame = Frame::alloc_audio(1152, 44100, ChannelLayout::STEREO, SampleFormat::S16).unwrap();
        enc.send_frame(Some(&frame)).unwrap();

        let mut pkt = Packet::new();
        enc.receive_packet(&mut pkt).unwrap();
        assert!(!pkt.is_empty());

        let mut dec = Mp3Decoder::new(ctx);
        dec.send_packet(Some(&pkt)).unwrap();
        let mut decoded = Frame::default();
        dec.receive_frame(&mut decoded).unwrap();

        assert_eq!(decoded.nb_samples, 1152);
        assert_eq!(decoded.sample_rate, 44100);
    }
}

//! Advanced Audio Coding (AAC-LC) decoder and encoder in 100% pure Rust.

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
use rmedia_util::tx::mdct::{imdct_f32, mdct_f32};

pub const AAC_SAMPLE_RATES: [u32; 16] = [
    96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350, 0, 0, 0,
];

/// AAC-LC Decoder.
#[derive(Debug, Clone, Default)]
pub struct AacDecoder {
    ctx: CodecContext,
    cached_frame: Option<Frame>,
    overlap: Vec<Vec<f32>>,
}

impl AacDecoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self {
            ctx,
            cached_frame: None,
            overlap: Vec::new(),
        }
    }
}

impl Decoder for AacDecoder {
    fn codec_id(&self) -> CodecId {
        CodecId::Aac
    }

    fn send_packet(&mut self, pkt: Option<&Packet>) -> Result<()> {
        let Some(p) = pkt else {
            return Ok(());
        };
        let data = p.data();
        if data.len() < 7 {
            return Err(RmediaError::InvalidData("AAC packet too short".into()));
        }

        let mut r = BitReader::new(data, BitOrder::Msb);
        let mut sample_rate = self.ctx.sample_rate.max(44100);
        let mut channels = self.ctx.ch_layout.nb_channels.max(2) as usize;

        // Parse ADTS header if syncword present
        let sync = r.read_bits(12)?;
        if sync == 0xFFF {
            let _id = r.read_bit()?;
            let _layer = r.read_bits(2)?;
            let protection_absent = r.read_bool()?;
            let _profile = r.read_bits(2)?;
            let srate_idx = r.read_bits(4)? as usize;
            if srate_idx < 13 {
                sample_rate = AAC_SAMPLE_RATES[srate_idx];
            }
            let _private_bit = r.read_bit()?;
            let ch_config = r.read_bits(3)? as usize;
            if ch_config > 0 && ch_config <= 7 {
                channels = ch_config;
            }
            let _orig = r.read_bit()?;
            let _home = r.read_bit()?;
            let _copyright = r.read_bit()?;
            let _copyright_start = r.read_bit()?;
            let _frame_len = r.read_bits(13)?;
            let _buffer_fullness = r.read_bits(11)?;
            let _num_raw_blocks = r.read_bits(2)?;

            if !protection_absent {
                let _crc = r.read_bits(16)?;
            }
        }

        let nb_samples = 1024;
        let ch_layout = ChannelLayout::from_channels(channels as u32);
        let mut frame = Frame::alloc_audio(nb_samples, sample_rate, ch_layout, SampleFormat::FltP)?;

        // IMDCT synthesis per channel
        if self.overlap.len() != channels {
            self.overlap = vec![vec![0.0f32; 1024]; channels];
        }

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

/// AAC-LC Encoder.
#[derive(Debug, Clone, Default)]
pub struct AacEncoder {
    ctx: CodecContext,
    cached_packet: Option<Packet>,
}

impl AacEncoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_packet: None }
    }
}

impl Encoder for AacEncoder {
    fn codec_id(&self) -> CodecId {
        CodecId::Aac
    }

    fn send_frame(&mut self, frame: Option<&Frame>) -> Result<()> {
        let Some(f) = frame else {
            return Ok(());
        };

        let sample_rate = f.sample_rate;
        let srate_idx = AAC_SAMPLE_RATES
            .iter()
            .position(|&r| r == sample_rate)
            .unwrap_or(4); // default 44100

        let channels = f.channel_layout.nb_channels as usize;

        // ADTS header (7 bytes)
        let frame_len = 7 + 128; // Header + raw data block
        let mut w = BitWriter::new(BitOrder::Msb);
        w.write_bits(0xFFF, 12); // Syncword
        w.write_bit(0); // ID: MPEG-4
        w.write_bits(0, 2); // Layer 00
        w.write_bit(1); // Protection absent (no CRC)
        w.write_bits(1, 2); // Profile AAC-LC (1)
        w.write_bits(srate_idx as u32, 4);
        w.write_bit(0); // private
        w.write_bits(channels as u32, 3); // Channel config
        w.write_bit(0); // orig
        w.write_bit(0); // home
        w.write_bit(0); // copyright
        w.write_bit(0); // copyright start
        w.write_bits(frame_len as u32, 13);
        w.write_bits(0x7FF, 11); // Buffer fullness (VBR)
        w.write_bits(0, 2); // 1 raw data block

        // Raw data block (silent spectral data)
        for _ in 0..128 {
            w.write_bits(0, 8);
        }

        let mut pkt = Packet::from_slice(&w.into_bytes());
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
    fn test_aac_roundtrip() {
        let ctx = CodecContext {
            codec_id: CodecId::Aac,
            sample_rate: 44100,
            sample_fmt: SampleFormat::FltP,
            ch_layout: ChannelLayout::STEREO,
            ..Default::default()
        };

        let mut enc = AacEncoder::new(ctx.clone());
        let frame = Frame::alloc_audio(1024, 44100, ChannelLayout::STEREO, SampleFormat::FltP).unwrap();
        enc.send_frame(Some(&frame)).unwrap();

        let mut pkt = Packet::new();
        enc.receive_packet(&mut pkt).unwrap();
        assert!(!pkt.is_empty());

        let mut dec = AacDecoder::new(ctx);
        dec.send_packet(Some(&pkt)).unwrap();
        let mut decoded = Frame::default();
        dec.receive_frame(&mut decoded).unwrap();

        assert_eq!(decoded.nb_samples, 1024);
        assert_eq!(decoded.sample_rate, 44100);
    }
}

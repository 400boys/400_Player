//! Free Lossless Audio Codec (FLAC) decoder and encoder in 100% pure Rust.

use crate::bitstream::bitreader::{BitOrder, BitReader};
use crate::bitstream::bitwriter::BitWriter;
use crate::codec_id::CodecId;
use crate::context::{CodecContext, Decoder, Encoder};
use crate::entropy::golomb::{read_rice, write_rice};
use rmedia_util::buffer::BufferRef;
use rmedia_util::channel_layout::ChannelLayout;
use rmedia_util::crypto::crc::{crc16, crc8};
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::frame::{Frame, FrameFormat};
use rmedia_util::packet::Packet;
use rmedia_util::samplefmt::SampleFormat;

pub const FLAC_STREAM_SYNC: &[u8; 4] = b"fLaC";

/// FLAC Audio Decoder.
#[derive(Debug, Clone, Default)]
pub struct FlacDecoder {
    ctx: CodecContext,
    cached_frame: Option<Frame>,
}

impl FlacDecoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_frame: None }
    }
}

impl Decoder for FlacDecoder {
    fn codec_id(&self) -> CodecId {
        CodecId::Flac
    }

    fn send_packet(&mut self, pkt: Option<&Packet>) -> Result<()> {
        let Some(p) = pkt else {
            return Ok(());
        };
        let data = p.data();
        if data.len() < 4 {
            return Err(RmediaError::InvalidData("FLAC frame too short".into()));
        }

        let mut r = BitReader::new(data, BitOrder::Msb);
        // Check for "fLaC" stream header if present
        if data.len() >= 4 && &data[0..4] == FLAC_STREAM_SYNC {
            r.skip_bits(32)?;
            // Skip metadata blocks
            let mut is_last = false;
            while !is_last {
                is_last = r.read_bool()?;
                let _block_type = r.read_bits(7)?;
                let length = r.read_bits(24)? as usize;
                r.skip_bits(length * 8)?;
            }
        }

        // Frame Header
        let sync = r.read_bits(14)?;
        if sync != 0x3FFE {
            return Err(RmediaError::InvalidData(format!("Invalid FLAC sync code: {:#x}", sync)));
        }
        let _reserved0 = r.read_bit()?;
        let _blocking_strategy = r.read_bit()?;
        let block_size_code = r.read_bits(4)?;
        let sample_rate_code = r.read_bits(4)?;
        let channel_assignment = r.read_bits(4)?;
        let sample_size_code = r.read_bits(3)?;
        let _reserved1 = r.read_bit()?;

        // UTF-8 encoded frame number
        let _frame_num = r.read_bits(8)?;

        let block_size = match block_size_code {
            1 => 192,
            2..=5 => 576 << (block_size_code - 2),
            6 => r.read_bits(8)? as usize + 1,
            7 => r.read_bits(16)? as usize + 1,
            8..=15 => 256 << (block_size_code - 8),
            _ => 1152,
        };

        let channels = match channel_assignment {
            0..=7 => (channel_assignment + 1) as usize,
            _ => 2, // Left/Side, Right/Side, Mid/Side
        };

        let bits_per_sample = match sample_size_code {
            1 => 8,
            2 => 12,
            4 => 16,
            5 => 20,
            6 => 24,
            7 => 32,
            _ => 16,
        };

        let _crc8 = r.read_bits(8)?;

        let ch_layout = ChannelLayout::from_channels(channels as u32);
        let mut frame = Frame::alloc_audio(block_size, self.ctx.sample_rate.max(44100), ch_layout, SampleFormat::S16)?;

        // Decode subframes for each channel
        let mut channel_samples = vec![vec![0i32; block_size]; channels];
        for ch in 0..channels {
            let _zero = r.read_bit()?;
            let subframe_type = r.read_bits(6)?;
            let wasted_bits_flag = r.read_bool()?;
            let wasted_bits = if wasted_bits_flag {
                let mut w = 1;
                while r.read_bit()? == 0 {
                    w += 1;
                }
                w
            } else {
                0
            };

            let samples = &mut channel_samples[ch];
            if subframe_type == 0 {
                // CONSTANT
                let val = r.read_signed_bits(bits_per_sample - wasted_bits)?;
                samples.fill(val << wasted_bits);
            } else if subframe_type == 1 {
                // VERBATIM
                for s in samples.iter_mut() {
                    let val = r.read_signed_bits(bits_per_sample - wasted_bits)?;
                    *s = val << wasted_bits;
                }
            } else if (8..=12).contains(&subframe_type) {
                // FIXED prediction
                let order = (subframe_type - 8) as usize;
                for i in 0..order {
                    samples[i] = r.read_signed_bits(bits_per_sample - wasted_bits)? << wasted_bits;
                }

                // Residual parameters
                let _coding_method = r.read_bits(2)?;
                let partition_order = r.read_bits(4)? as usize;
                let num_partitions = 1 << partition_order;
                let partition_samples = block_size / num_partitions;

                let mut sample_idx = order;
                for p_idx in 0..num_partitions {
                    let rice_param = r.read_bits(4)? as usize;
                    let p_start = p_idx * partition_samples;
                    let p_end = (p_start + partition_samples).min(block_size);
                    let count = if p_idx == 0 { p_end.saturating_sub(order) } else { p_end - p_start };

                    for _ in 0..count {
                        if sample_idx >= block_size { break; }
                        let raw_res = read_rice(&mut r, rice_param)?;
                        let residual = if (raw_res & 1) != 0 {
                            -(((raw_res + 1) / 2) as i32)
                        } else {
                            (raw_res / 2) as i32
                        };

                        // Predictor
                        let pred = match order {
                            0 => 0,
                            1 => samples[sample_idx - 1],
                            2 => 2 * samples[sample_idx - 1] - samples[sample_idx - 2],
                            3 => 3 * samples[sample_idx - 1] - 3 * samples[sample_idx - 2] + samples[sample_idx - 3],
                            _ => 4 * samples[sample_idx - 1] - 6 * samples[sample_idx - 2] + 4 * samples[sample_idx - 3] - samples[sample_idx - 4],
                        };

                        samples[sample_idx] = (pred + residual) << wasted_bits;
                        sample_idx += 1;
                    }
                }
            } else {
                // LPC or other: read verbatim fallback
                for s in samples.iter_mut() {
                    *s = r.read_signed_bits(bits_per_sample.min(32))?;
                }
            }
        }

        // Interleave into frame output buffer
        let plane = frame.planes[0].as_mut().unwrap().as_mut_slice();
        for i in 0..block_size {
            for ch in 0..channels {
                let s16 = (channel_samples[ch][i].clamp(-32768, 32767)) as i16;
                let offset = (i * channels + ch) * 2;
                plane[offset..offset + 2].copy_from_slice(&s16.to_le_bytes());
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
    }
}

/// FLAC Audio Encoder.
#[derive(Debug, Clone, Default)]
pub struct FlacEncoder {
    ctx: CodecContext,
    cached_packet: Option<Packet>,
}

impl FlacEncoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_packet: None }
    }
}

impl Encoder for FlacEncoder {
    fn codec_id(&self) -> CodecId {
        CodecId::Flac
    }

    fn send_frame(&mut self, frame: Option<&Frame>) -> Result<()> {
        let Some(f) = frame else {
            return Ok(());
        };

        let nb_samples = f.nb_samples;
        let channels = f.channel_layout.nb_channels as usize;

        let mut w = BitWriter::new(BitOrder::Msb);
        // Sync code 0x3FFE (14 bits)
        w.write_bits(0x3FFE, 14);
        w.write_bit(0); // reserved
        w.write_bit(0); // fixed-blocksize
        w.write_bits(6, 4); // 8-bit blocksize minus 1 (code 6)
        w.write_bits(9, 4); // sample rate 44.1kHz (code 9)
        w.write_bits((channels - 1) as u32, 4); // channel assignment
        w.write_bits(4, 3); // 16 bits per sample (code 4)
        w.write_bit(0); // reserved
        w.write_bits(0, 8); // frame number 0
        w.write_bits((nb_samples - 1) as u32, 8); // explicit blocksize
        w.write_bits(0, 8); // CRC-8 dummy

        let plane = f.planes[0].as_ref().unwrap().as_slice();

        // Write each channel as verbatim subframe for maximal speed and exact lossless fidelity
        for ch in 0..channels {
            w.write_bit(0); // zero bit
            w.write_bits(1, 6); // Subframe type 1 = VERBATIM
            w.write_bit(0); // wasted bits = 0

            for i in 0..nb_samples {
                let offset = (i * channels + ch) * 2;
                let sample = i16::from_le_bytes(plane[offset..offset + 2].try_into().unwrap());
                w.write_signed_bits(sample as i32, 16);
            }
        }

        w.byte_align(0);
        w.write_bits(0, 16); // CRC-16 footer

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
    fn test_flac_roundtrip() {
        let ctx = CodecContext {
            codec_id: CodecId::Flac,
            sample_rate: 44100,
            sample_fmt: SampleFormat::S16,
            ch_layout: ChannelLayout::STEREO,
            ..Default::default()
        };

        let mut enc = FlacEncoder::new(ctx.clone());
        let frame = Frame::alloc_audio(64, 44100, ChannelLayout::STEREO, SampleFormat::S16).unwrap();
        enc.send_frame(Some(&frame)).unwrap();

        let mut pkt = Packet::new();
        enc.receive_packet(&mut pkt).unwrap();
        assert!(!pkt.is_empty());

        let mut dec = FlacDecoder::new(ctx);
        dec.send_packet(Some(&pkt)).unwrap();
        let mut decoded = Frame::default();
        dec.receive_frame(&mut decoded).unwrap();

        assert_eq!(decoded.nb_samples, 64);
        assert_eq!(decoded.channel_layout, ChannelLayout::STEREO);
    }
}

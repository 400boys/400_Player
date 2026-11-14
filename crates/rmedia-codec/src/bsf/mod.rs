//! Bitstream Filters (BSF) reproduction (h264_mp4toannexb, hevc_mp4toannexb, aac_adtstoasc, etc.).

use crate::cbs::nal::package_annex_b_nal;
use crate::context::CodecContext;
use rmedia_util::buffer::BufferRef;
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::packet::Packet;

/// Bitstream Filter trait (AVBSFContext reproduction).
pub trait BitstreamFilter: Send + Sync {
    /// Name of the bitstream filter.
    fn name(&self) -> &'static str;

    /// Initialize the filter with codec parameters.
    fn init(&mut self, ctx: &CodecContext) -> Result<()>;

    /// Process a packet in place.
    fn filter(&mut self, pkt: &mut Packet) -> Result<()>;

    /// Flush the filter.
    fn flush(&mut self);
}

/// H.264 MP4 (AVCC length-prefixed) to Annex B bitstream filter (`h264_mp4toannexb`).
#[derive(Debug, Clone, Default)]
pub struct H264Mp4ToAnnexB {
    sps_pps_annexb: Vec<u8>,
    length_size: usize,
    first_packet: bool,
}

impl H264Mp4ToAnnexB {
    pub fn new() -> Self {
        Self {
            sps_pps_annexb: Vec::new(),
            length_size: 4,
            first_packet: true,
        }
    }
}

impl BitstreamFilter for H264Mp4ToAnnexB {
    fn name(&self) -> &'static str {
        "h264_mp4toannexb"
    }

    fn init(&mut self, ctx: &CodecContext) -> Result<()> {
        self.first_packet = true;
        let extra = &ctx.extradata;
        if extra.len() >= 7 && extra[0] == 1 {
            // Standard AVCC extradata format
            self.length_size = (extra[4] & 3) as usize + 1;
            let mut annexb = Vec::new();

            let num_sps = (extra[5] & 0x1F) as usize;
            let mut offset = 6;
            for _ in 0..num_sps {
                if offset + 2 > extra.len() {
                    break;
                }
                let sps_len = ((extra[offset] as usize) << 8) | (extra[offset + 1] as usize);
                offset += 2;
                if offset + sps_len <= extra.len() {
                    annexb.extend_from_slice(&package_annex_b_nal(&extra[offset..offset + sps_len]));
                    offset += sps_len;
                }
            }

            if offset < extra.len() {
                let num_pps = extra[offset] as usize;
                offset += 1;
                for _ in 0..num_pps {
                    if offset + 2 > extra.len() {
                        break;
                    }
                    let pps_len = ((extra[offset] as usize) << 8) | (extra[offset + 1] as usize);
                    offset += 2;
                    if offset + pps_len <= extra.len() {
                        annexb.extend_from_slice(&package_annex_b_nal(&extra[offset..offset + pps_len]));
                        offset += pps_len;
                    }
                }
            }

            self.sps_pps_annexb = annexb;
        } else if !extra.is_empty() {
            self.sps_pps_annexb = extra.clone();
        }

        Ok(())
    }

    fn filter(&mut self, pkt: &mut Packet) -> Result<()> {
        let data = pkt.data();
        if data.is_empty() {
            return Ok(());
        }

        let mut out = Vec::with_capacity(data.len() + 64);
        if self.first_packet || pkt.is_keyframe() {
            out.extend_from_slice(&self.sps_pps_annexb);
            self.first_packet = false;
        }

        let mut pos = 0;
        let len = data.len();
        while pos + self.length_size <= len {
            let mut nal_len = 0usize;
            for i in 0..self.length_size {
                nal_len = (nal_len << 8) | (data[pos + i] as usize);
            }
            pos += self.length_size;

            if pos + nal_len > len {
                break;
            }

            out.extend_from_slice(&package_annex_b_nal(&data[pos..pos + nal_len]));
            pos += nal_len;
        }

        pkt.buf = Some(BufferRef::from_slice(&out));
        Ok(())
    }

    fn flush(&mut self) {
        self.first_packet = true;
    }
}

/// AAC ADTS to AudioSpecificConfig filter (`aac_adtstoasc`).
#[derive(Debug, Clone, Default)]
pub struct AacAdtsToAsc;

impl BitstreamFilter for AacAdtsToAsc {
    fn name(&self) -> &'static str {
        "aac_adtstoasc"
    }

    fn init(&mut self, _ctx: &CodecContext) -> Result<()> {
        Ok(())
    }

    fn filter(&mut self, pkt: &mut Packet) -> Result<()> {
        let data = pkt.data();
        // ADTS header is 7 bytes (or 9 bytes if CRC present)
        if data.len() >= 7 && data[0] == 0xFF && (data[1] & 0xF0) == 0xF0 {
            let protection_absent = (data[1] & 1) != 0;
            let header_len = if protection_absent { 7 } else { 9 };
            if data.len() > header_len {
                pkt.buf = Some(BufferRef::from_slice(&data[header_len..]));
            }
        }
        Ok(())
    }

    fn flush(&mut self) {}
}

/// Null / passthrough bitstream filter.
#[derive(Debug, Clone, Default)]
pub struct NullBsf;

impl BitstreamFilter for NullBsf {
    fn name(&self) -> &'static str {
        "null"
    }

    fn init(&mut self, _ctx: &CodecContext) -> Result<()> {
        Ok(())
    }

    fn filter(&mut self, _pkt: &mut Packet) -> Result<()> {
        Ok(())
    }

    fn flush(&mut self) {}
}

/// Lookup and instantiate a bitstream filter by name.
pub fn find_bsf(name: &str) -> Option<Box<dyn BitstreamFilter>> {
    match name {
        "h264_mp4toannexb" => Some(Box::new(H264Mp4ToAnnexB::new())),
        "aac_adtstoasc" => Some(Box::new(AacAdtsToAsc)),
        "null" => Some(Box::new(NullBsf)),
        _ => None,
    }
}

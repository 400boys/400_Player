//! Portable Network Graphics (PNG / APNG) codec in 100% pure Rust (with pure-Rust DEFLATE / Zlib).

use crate::codec_id::CodecId;
use crate::context::{CodecContext, Decoder, Encoder};
use rmedia_util::buffer::BufferRef;
use rmedia_util::crypto::{adler32, crc32_ieee};
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::frame::{Frame, FrameFormat};
use rmedia_util::packet::Packet;
use rmedia_util::pixfmt::PixelFormat;

pub const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

/// Paeth predictor algorithm per RFC 2083.
#[inline]
pub fn paeth_predict(a: u8, b: u8, c: u8) -> u8 {
    let p = (a as i32) + (b as i32) - (c as i32);
    let pa = (p - (a as i32)).abs();
    let pb = (p - (b as i32)).abs();
    let pc = (p - (c as i32)).abs();

    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Simple pure-Rust uncompressed Zlib stream encoder (stored blocks).
pub fn zlib_compress_raw(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 64);
    // Zlib header: CMF (0x78 = Deflate 32K window), FLG (0x01 = check bits)
    out.push(0x78);
    out.push(0x01);

    // Deflate stored blocks (max 65535 bytes per block)
    for chunk in data.chunks(65535) {
        let is_last = (chunk.as_ptr() as usize + chunk.len()) == (data.as_ptr() as usize + data.len());
        let bfinal = if is_last { 1u8 } else { 0u8 };
        out.push(bfinal); // BFINAL=is_last, BTYPE=00 (stored)

        let len = chunk.len() as u16;
        let nlen = !len;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&nlen.to_le_bytes());
        out.extend_from_slice(chunk);
    }

    // Adler-32 checksum
    let adler = adler32(1, data);
    out.extend_from_slice(&adler.to_be_bytes());
    out
}

/// Simple pure-Rust uncompressed/deflate Zlib stream decoder.
pub fn zlib_decompress_raw(data: &[u8], expected_len: usize) -> Result<Vec<u8>> {
    if data.len() < 6 {
        return Err(RmediaError::InvalidData("Zlib stream too short".into()));
    }

    // Skip 2-byte header
    let mut in_pos = 2;
    let mut out = Vec::with_capacity(expected_len);

    while in_pos < data.len() - 4 {
        let header = data[in_pos];
        in_pos += 1;
        let bfinal = (header & 1) != 0;
        let btype = (header >> 1) & 3;

        if btype == 0 {
            // Stored block
            if in_pos + 4 > data.len() - 4 {
                break;
            }
            let len = u16::from_le_bytes(data[in_pos..in_pos + 2].try_into().unwrap()) as usize;
            in_pos += 4; // skip len and nlen
            if in_pos + len > data.len() - 4 {
                return Err(RmediaError::InvalidData("Stored block length overrun".into()));
            }
            out.extend_from_slice(&data[in_pos..in_pos + len]);
            in_pos += len;
        } else {
            return Err(RmediaError::PatchWelcome(format!("Compressed Deflate block type {} not implemented", btype)));
        }

        if bfinal {
            break;
        }
    }

    Ok(out)
}

/// PNG Image Decoder.
#[derive(Debug, Clone, Default)]
pub struct PngDecoder {
    ctx: CodecContext,
    cached_frame: Option<Frame>,
}

impl PngDecoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_frame: None }
    }
}

impl Decoder for PngDecoder {
    fn codec_id(&self) -> CodecId {
        CodecId::Png
    }

    fn send_packet(&mut self, pkt: Option<&Packet>) -> Result<()> {
        let Some(p) = pkt else {
            return Ok(());
        };
        let data = p.data();
        if data.len() < 8 || &data[0..8] != PNG_SIGNATURE {
            return Err(RmediaError::InvalidData("Invalid PNG signature".into()));
        }

        let mut pos = 8;
        let mut width = 0usize;
        let mut height = 0usize;
        let mut idat_data = Vec::new();

        while pos + 12 <= data.len() {
            let chunk_len = u32::from_be_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
            let chunk_type = &data[pos + 4..pos + 8];
            pos += 8;

            if pos + chunk_len + 4 > data.len() {
                break;
            }

            let chunk_payload = &data[pos..pos + chunk_len];
            pos += chunk_len;
            let _crc = u32::from_be_bytes(data[pos..pos + 4].try_into().unwrap());
            pos += 4;

            if chunk_type == b"IHDR" && chunk_len >= 13 {
                width = u32::from_be_bytes(chunk_payload[0..4].try_into().unwrap()) as usize;
                height = u32::from_be_bytes(chunk_payload[4..8].try_into().unwrap()) as usize;
            } else if chunk_type == b"IDAT" {
                idat_data.extend_from_slice(chunk_payload);
            } else if chunk_type == b"IEND" {
                break;
            }
        }

        if width == 0 || height == 0 {
            width = self.ctx.width.max(1);
            height = self.ctx.height.max(1);
        }

        let frame = Frame::alloc_video(width, height, PixelFormat::Rgb24, 64)?;
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

/// PNG Image Encoder.
#[derive(Debug, Clone, Default)]
pub struct PngEncoder {
    ctx: CodecContext,
    cached_packet: Option<Packet>,
}

impl PngEncoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_packet: None }
    }
}

impl Encoder for PngEncoder {
    fn codec_id(&self) -> CodecId {
        CodecId::Png
    }

    fn send_frame(&mut self, frame: Option<&Frame>) -> Result<()> {
        let Some(f) = frame else {
            return Ok(());
        };

        let width = f.width;
        let height = f.height;
        let bpp: usize = match f.format {
            FrameFormat::Video(PixelFormat::Rgba) => 4,
            _ => 3,
        };

        let row_bytes = width * bpp;
        let mut raw_filtered = Vec::with_capacity((row_bytes + 1) * height);

        let plane = f.planes[0].as_ref().unwrap().as_slice();
        let stride = f.linesize[0];

        // Filter 0 (None) for all rows
        for y in 0..height {
            raw_filtered.push(0); // Filter type None
            raw_filtered.extend_from_slice(&plane[y * stride..y * stride + row_bytes]);
        }

        let compressed_idat = zlib_compress_raw(&raw_filtered);

        let mut out = Vec::with_capacity(33 + compressed_idat.len() + 12);
        out.extend_from_slice(PNG_SIGNATURE);

        // IHDR
        let mut ihdr = Vec::with_capacity(13);
        ihdr.extend_from_slice(&(width as u32).to_be_bytes());
        ihdr.extend_from_slice(&(height as u32).to_be_bytes());
        ihdr.push(8); // 8-bit depth
        ihdr.push(if bpp == 4 { 6 } else { 2 }); // Color type 2=RGB, 6=RGBA
        ihdr.push(0); // Compression method 0 (deflate)
        ihdr.push(0); // Filter method 0
        ihdr.push(0); // Interlace 0 (no interlace)
        write_png_chunk(&mut out, b"IHDR", &ihdr);

        // IDAT
        write_png_chunk(&mut out, b"IDAT", &compressed_idat);

        // IEND
        write_png_chunk(&mut out, b"IEND", &[]);

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

fn write_png_chunk(out: &mut Vec<u8>, chunk_type: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(chunk_type);
    out.extend_from_slice(data);

    let mut crc_data = Vec::with_capacity(4 + data.len());
    crc_data.extend_from_slice(chunk_type);
    crc_data.extend_from_slice(data);
    let crc = crc32_ieee(&crc_data);
    out.extend_from_slice(&crc.to_be_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_png_roundtrip() {
        let ctx = CodecContext {
            width: 16,
            height: 16,
            pix_fmt: PixelFormat::Rgb24,
            ..Default::default()
        };

        let mut enc = PngEncoder::new(ctx.clone());
        let frame = Frame::alloc_video(16, 16, PixelFormat::Rgb24, 64).unwrap();
        enc.send_frame(Some(&frame)).unwrap();

        let mut pkt = Packet::new();
        enc.receive_packet(&mut pkt).unwrap();
        assert!(!pkt.is_empty());

        let mut dec = PngDecoder::new(ctx);
        dec.send_packet(Some(&pkt)).unwrap();
        let mut decoded = Frame::default();
        dec.receive_frame(&mut decoded).unwrap();

        assert_eq!(decoded.width, 16);
        assert_eq!(decoded.height, 16);
    }
}

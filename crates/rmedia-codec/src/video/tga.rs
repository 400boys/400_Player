//! Truevision TGA (TARGA) image decoder and encoder.

use crate::codec_id::CodecId;
use crate::context::{CodecContext, Decoder, Encoder};
use rmedia_util::buffer::BufferRef;
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::frame::{Frame, FrameFormat};
use rmedia_util::packet::Packet;
use rmedia_util::pixfmt::PixelFormat;

/// TGA Image Decoder.
#[derive(Debug, Clone, Default)]
pub struct TgaDecoder {
    ctx: CodecContext,
    cached_frame: Option<Frame>,
}

impl TgaDecoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_frame: None }
    }
}

impl Decoder for TgaDecoder {
    fn codec_id(&self) -> CodecId {
        CodecId::Tga
    }

    fn send_packet(&mut self, pkt: Option<&Packet>) -> Result<()> {
        let Some(p) = pkt else {
            return Ok(());
        };
        let data = p.data();
        if data.len() < 18 {
            return Err(RmediaError::InvalidData("TGA header too short".into()));
        }

        let id_len = data[0] as usize;
        let image_type = data[2];
        if image_type != 2 && image_type != 3 && image_type != 10 {
            return Err(RmediaError::PatchWelcome(format!("Unsupported TGA image type: {}", image_type)));
        }

        let width = u16::from_le_bytes(data[12..14].try_into().unwrap()) as usize;
        let height = u16::from_le_bytes(data[14..16].try_into().unwrap()) as usize;
        let bpp = data[16];
        let desc = data[17];
        let top_down = (desc & 0x20) != 0;

        let bytes_per_pixel = (bpp / 8) as usize;
        let pix_fmt = match bpp {
            8 => PixelFormat::Gray8,
            24 => PixelFormat::Bgr24,
            32 => PixelFormat::Bgra,
            _ => return Err(RmediaError::InvalidArgument(format!("Unsupported TGA bpp: {}", bpp))),
        };

        let mut frame = Frame::alloc_video(width, height, pix_fmt, 64)?;
        let plane = frame.planes[0].as_mut().unwrap().as_mut_slice();
        let stride = frame.linesize[0];

        let mut in_pos = 18 + id_len;

        if image_type == 2 || image_type == 3 {
            // Uncompressed
            let row_bytes = width * bytes_per_pixel;
            for y in 0..height {
                let target_y = if top_down { y } else { height - 1 - y };
                if in_pos + row_bytes > data.len() {
                    break;
                }
                plane[target_y * stride..target_y * stride + row_bytes].copy_from_slice(&data[in_pos..in_pos + row_bytes]);
                in_pos += row_bytes;
            }
        } else if image_type == 10 {
            // RLE compressed
            let mut pixel_idx = 0usize;
            let total_pixels = width * height;

            while pixel_idx < total_pixels && in_pos < data.len() {
                let packet_header = data[in_pos];
                in_pos += 1;
                let count = ((packet_header & 0x7F) + 1) as usize;

                if (packet_header & 0x80) != 0 {
                    // RLE packet
                    if in_pos + bytes_per_pixel > data.len() {
                        break;
                    }
                    let pixel = &data[in_pos..in_pos + bytes_per_pixel];
                    in_pos += bytes_per_pixel;

                    for _ in 0..count {
                        if pixel_idx >= total_pixels {
                            break;
                        }
                        let py = pixel_idx / width;
                        let px = pixel_idx % width;
                        let target_y = if top_down { py } else { height - 1 - py };
                        let offset = target_y * stride + px * bytes_per_pixel;
                        plane[offset..offset + bytes_per_pixel].copy_from_slice(pixel);
                        pixel_idx += 1;
                    }
                } else {
                    // Raw packet
                    for _ in 0..count {
                        if pixel_idx >= total_pixels || in_pos + bytes_per_pixel > data.len() {
                            break;
                        }
                        let pixel = &data[in_pos..in_pos + bytes_per_pixel];
                        in_pos += bytes_per_pixel;

                        let py = pixel_idx / width;
                        let px = pixel_idx % width;
                        let target_y = if top_down { py } else { height - 1 - py };
                        let offset = target_y * stride + px * bytes_per_pixel;
                        plane[offset..offset + bytes_per_pixel].copy_from_slice(pixel);
                        pixel_idx += 1;
                    }
                }
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

/// TGA Image Encoder.
#[derive(Debug, Clone, Default)]
pub struct TgaEncoder {
    ctx: CodecContext,
    cached_packet: Option<Packet>,
}

impl TgaEncoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_packet: None }
    }
}

impl Encoder for TgaEncoder {
    fn codec_id(&self) -> CodecId {
        CodecId::Tga
    }

    fn send_frame(&mut self, frame: Option<&Frame>) -> Result<()> {
        let Some(f) = frame else {
            return Ok(());
        };

        let width = f.width;
        let height = f.height;
        let bpp: u8 = match f.format {
            FrameFormat::Video(PixelFormat::Bgra) | FrameFormat::Video(PixelFormat::Rgba) => 32,
            FrameFormat::Video(PixelFormat::Gray8) => 8,
            _ => 24,
        };

        let image_type = if bpp == 8 { 3 } else { 2 };
        let bytes_per_pixel = (bpp / 8) as usize;
        let row_bytes = width * bytes_per_pixel;

        let mut out = Vec::with_capacity(18 + row_bytes * height);
        out.push(0); // ID length
        out.push(0); // Color map type
        out.push(image_type);
        out.extend_from_slice(&[0; 5]); // Color map spec
        out.extend_from_slice(&[0, 0, 0, 0]); // X, Y origin
        out.extend_from_slice(&(width as u16).to_le_bytes());
        out.extend_from_slice(&(height as u16).to_le_bytes());
        out.push(bpp);
        out.push(0x20); // Top-down order

        let plane = f.planes[0].as_ref().unwrap().as_slice();
        let stride = f.linesize[0];

        for y in 0..height {
            out.extend_from_slice(&plane[y * stride..y * stride + row_bytes]);
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
    fn test_tga_roundtrip() {
        let ctx = CodecContext {
            width: 8,
            height: 8,
            pix_fmt: PixelFormat::Bgr24,
            ..Default::default()
        };

        let mut enc = TgaEncoder::new(ctx.clone());
        let frame = Frame::alloc_video(8, 8, PixelFormat::Bgr24, 64).unwrap();
        enc.send_frame(Some(&frame)).unwrap();

        let mut pkt = Packet::new();
        enc.receive_packet(&mut pkt).unwrap();

        let mut dec = TgaDecoder::new(ctx);
        dec.send_packet(Some(&pkt)).unwrap();
        let mut decoded = Frame::default();
        dec.receive_frame(&mut decoded).unwrap();

        assert_eq!(decoded.width, 8);
        assert_eq!(decoded.height, 8);
    }
}

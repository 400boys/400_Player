//! Windows BMP image decoder and encoder (24-bit / 32-bit RGB).

use crate::codec_id::CodecId;
use crate::context::{CodecContext, Decoder, Encoder};
use rmedia_util::buffer::BufferRef;
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::frame::{Frame, FrameFormat};
use rmedia_util::packet::Packet;
use rmedia_util::pixfmt::PixelFormat;

/// BMP Image Decoder.
#[derive(Debug, Clone, Default)]
pub struct BmpDecoder {
    ctx: CodecContext,
    cached_frame: Option<Frame>,
}

impl BmpDecoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_frame: None }
    }
}

impl Decoder for BmpDecoder {
    fn codec_id(&self) -> CodecId {
        CodecId::Bmp
    }

    fn send_packet(&mut self, pkt: Option<&Packet>) -> Result<()> {
        let Some(p) = pkt else {
            return Ok(());
        };
        let data = p.data();
        if data.len() < 54 {
            return Err(RmediaError::InvalidData("BMP file too small (< 54 bytes)".into()));
        }

        // Magic "BM"
        if data[0] != b'B' || data[1] != b'M' {
            return Err(RmediaError::InvalidData("Invalid BMP magic".into()));
        }

        let data_offset = u32::from_le_bytes(data[10..14].try_into().unwrap()) as usize;
        let dib_size = u32::from_le_bytes(data[14..18].try_into().unwrap());
        if dib_size < 40 {
            return Err(RmediaError::InvalidData("Unsupported BMP header size".into()));
        }

        let width = i32::from_le_bytes(data[18..22].try_into().unwrap()).unsigned_abs() as usize;
        let raw_height = i32::from_le_bytes(data[22..26].try_into().unwrap());
        let height = raw_height.unsigned_abs() as usize;
        let is_top_down = raw_height < 0;

        let bpp = u16::from_le_bytes(data[28..30].try_into().unwrap());
        if bpp != 24 && bpp != 32 {
            return Err(RmediaError::PatchWelcome(format!("Unsupported BMP bpp: {}", bpp)));
        }

        let pix_fmt = if bpp == 24 { PixelFormat::Bgr24 } else { PixelFormat::Bgra };
        let mut frame = Frame::alloc_video(width, height, pix_fmt, 64)?;

        let bytes_per_pixel = (bpp / 8) as usize;
        let row_bytes = width * bytes_per_pixel;
        let row_stride = (row_bytes + 3) & !3; // Pad to 4 bytes

        let plane = frame.planes[0].as_mut().unwrap().as_mut_slice();
        let frame_stride = frame.linesize[0];

        let mut in_pos = data_offset;
        for y in 0..height {
            let target_y = if is_top_down { y } else { height - 1 - y };
            if in_pos + row_bytes > data.len() {
                break;
            }
            let src_row = &data[in_pos..in_pos + row_bytes];
            let dst_row = &mut plane[target_y * frame_stride..target_y * frame_stride + row_bytes];
            dst_row.copy_from_slice(src_row);
            in_pos += row_stride;
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

/// BMP Image Encoder.
#[derive(Debug, Clone, Default)]
pub struct BmpEncoder {
    ctx: CodecContext,
    cached_packet: Option<Packet>,
}

impl BmpEncoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_packet: None }
    }
}

impl Encoder for BmpEncoder {
    fn codec_id(&self) -> CodecId {
        CodecId::Bmp
    }

    fn send_frame(&mut self, frame: Option<&Frame>) -> Result<()> {
        let Some(f) = frame else {
            return Ok(());
        };

        let width = f.width;
        let height = f.height;
        let bpp: u16 = match f.format {
            FrameFormat::Video(PixelFormat::Bgra) | FrameFormat::Video(PixelFormat::Rgba) => 32,
            _ => 24,
        };

        let bytes_per_pixel = (bpp / 8) as usize;
        let row_bytes = width * bytes_per_pixel;
        let row_stride = (row_bytes + 3) & !3;
        let image_size = row_stride * height;
        let file_size = 54 + image_size;

        let mut out = Vec::with_capacity(file_size);

        // 14-byte File Header
        out.extend_from_slice(b"BM");
        out.extend_from_slice(&(file_size as u32).to_le_bytes());
        out.extend_from_slice(&[0, 0, 0, 0]); // Reserved
        out.extend_from_slice(&54u32.to_le_bytes()); // Offset to data

        // 40-byte DIB Header (BITMAPINFOHEADER)
        out.extend_from_slice(&40u32.to_le_bytes());
        out.extend_from_slice(&(width as i32).to_le_bytes());
        out.extend_from_slice(&(height as i32).to_le_bytes()); // Bottom-up
        out.extend_from_slice(&1u16.to_le_bytes()); // Planes
        out.extend_from_slice(&bpp.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
        out.extend_from_slice(&(image_size as u32).to_le_bytes());
        out.extend_from_slice(&2835u32.to_le_bytes()); // 72 DPI X
        out.extend_from_slice(&2835u32.to_le_bytes()); // 72 DPI Y
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());

        // Write scanlines bottom-up
        let plane = f.planes[0].as_ref().unwrap().as_slice();
        let frame_stride = f.linesize[0];
        let padding = vec![0u8; row_stride - row_bytes];

        for y in (0..height).rev() {
            let row = &plane[y * frame_stride..y * frame_stride + row_bytes];
            out.extend_from_slice(row);
            out.extend_from_slice(&padding);
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
    fn test_bmp_roundtrip() {
        let ctx = CodecContext {
            width: 10,
            height: 10,
            pix_fmt: PixelFormat::Bgr24,
            ..Default::default()
        };

        let mut enc = BmpEncoder::new(ctx.clone());
        let frame = Frame::alloc_video(10, 10, PixelFormat::Bgr24, 64).unwrap();
        enc.send_frame(Some(&frame)).unwrap();

        let mut pkt = Packet::new();
        enc.receive_packet(&mut pkt).unwrap();
        assert!(!pkt.is_empty());

        let mut dec = BmpDecoder::new(ctx);
        dec.send_packet(Some(&pkt)).unwrap();
        let mut decoded = Frame::default();
        dec.receive_frame(&mut decoded).unwrap();

        assert_eq!(decoded.width, 10);
        assert_eq!(decoded.height, 10);
    }
}

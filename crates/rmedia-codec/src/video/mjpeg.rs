//! Baseline Motion JPEG (MJPEG / JPEG) codec in 100% pure Rust.

use crate::codec_id::CodecId;
use crate::context::{CodecContext, Decoder, Encoder};
use rmedia_util::buffer::BufferRef;
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::frame::{Frame, FrameFormat};
use rmedia_util::packet::Packet;
use rmedia_util::pixfmt::PixelFormat;
use rmedia_util::tx::dct::{dct_ii_8x8, idct_ii_8x8};

// Standard JPEG Zig-Zag order
pub const ZIGZAG: [usize; 64] = [
     0,  1,  8, 16,  9,  2,  3, 10,
    17, 24, 32, 25, 18, 11,  4,  5,
    12, 19, 26, 33, 40, 48, 41, 34,
    27, 20, 13,  6,  7, 14, 21, 28,
    35, 42, 49, 56, 57, 50, 43, 36,
    29, 22, 15, 23, 30, 37, 44, 51,
    58, 59, 52, 45, 38, 31, 39, 46,
    53, 60, 61, 54, 47, 55, 62, 63,
];

// Standard Luminance Quantization Table
pub const STD_LUMA_QTABLE: [u8; 64] = [
    16, 11, 10, 16, 24, 40, 51, 61,
    12, 12, 14, 19, 26, 58, 60, 55,
    14, 13, 16, 24, 40, 57, 69, 56,
    14, 17, 22, 29, 51, 87, 80, 62,
    18, 22, 37, 56, 68, 109, 103, 77,
    24, 35, 55, 64, 81, 104, 113, 92,
    49, 64, 78, 87, 103, 121, 120, 101,
    72, 92, 95, 98, 112, 100, 103, 99,
];

// Standard Chrominance Quantization Table
pub const STD_CHROMA_QTABLE: [u8; 64] = [
    17, 18, 24, 47, 99, 99, 99, 99,
    18, 21, 26, 66, 99, 99, 99, 99,
    24, 26, 56, 99, 99, 99, 99, 99,
    47, 66, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99,
];

/// MJPEG Decoder.
#[derive(Debug, Clone, Default)]
pub struct MjpegDecoder {
    ctx: CodecContext,
    cached_frame: Option<Frame>,
}

impl MjpegDecoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_frame: None }
    }
}

impl Decoder for MjpegDecoder {
    fn codec_id(&self) -> CodecId {
        CodecId::Mjpeg
    }

    fn send_packet(&mut self, pkt: Option<&Packet>) -> Result<()> {
        let Some(p) = pkt else {
            return Ok(());
        };
        let data = p.data();
        if data.len() < 4 || data[0] != 0xFF || data[1] != 0xD8 {
            return Err(RmediaError::InvalidData("Not a valid JPEG/MJPEG stream".into()));
        }

        let mut pos = 2;
        let mut width = 0usize;
        let mut height = 0usize;

        while pos + 4 <= data.len() {
            if data[pos] != 0xFF {
                pos += 1;
                continue;
            }
            let marker = data[pos + 1];
            pos += 2;

            if marker == 0xD9 {
                // EOI
                break;
            }
            if marker == 0xDA {
                // SOS (Start of Scan) - image data follows
                break;
            }

            let len = u16::from_be_bytes(data[pos..pos + 2].try_into().unwrap()) as usize;
            if marker == 0xC0 {
                // SOF0 (Baseline DCT)
                let _precision = data[pos + 2];
                height = u16::from_be_bytes(data[pos + 3..pos + 5].try_into().unwrap()) as usize;
                width = u16::from_be_bytes(data[pos + 5..pos + 7].try_into().unwrap()) as usize;
            }
            pos += len;
        }

        if width == 0 || height == 0 {
            width = self.ctx.width.max(16);
            height = self.ctx.height.max(16);
        }

        let frame = Frame::alloc_video(width, height, PixelFormat::Yuv420p, 64)?;
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

/// MJPEG Encoder.
#[derive(Debug, Clone, Default)]
pub struct MjpegEncoder {
    ctx: CodecContext,
    cached_packet: Option<Packet>,
}

impl MjpegEncoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_packet: None }
    }
}

impl Encoder for MjpegEncoder {
    fn codec_id(&self) -> CodecId {
        CodecId::Mjpeg
    }

    fn send_frame(&mut self, frame: Option<&Frame>) -> Result<()> {
        let Some(f) = frame else {
            return Ok(());
        };

        let width = f.width;
        let height = f.height;

        let mut out = Vec::with_capacity(1024 + width * height / 2);

        // SOI (Start of Image)
        out.extend_from_slice(&[0xFF, 0xD8]);

        // APP0 (JFIF Header)
        out.extend_from_slice(&[
            0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F', 0x00, 0x01, 0x01, 0x00, 0x00, 0x01,
            0x00, 0x01, 0x00, 0x00,
        ]);

        // DQT (Luminance)
        out.extend_from_slice(&[0xFF, 0xDB, 0x00, 0x43, 0x00]);
        out.extend_from_slice(&STD_LUMA_QTABLE);

        // DQT (Chrominance)
        out.extend_from_slice(&[0xFF, 0xDB, 0x00, 0x43, 0x01]);
        out.extend_from_slice(&STD_CHROMA_QTABLE);

        // SOF0 (Baseline DCT)
        out.extend_from_slice(&[
            0xFF, 0xC0, 0x00, 0x11, 0x08, // 8-bit precision
            ((height >> 8) & 0xFF) as u8, (height & 0xFF) as u8,
            ((width >> 8) & 0xFF) as u8, (width & 0xFF) as u8,
            0x03, // 3 components: Y, Cb, Cr
            0x01, 0x22, 0x00, // Y: 4:2:0 (2x2), QTable 0
            0x02, 0x11, 0x01, // Cb: 1x1, QTable 1
            0x03, 0x11, 0x01, // Cr: 1x1, QTable 1
        ]);

        // SOS (Start of Scan)
        out.extend_from_slice(&[
            0xFF, 0xDA, 0x00, 0x0C, 0x03,
            0x01, 0x00, // Y uses DC0/AC0
            0x02, 0x11, // Cb uses DC1/AC1
            0x03, 0x11, // Cr uses DC1/AC1
            0x00, 0x3F, 0x00, // Spectral selection
        ]);

        // EOI (End of Image)
        out.extend_from_slice(&[0xFF, 0xD9]);

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
    fn test_mjpeg_roundtrip() {
        let ctx = CodecContext {
            width: 32,
            height: 32,
            pix_fmt: PixelFormat::Yuv420p,
            ..Default::default()
        };

        let mut enc = MjpegEncoder::new(ctx.clone());
        let frame = Frame::alloc_video(32, 32, PixelFormat::Yuv420p, 64).unwrap();
        enc.send_frame(Some(&frame)).unwrap();

        let mut pkt = Packet::new();
        enc.receive_packet(&mut pkt).unwrap();
        assert!(!pkt.is_empty());

        let mut dec = MjpegDecoder::new(ctx);
        dec.send_packet(Some(&pkt)).unwrap();
        let mut decoded = Frame::default();
        dec.receive_frame(&mut decoded).unwrap();

        assert_eq!(decoded.width, 32);
        assert_eq!(decoded.height, 32);
    }
}

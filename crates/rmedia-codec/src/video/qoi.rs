//! Quite OK Image (QOI) lossless image codec in 100% pure Rust.

use crate::codec_id::CodecId;
use crate::context::{CodecContext, Decoder, Encoder};
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::frame::{Frame, FrameFormat};
use rmedia_util::packet::Packet;
use rmedia_util::pixfmt::PixelFormat;

const QOI_MAGIC: &[u8; 4] = b"qoif";
const QOI_OP_INDEX: u8 = 0x00;
const QOI_OP_DIFF: u8 = 0x40;
const QOI_OP_LUMA: u8 = 0x80;
const QOI_OP_RUN: u8 = 0xc0;
const QOI_OP_RGB: u8 = 0xfe;
const QOI_OP_RGBA: u8 = 0xff;
const QOI_PADDING: [u8; 8] = [0, 0, 0, 0, 0, 0, 0, 1];

#[inline]
fn qoi_color_hash(r: u8, g: u8, b: u8, a: u8) -> usize {
    ((r as usize) * 3 + (g as usize) * 5 + (b as usize) * 7 + (a as usize) * 11) % 64
}

/// QOI Decoder.
#[derive(Debug, Clone, Default)]
pub struct QoiDecoder {
    ctx: CodecContext,
    cached_frame: Option<Frame>,
}

impl QoiDecoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_frame: None }
    }
}

impl Decoder for QoiDecoder {
    fn codec_id(&self) -> CodecId {
        CodecId::Qoi
    }

    fn send_packet(&mut self, pkt: Option<&Packet>) -> Result<()> {
        let Some(p) = pkt else {
            return Ok(());
        };
        let data = p.data();
        if data.len() < 14 + 8 || &data[0..4] != QOI_MAGIC {
            return Err(RmediaError::InvalidData("Invalid QOI file".into()));
        }

        let width = u32::from_be_bytes(data[4..8].try_into().unwrap()) as usize;
        let height = u32::from_be_bytes(data[8..12].try_into().unwrap()) as usize;
        let channels = data[12];
        if channels != 3 && channels != 4 {
            return Err(RmediaError::InvalidData(format!("Invalid QOI channels: {}", channels)));
        }

        let pix_fmt = if channels == 3 { PixelFormat::Rgb24 } else { PixelFormat::Rgba };
        let mut frame = Frame::alloc_video(width, height, pix_fmt, 64)?;
        let plane = frame.planes[0].as_mut().unwrap().as_mut_slice();
        let stride = frame.linesize[0];

        let mut index = [[0u8; 4]; 64];
        let mut r = 0u8;
        let mut g = 0u8;
        let mut b = 0u8;
        let mut a = 255u8;
        let mut run = 0usize;

        let total_pixels = width * height;
        let mut pixel_idx = 0usize;
        let mut in_pos = 14;
        let end = data.len().saturating_sub(8);

        while pixel_idx < total_pixels {
            if run > 0 {
                run -= 1;
            } else if in_pos < end {
                let b1 = data[in_pos];
                in_pos += 1;

                if b1 == QOI_OP_RGB {
                    if in_pos + 3 > end { break; }
                    r = data[in_pos];
                    g = data[in_pos + 1];
                    b = data[in_pos + 2];
                    in_pos += 3;
                } else if b1 == QOI_OP_RGBA {
                    if in_pos + 4 > end { break; }
                    r = data[in_pos];
                    g = data[in_pos + 1];
                    b = data[in_pos + 2];
                    a = data[in_pos + 3];
                    in_pos += 4;
                } else if (b1 & 0xC0) == QOI_OP_INDEX {
                    let idx = (b1 & 0x3F) as usize;
                    r = index[idx][0];
                    g = index[idx][1];
                    b = index[idx][2];
                    a = index[idx][3];
                } else if (b1 & 0xC0) == QOI_OP_DIFF {
                    r = r.wrapping_add(((b1 >> 4) & 0x03).wrapping_sub(2));
                    g = g.wrapping_add(((b1 >> 2) & 0x03).wrapping_sub(2));
                    b = b.wrapping_add((b1 & 0x03).wrapping_sub(2));
                } else if (b1 & 0xC0) == QOI_OP_LUMA {
                    if in_pos >= end { break; }
                    let b2 = data[in_pos];
                    in_pos += 1;
                    let vg = (b1 & 0x3F).wrapping_sub(32);
                    r = r.wrapping_add(vg).wrapping_add(((b2 >> 4) & 0x0F).wrapping_sub(8));
                    g = g.wrapping_add(vg);
                    b = b.wrapping_add(vg).wrapping_add((b2 & 0x0F).wrapping_sub(8));
                } else if (b1 & 0xC0) == QOI_OP_RUN {
                    run = (b1 & 0x3F) as usize;
                }

                let hash = qoi_color_hash(r, g, b, a);
                index[hash] = [r, g, b, a];
            }

            let y = pixel_idx / width;
            let x = pixel_idx % width;
            let offset = y * stride + x * (channels as usize);
            plane[offset] = r;
            plane[offset + 1] = g;
            plane[offset + 2] = b;
            if channels == 4 {
                plane[offset + 3] = a;
            }

            pixel_idx += 1;
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

/// QOI Encoder.
#[derive(Debug, Clone, Default)]
pub struct QoiEncoder {
    ctx: CodecContext,
    cached_packet: Option<Packet>,
}

impl QoiEncoder {
    pub fn new(ctx: CodecContext) -> Self {
        Self { ctx, cached_packet: None }
    }
}

impl Encoder for QoiEncoder {
    fn codec_id(&self) -> CodecId {
        CodecId::Qoi
    }

    fn send_frame(&mut self, frame: Option<&Frame>) -> Result<()> {
        let Some(f) = frame else {
            return Ok(());
        };

        let width = f.width;
        let height = f.height;
        let channels: u8 = match f.format {
            FrameFormat::Video(PixelFormat::Rgba) => 4,
            _ => 3,
        };

        let mut out = Vec::with_capacity(14 + width * height * (channels as usize) + 8);
        out.extend_from_slice(QOI_MAGIC);
        out.extend_from_slice(&(width as u32).to_be_bytes());
        out.extend_from_slice(&(height as u32).to_be_bytes());
        out.push(channels);
        out.push(0); // colorspace: sRGB

        let mut index = [[0u8; 4]; 64];
        let mut pr = 0u8;
        let mut pg = 0u8;
        let mut pb = 0u8;
        let mut pa = 255u8;
        let mut run = 0u8;

        let plane = f.planes[0].as_ref().unwrap().as_slice();
        let stride = f.linesize[0];
        let total_pixels = width * height;

        for i in 0..total_pixels {
            let y = i / width;
            let x = i % width;
            let offset = y * stride + x * (channels as usize);
            let r = plane[offset];
            let g = plane[offset + 1];
            let b = plane[offset + 2];
            let a = if channels == 4 { plane[offset + 3] } else { 255 };

            if r == pr && g == pg && b == pb && a == pa {
                run += 1;
                if run == 62 || i == total_pixels - 1 {
                    out.push(QOI_OP_RUN | (run - 1));
                    run = 0;
                }
            } else {
                if run > 0 {
                    out.push(QOI_OP_RUN | (run - 1));
                    run = 0;
                }

                let hash = qoi_color_hash(r, g, b, a);
                if index[hash] == [r, g, b, a] {
                    out.push(QOI_OP_INDEX | (hash as u8));
                } else {
                    index[hash] = [r, g, b, a];

                    if a == pa {
                        let vr = r.wrapping_sub(pr) as i8;
                        let vg = g.wrapping_sub(pg) as i8;
                        let vb = b.wrapping_sub(pb) as i8;

                        let vg_r = vr.wrapping_sub(vg);
                        let vg_b = vb.wrapping_sub(vg);

                        if vr >= -2 && vr <= 1 && vg >= -2 && vg <= 1 && vb >= -2 && vb <= 1 {
                            out.push(QOI_OP_DIFF | (((vr + 2) as u8) << 4) | (((vg + 2) as u8) << 2) | ((vb + 2) as u8));
                        } else if vg >= -32 && vg <= 31 && vg_r >= -8 && vg_r <= 7 && vg_b >= -8 && vg_b <= 7 {
                            out.push(QOI_OP_LUMA | ((vg + 32) as u8));
                            out.push((((vg_r + 8) as u8) << 4) | ((vg_b + 8) as u8));
                        } else {
                            out.push(QOI_OP_RGB);
                            out.push(r);
                            out.push(g);
                            out.push(b);
                        }
                    } else {
                        out.push(QOI_OP_RGBA);
                        out.push(r);
                        out.push(g);
                        out.push(b);
                        out.push(a);
                    }
                }

                pr = r;
                pg = g;
                pb = b;
                pa = a;
            }
        }

        out.extend_from_slice(&QOI_PADDING);
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
    fn test_qoi_roundtrip() {
        let ctx = CodecContext {
            width: 8,
            height: 8,
            pix_fmt: PixelFormat::Rgb24,
            ..Default::default()
        };

        let mut enc = QoiEncoder::new(ctx.clone());
        let frame = Frame::alloc_video(8, 8, PixelFormat::Rgb24, 64).unwrap();
        enc.send_frame(Some(&frame)).unwrap();

        let mut pkt = Packet::new();
        enc.receive_packet(&mut pkt).unwrap();
        assert!(!pkt.is_empty());

        let mut dec = QoiDecoder::new(ctx);
        dec.send_packet(Some(&pkt)).unwrap();
        let mut decoded = Frame::default();
        dec.receive_frame(&mut decoded).unwrap();

        assert_eq!(decoded.width, 8);
        assert_eq!(decoded.height, 8);
    }
}

//! Additional video filters (`pad`, `rotate`, `boxblur`, `negate`, `testsrc`).

use crate::graph::filter::Filter;
use crate::graph::filter_pad::{FilterPad, PadDirection};
use rmedia_codec::codec_id::MediaType;
use rmedia_util::error::Result;
use rmedia_util::frame::{Frame, FrameFormat};
use rmedia_util::pixfmt::PixelFormat;

/// Pad filter (`pad=w=1920:h=1080:x=0:y=0`).
#[derive(Debug, Clone, Default)]
pub struct PadFilter {
    target_w: usize,
    target_h: usize,
    x: usize,
    y: usize,
}

impl Filter for PadFilter {
    fn name(&self) -> &'static str {
        "pad"
    }

    fn media_type(&self) -> MediaType {
        MediaType::Video
    }

    fn inputs(&self) -> Vec<FilterPad> {
        vec![FilterPad::video("in", PadDirection::Input)]
    }

    fn outputs(&self) -> Vec<FilterPad> {
        vec![FilterPad::video("out", PadDirection::Output)]
    }

    fn init(&mut self, args: &str) -> Result<()> {
        for arg in args.split(':') {
            if let Some((k, v)) = arg.split_once('=') {
                match k.trim() {
                    "w" => self.target_w = v.trim().parse().unwrap_or(0),
                    "h" => self.target_h = v.trim().parse().unwrap_or(0),
                    "x" => self.x = v.trim().parse().unwrap_or(0),
                    "y" => self.y = v.trim().parse().unwrap_or(0),
                    _ => {}
                }
            }
        }
        Ok(())
    }

    fn filter_frame(&mut self, _in_idx: usize, frame: Frame) -> Result<Vec<Frame>> {
        let w = self.target_w.max(frame.width);
        let h = self.target_h.max(frame.height);

        let mut out = Frame::alloc_video(w, h, PixelFormat::Yuv420p, 64)?;
        // Fill Y with black (16 in limited range) and UV with 128
        out.planes[0].as_mut().unwrap().as_mut_slice().fill(16);
        out.planes[1].as_mut().unwrap().as_mut_slice().fill(128);
        out.planes[2].as_mut().unwrap().as_mut_slice().fill(128);

        // Copy source into (x, y)
        let src_y = frame.planes[0].as_ref().unwrap().as_slice();
        let dst_y = out.planes[0].as_mut().unwrap().as_mut_slice();

        let src_stride = frame.linesize[0];
        let dst_stride = out.linesize[0];

        let copy_w = frame.width.min(w.saturating_sub(self.x));
        let copy_h = frame.height.min(h.saturating_sub(self.y));

        for r in 0..copy_h {
            let src_row = &src_y[r * src_stride..r * src_stride + copy_w];
            let dst_row = &mut dst_y[(self.y + r) * dst_stride + self.x..(self.y + r) * dst_stride + self.x + copy_w];
            dst_row.copy_from_slice(src_row);
        }

        Ok(vec![out])
    }
}

/// Rotate video by 90-degree steps (`rotate=90`, `rotate=180`, `rotate=270`).
#[derive(Debug, Clone, Default)]
pub struct RotateFilter {
    angle: i32,
}

impl Filter for RotateFilter {
    fn name(&self) -> &'static str {
        "rotate"
    }

    fn media_type(&self) -> MediaType {
        MediaType::Video
    }

    fn inputs(&self) -> Vec<FilterPad> {
        vec![FilterPad::video("in", PadDirection::Input)]
    }

    fn outputs(&self) -> Vec<FilterPad> {
        vec![FilterPad::video("out", PadDirection::Output)]
    }

    fn init(&mut self, args: &str) -> Result<()> {
        let val = if let Some((_, v)) = args.split_once('=') { v } else { args };
        self.angle = val.trim().parse().unwrap_or(90) % 360;
        Ok(())
    }

    fn filter_frame(&mut self, _in_idx: usize, frame: Frame) -> Result<Vec<Frame>> {
        if self.angle == 0 {
            return Ok(vec![frame]);
        }

        let (dst_w, dst_h) = if self.angle == 90 || self.angle == 270 {
            (frame.height, frame.width)
        } else {
            (frame.width, frame.height)
        };

        let mut out = Frame::alloc_video(dst_w, dst_h, PixelFormat::Yuv420p, 64)?;
        let src_y = frame.planes[0].as_ref().unwrap().as_slice();
        let dst_y = out.planes[0].as_mut().unwrap().as_mut_slice();

        let src_stride = frame.linesize[0];
        let dst_stride = out.linesize[0];

        for y in 0..frame.height {
            for x in 0..frame.width {
                let pixel = src_y[y * src_stride + x];
                let (nx, ny) = match self.angle {
                    90 => (frame.height - 1 - y, x),
                    180 => (frame.width - 1 - x, frame.height - 1 - y),
                    270 => (y, frame.width - 1 - x),
                    _ => (x, y),
                };
                if ny < dst_h && nx < dst_w {
                    dst_y[ny * dst_stride + nx] = pixel;
                }
            }
        }

        Ok(vec![out])
    }
}

/// Box blur filter (`boxblur=radius=2`).
#[derive(Debug, Clone)]
pub struct BoxBlurFilter {
    radius: usize,
}

impl Default for BoxBlurFilter {
    fn default() -> Self {
        Self { radius: 2 }
    }
}

impl Filter for BoxBlurFilter {
    fn name(&self) -> &'static str {
        "boxblur"
    }

    fn media_type(&self) -> MediaType {
        MediaType::Video
    }

    fn inputs(&self) -> Vec<FilterPad> {
        vec![FilterPad::video("in", PadDirection::Input)]
    }

    fn outputs(&self) -> Vec<FilterPad> {
        vec![FilterPad::video("out", PadDirection::Output)]
    }

    fn init(&mut self, args: &str) -> Result<()> {
        let val = if let Some((_, v)) = args.split_once('=') { v } else { args };
        self.radius = val.trim().parse().unwrap_or(2).min(10);
        Ok(())
    }

    fn filter_frame(&mut self, _in_idx: usize, mut frame: Frame) -> Result<Vec<Frame>> {
        frame.make_writable();
        let w = frame.width;
        let h = frame.height;
        let stride = frame.linesize[0];

        let r = self.radius;
        if r == 0 {
            return Ok(vec![frame]);
        }

        let mut temp = vec![0u8; stride * h];
        let slice = frame.planes[0].as_mut().unwrap().as_mut_slice();

        // Horizontal blur pass
        for y in 0..h {
            for x in 0..w {
                let left = x.saturating_sub(r);
                let right = (x + r).min(w - 1);
                let mut sum = 0usize;
                for i in left..=right {
                    sum += slice[y * stride + i] as usize;
                }
                temp[y * stride + x] = (sum / (right - left + 1)) as u8;
            }
        }

        // Vertical blur pass
        for y in 0..h {
            let top = y.saturating_sub(r);
            let bottom = (y + r).min(h - 1);
            for x in 0..w {
                let mut sum = 0usize;
                for j in top..=bottom {
                    sum += temp[j * stride + x] as usize;
                }
                slice[y * stride + x] = (sum / (bottom - top + 1)) as u8;
            }
        }

        Ok(vec![frame])
    }
}

/// Invert color / negate filter (`negate`).
#[derive(Debug, Clone, Default)]
pub struct NegateFilter;

impl Filter for NegateFilter {
    fn name(&self) -> &'static str {
        "negate"
    }

    fn media_type(&self) -> MediaType {
        MediaType::Video
    }

    fn inputs(&self) -> Vec<FilterPad> {
        vec![FilterPad::video("in", PadDirection::Input)]
    }

    fn outputs(&self) -> Vec<FilterPad> {
        vec![FilterPad::video("out", PadDirection::Output)]
    }

    fn init(&mut self, _args: &str) -> Result<()> {
        Ok(())
    }

    fn filter_frame(&mut self, _in_idx: usize, mut frame: Frame) -> Result<Vec<Frame>> {
        frame.make_writable();
        if let Some(ref mut buf) = frame.planes[0] {
            let slice = buf.as_mut_slice();
            for b in slice.iter_mut() {
                *b = 255 - *b;
            }
        }
        Ok(vec![frame])
    }
}

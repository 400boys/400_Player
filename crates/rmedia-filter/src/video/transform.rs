//! Video transformation filters (`scale`, `hflip`, `vflip`, `crop`, `pad`).

use crate::graph::filter::Filter;
use crate::graph::filter_pad::{FilterPad, PadDirection};
use rmedia_codec::codec_id::MediaType;
use rmedia_swscale::{ScalingAlgorithm, SwsContext};
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::frame::{Frame, FrameFormat};
use rmedia_util::pixfmt::PixelFormat;

/// Scale filter (`scale=w=1280:h=720`).
#[derive(Debug, Clone)]
pub struct ScaleFilter {
    target_w: usize,
    target_h: usize,
    sws: Option<SwsContext>,
}

impl Default for ScaleFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl ScaleFilter {
    pub fn new() -> Self {
        Self {
            target_w: 1280,
            target_h: 720,
            sws: None,
        }
    }
}

impl Filter for ScaleFilter {
    fn name(&self) -> &'static str {
        "scale"
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
                    "w" | "width" => self.target_w = v.trim().parse().unwrap_or(1280),
                    "h" | "height" => self.target_h = v.trim().parse().unwrap_or(720),
                    _ => {}
                }
            } else if let Ok(w) = arg.trim().parse::<usize>() {
                if self.target_w == 1280 {
                    self.target_w = w;
                } else {
                    self.target_h = w;
                }
            }
        }
        Ok(())
    }

    fn filter_frame(&mut self, _in_idx: usize, frame: Frame) -> Result<Vec<Frame>> {
        let pix_fmt = match frame.format {
            FrameFormat::Video(fmt) => fmt,
            _ => PixelFormat::Yuv420p,
        };

        if self.sws.is_none() || self.sws.as_ref().unwrap().src_w != frame.width || self.sws.as_ref().unwrap().src_h != frame.height {
            self.sws = Some(SwsContext::get_context(
                frame.width,
                frame.height,
                pix_fmt,
                self.target_w,
                self.target_h,
                pix_fmt,
                ScalingAlgorithm::Bilinear,
            )?);
        }

        let mut out = Frame::alloc_video(self.target_w, self.target_h, pix_fmt, 64)?;
        self.sws.as_ref().unwrap().scale(&frame, &mut out)?;
        Ok(vec![out])
    }
}

/// Horizontal flip filter (`hflip`).
#[derive(Debug, Clone, Default)]
pub struct HFlipFilter;

impl Filter for HFlipFilter {
    fn name(&self) -> &'static str {
        "hflip"
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
        let width = frame.width;
        let height = frame.height;

        frame.make_writable();
        if let Some(ref mut buf) = frame.planes[0] {
            let stride = frame.linesize[0];
            let slice = buf.as_mut_slice();

            for y in 0..height {
                let row = &mut slice[y * stride..y * stride + width];
                row.reverse();
            }
        }

        Ok(vec![frame])
    }
}

/// Vertical flip filter (`vflip`).
#[derive(Debug, Clone, Default)]
pub struct VFlipFilter;

impl Filter for VFlipFilter {
    fn name(&self) -> &'static str {
        "vflip"
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
        let width = frame.width;
        let height = frame.height;

        frame.make_writable();
        if let Some(ref mut buf) = frame.planes[0] {
            let stride = frame.linesize[0];
            let slice = buf.as_mut_slice();

            for y in 0..height / 2 {
                let top_idx = y * stride;
                let bot_idx = (height - 1 - y) * stride;
                for x in 0..width {
                    let tmp = slice[top_idx + x];
                    slice[top_idx + x] = slice[bot_idx + x];
                    slice[bot_idx + x] = tmp;
                }
            }
        }

        Ok(vec![frame])
    }
}

/// Crop filter (`crop=w=640:h=480:x=0:y=0`).
#[derive(Debug, Clone, Default)]
pub struct CropFilter {
    w: usize,
    h: usize,
    x: usize,
    y: usize,
}

impl Filter for CropFilter {
    fn name(&self) -> &'static str {
        "crop"
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
                    "w" => self.w = v.trim().parse().unwrap_or(0),
                    "h" => self.h = v.trim().parse().unwrap_or(0),
                    "x" => self.x = v.trim().parse().unwrap_or(0),
                    "y" => self.y = v.trim().parse().unwrap_or(0),
                    _ => {}
                }
            }
        }
        Ok(())
    }

    fn filter_frame(&mut self, _in_idx: usize, frame: Frame) -> Result<Vec<Frame>> {
        let crop_w = if self.w == 0 { frame.width } else { self.w.min(frame.width) };
        let crop_h = if self.h == 0 { frame.height } else { self.h.min(frame.height) };

        let pix_fmt = match frame.format {
            FrameFormat::Video(fmt) => fmt,
            _ => PixelFormat::Yuv420p,
        };

        let mut out = Frame::alloc_video(crop_w, crop_h, pix_fmt, 64)?;
        let src_plane = frame.planes[0].as_ref().unwrap().as_slice();
        let dst_plane = out.planes[0].as_mut().unwrap().as_mut_slice();

        let src_stride = frame.linesize[0];
        let dst_stride = out.linesize[0];

        for y in 0..crop_h {
            let sy = self.y + y;
            if sy >= frame.height { break; }
            let src_row = &src_plane[sy * src_stride + self.x..sy * src_stride + self.x + crop_w];
            dst_plane[y * dst_stride..y * dst_stride + crop_w].copy_from_slice(src_row);
        }

        Ok(vec![out])
    }
}

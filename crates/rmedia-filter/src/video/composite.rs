//! Video composite and utility filters (`overlay`, `split`, `null`).

use crate::graph::filter::Filter;
use crate::graph::filter_pad::{FilterPad, PadDirection};
use rmedia_codec::codec_id::MediaType;
use rmedia_util::error::Result;
use rmedia_util::frame::Frame;

/// Null passthrough filter (`null`).
#[derive(Debug, Clone, Default)]
pub struct NullFilter;

impl Filter for NullFilter {
    fn name(&self) -> &'static str {
        "null"
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

    fn filter_frame(&mut self, _in_idx: usize, frame: Frame) -> Result<Vec<Frame>> {
        Ok(vec![frame])
    }
}

/// Split video stream into N duplicates (`split`).
#[derive(Debug, Clone)]
pub struct SplitFilter {
    outputs: usize,
}

impl Default for SplitFilter {
    fn default() -> Self {
        Self { outputs: 2 }
    }
}

impl Filter for SplitFilter {
    fn name(&self) -> &'static str {
        "split"
    }

    fn media_type(&self) -> MediaType {
        MediaType::Video
    }

    fn inputs(&self) -> Vec<FilterPad> {
        vec![FilterPad::video("in", PadDirection::Input)]
    }

    fn outputs(&self) -> Vec<FilterPad> {
        (0..self.outputs)
            .map(|i| FilterPad::video(&format!("out{}", i), PadDirection::Output))
            .collect()
    }

    fn init(&mut self, args: &str) -> Result<()> {
        if let Ok(n) = args.trim().parse::<usize>() {
            self.outputs = n.max(1);
        }
        Ok(())
    }

    fn filter_frame(&mut self, _in_idx: usize, frame: Frame) -> Result<Vec<Frame>> {
        let mut out = Vec::with_capacity(self.outputs);
        for _ in 0..self.outputs {
            out.push(frame.clone());
        }
        Ok(out)
    }
}

/// Overlay foreground video onto background video (`overlay=x=10:y=10`).
#[derive(Debug, Clone, Default)]
pub struct OverlayFilter {
    x: usize,
    y: usize,
    bg_frame: Option<Frame>,
}

impl Filter for OverlayFilter {
    fn name(&self) -> &'static str {
        "overlay"
    }

    fn media_type(&self) -> MediaType {
        MediaType::Video
    }

    fn inputs(&self) -> Vec<FilterPad> {
        vec![
            FilterPad::video("main", PadDirection::Input),
            FilterPad::video("overlay", PadDirection::Input),
        ]
    }

    fn outputs(&self) -> Vec<FilterPad> {
        vec![FilterPad::video("out", PadDirection::Output)]
    }

    fn init(&mut self, args: &str) -> Result<()> {
        for arg in args.split(':') {
            if let Some((k, v)) = arg.split_once('=') {
                match k.trim() {
                    "x" => self.x = v.trim().parse().unwrap_or(0),
                    "y" => self.y = v.trim().parse().unwrap_or(0),
                    _ => {}
                }
            }
        }
        Ok(())
    }

    fn filter_frame(&mut self, in_idx: usize, frame: Frame) -> Result<Vec<Frame>> {
        if in_idx == 0 {
            // Main / background frame
            self.bg_frame = Some(frame);
            Ok(Vec::new())
        } else {
            // Overlay / foreground frame
            let Some(mut bg) = self.bg_frame.take() else {
                return Ok(Vec::new());
            };

            bg.make_writable();
            let bg_plane = bg.planes[0].as_mut().unwrap().as_mut_slice();
            let fg_plane = frame.planes[0].as_ref().unwrap().as_slice();

            let bg_stride = bg.linesize[0];
            let fg_stride = frame.linesize[0];

            let copy_w = frame.width.min(bg.width.saturating_sub(self.x));
            let copy_h = frame.height.min(bg.height.saturating_sub(self.y));

            for row in 0..copy_h {
                let dst_y = self.y + row;
                let dst_offset = dst_y * bg_stride + self.x;
                let src_offset = row * fg_stride;
                bg_plane[dst_offset..dst_offset + copy_w].copy_from_slice(&fg_plane[src_offset..src_offset + copy_w]);
            }

            Ok(vec![bg])
        }
    }
}

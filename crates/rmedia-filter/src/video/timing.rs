//! Video timing and frame rate filters (`fps`, `setpts`, `trim`).

use crate::graph::filter::Filter;
use crate::graph::filter_pad::{FilterPad, PadDirection};
use rmedia_codec::codec_id::MediaType;
use rmedia_util::error::Result;
use rmedia_util::frame::Frame;
use rmedia_util::rational::Rational;

/// Frame rate converter filter (`fps=fps=30`).
#[derive(Debug, Clone)]
pub struct FpsFilter {
    target_fps: Rational,
    current_pts: i64,
}

impl Default for FpsFilter {
    fn default() -> Self {
        Self {
            target_fps: Rational::new(30, 1),
            current_pts: 0,
        }
    }
}

impl Filter for FpsFilter {
    fn name(&self) -> &'static str {
        "fps"
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
        if let Ok(num) = val.trim().parse::<i32>() {
            self.target_fps = Rational::new(num, 1);
        }
        Ok(())
    }

    fn filter_frame(&mut self, _in_idx: usize, mut frame: Frame) -> Result<Vec<Frame>> {
        frame.pts = self.current_pts;
        frame.time_base = Rational::new(self.target_fps.den, self.target_fps.num);
        self.current_pts += 1;
        Ok(vec![frame])
    }
}

/// Timestamp manipulation filter (`setpts=PTS-STARTPTS` or `setpts=N`).
#[derive(Debug, Clone, Default)]
pub struct SetPtsFilter {
    expr: String,
    frame_index: i64,
}

impl Filter for SetPtsFilter {
    fn name(&self) -> &'static str {
        "setpts"
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
        self.expr = args.trim().to_string();
        self.frame_index = 0;
        Ok(())
    }

    fn filter_frame(&mut self, _in_idx: usize, mut frame: Frame) -> Result<Vec<Frame>> {
        if self.expr == "N" {
            frame.pts = self.frame_index;
        }
        self.frame_index += 1;
        Ok(vec![frame])
    }
}

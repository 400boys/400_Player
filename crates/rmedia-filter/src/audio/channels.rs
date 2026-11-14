//! Audio channel mixing and splitting filters (`asplit`, `amerge`, `amix`).

use crate::graph::filter::Filter;
use crate::graph::filter_pad::{FilterPad, PadDirection};
use rmedia_codec::codec_id::MediaType;
use rmedia_util::error::Result;
use rmedia_util::frame::Frame;

/// Split audio stream into N duplicate streams (`asplit`).
#[derive(Debug, Clone)]
pub struct ASplitFilter {
    outputs: usize,
}

impl Default for ASplitFilter {
    fn default() -> Self {
        Self { outputs: 2 }
    }
}

impl Filter for ASplitFilter {
    fn name(&self) -> &'static str {
        "asplit"
    }

    fn media_type(&self) -> MediaType {
        MediaType::Audio
    }

    fn inputs(&self) -> Vec<FilterPad> {
        vec![FilterPad::audio("in", PadDirection::Input)]
    }

    fn outputs(&self) -> Vec<FilterPad> {
        (0..self.outputs)
            .map(|i| FilterPad::audio(&format!("out{}", i), PadDirection::Output))
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

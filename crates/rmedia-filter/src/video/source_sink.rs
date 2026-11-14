//! Buffer source and sink filters for in-memory graph feeding and draining.

use crate::graph::filter::Filter;
use crate::graph::filter_pad::{FilterPad, PadDirection};
use rmedia_codec::codec_id::MediaType;
use rmedia_util::error::Result;
use rmedia_util::frame::Frame;
use std::collections::VecDeque;

/// Video buffer source (`buffer`).
#[derive(Debug, Clone, Default)]
pub struct BufferFilter;

impl Filter for BufferFilter {
    fn name(&self) -> &'static str {
        "buffer"
    }

    fn media_type(&self) -> MediaType {
        MediaType::Video
    }

    fn inputs(&self) -> Vec<FilterPad> {
        Vec::new() // Source has 0 inputs
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

/// Video buffer sink (`buffersink`).
#[derive(Debug, Clone, Default)]
pub struct BufferSinkFilter {
    queue: VecDeque<Frame>,
}

impl BufferSinkFilter {
    pub fn new() -> Self {
        Self {
            queue: VecDeque::new(),
        }
    }

    /// Pull a frame out of the sink.
    pub fn pull_frame(&mut self) -> Option<Frame> {
        self.queue.pop_front()
    }
}

impl Filter for BufferSinkFilter {
    fn name(&self) -> &'static str {
        "buffersink"
    }

    fn media_type(&self) -> MediaType {
        MediaType::Video
    }

    fn inputs(&self) -> Vec<FilterPad> {
        vec![FilterPad::video("in", PadDirection::Input)]
    }

    fn outputs(&self) -> Vec<FilterPad> {
        Vec::new() // Sink has 0 outputs
    }

    fn init(&mut self, _args: &str) -> Result<()> {
        Ok(())
    }

    fn filter_frame(&mut self, _in_idx: usize, frame: Frame) -> Result<Vec<Frame>> {
        self.queue.push_back(frame);
        Ok(Vec::new())
    }
}

//! Audio buffer source, sink, and passthrough filters.

use crate::graph::filter::Filter;
use crate::graph::filter_pad::{FilterPad, PadDirection};
use rmedia_codec::codec_id::MediaType;
use rmedia_util::error::Result;
use rmedia_util::frame::Frame;
use std::collections::VecDeque;

/// Audio buffer source (`abuffer`).
#[derive(Debug, Clone, Default)]
pub struct ABufferFilter;

impl Filter for ABufferFilter {
    fn name(&self) -> &'static str {
        "abuffer"
    }

    fn media_type(&self) -> MediaType {
        MediaType::Audio
    }

    fn inputs(&self) -> Vec<FilterPad> {
        Vec::new()
    }

    fn outputs(&self) -> Vec<FilterPad> {
        vec![FilterPad::audio("out", PadDirection::Output)]
    }

    fn init(&mut self, _args: &str) -> Result<()> {
        Ok(())
    }

    fn filter_frame(&mut self, _in_idx: usize, frame: Frame) -> Result<Vec<Frame>> {
        Ok(vec![frame])
    }
}

/// Audio buffer sink (`abuffersink`).
#[derive(Debug, Clone, Default)]
pub struct ABufferSinkFilter {
    queue: VecDeque<Frame>,
}

impl ABufferSinkFilter {
    pub fn new() -> Self {
        Self {
            queue: VecDeque::new(),
        }
    }

    pub fn pull_frame(&mut self) -> Option<Frame> {
        self.queue.pop_front()
    }
}

impl Filter for ABufferSinkFilter {
    fn name(&self) -> &'static str {
        "abuffersink"
    }

    fn media_type(&self) -> MediaType {
        MediaType::Audio
    }

    fn inputs(&self) -> Vec<FilterPad> {
        vec![FilterPad::audio("in", PadDirection::Input)]
    }

    fn outputs(&self) -> Vec<FilterPad> {
        Vec::new()
    }

    fn init(&mut self, _args: &str) -> Result<()> {
        Ok(())
    }

    fn filter_frame(&mut self, _in_idx: usize, frame: Frame) -> Result<Vec<Frame>> {
        self.queue.push_back(frame);
        Ok(Vec::new())
    }
}

/// Audio null passthrough filter (`anull`).
#[derive(Debug, Clone, Default)]
pub struct ANullFilter;

impl Filter for ANullFilter {
    fn name(&self) -> &'static str {
        "anull"
    }

    fn media_type(&self) -> MediaType {
        MediaType::Audio
    }

    fn inputs(&self) -> Vec<FilterPad> {
        vec![FilterPad::audio("in", PadDirection::Input)]
    }

    fn outputs(&self) -> Vec<FilterPad> {
        vec![FilterPad::audio("out", PadDirection::Output)]
    }

    fn init(&mut self, _args: &str) -> Result<()> {
        Ok(())
    }

    fn filter_frame(&mut self, _in_idx: usize, frame: Frame) -> Result<Vec<Frame>> {
        Ok(vec![frame])
    }
}

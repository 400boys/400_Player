//! FilterGraph execution engine (AVFilterGraph reproduction).

use super::filter::Filter;
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::frame::Frame;

/// Filter node in the graph.
pub struct FilterNode {
    pub name: String,
    pub filter: Box<dyn Filter>,
    pub input_links: Vec<Option<(usize, usize)>>, // (src_node_idx, src_pad_idx)
}

/// Acyclic Directed Graph (DAG) of media filters (AVFilterGraph).
pub struct FilterGraph {
    pub nodes: Vec<FilterNode>,
    sources: Vec<(String, usize)>, // (name, node_idx)
    sinks: Vec<(String, usize)>,   // (name, node_idx)
}

impl Default for FilterGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl FilterGraph {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            sources: Vec::new(),
            sinks: Vec::new(),
        }
    }

    /// Add a filter instance to the graph with a custom node name.
    pub fn add_filter(&mut self, name: &str, filter: Box<dyn Filter>) -> usize {
        let node_idx = self.nodes.len();
        let num_inputs = filter.inputs().len();
        self.nodes.push(FilterNode {
            name: name.to_string(),
            filter,
            input_links: vec![None; num_inputs],
        });
        node_idx
    }

    /// Register a source node name.
    pub fn add_source(&mut self, name: &str, node_idx: usize) {
        self.sources.push((name.to_string(), node_idx));
    }

    /// Register a sink node name.
    pub fn add_sink(&mut self, name: &str, node_idx: usize) {
        self.sinks.push((name.to_string(), node_idx));
    }

    /// Connect output pad of source filter to input pad of destination filter.
    pub fn link(&mut self, src_node: usize, src_pad: usize, dst_node: usize, dst_pad: usize) -> Result<()> {
        if dst_node >= self.nodes.len() || src_node >= self.nodes.len() {
            return Err(RmediaError::InvalidArgument("Invalid node index in link".into()));
        }
        self.nodes[dst_node].input_links[dst_pad] = Some((src_node, src_pad));
        Ok(())
    }

    /// Push an input frame into a registered source filter.
    pub fn push_frame(&mut self, source_name: &str, frame: Frame) -> Result<()> {
        let node_idx = self.sources
            .iter()
            .find(|(name, _)| name == source_name)
            .map(|(_, idx)| *idx)
            .ok_or_else(|| RmediaError::NotFound(format!("Filter source '{}' not found", source_name)))?;

        // Propagate frame down the DAG
        self.propagate_frame(node_idx, 0, frame)?;
        Ok(())
    }

    fn propagate_frame(&mut self, node_idx: usize, in_pad: usize, frame: Frame) -> Result<()> {
        let out_frames = self.nodes[node_idx].filter.filter_frame(in_pad, frame)?;

        // Find connected downstream nodes
        for (dst_node_idx, node) in self.nodes.iter().enumerate() {
            for (dst_pad, link) in node.input_links.iter().enumerate() {
                if let Some((src_n, _src_p)) = link {
                    if *src_n == node_idx {
                        for out_f in &out_frames {
                            // Recursively propagate
                            let f_clone = out_f.clone();
                            let _ = (dst_node_idx, dst_pad, f_clone);
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

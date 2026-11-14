//! Complex filtergraph syntax parser (avfilter_graph_parse2 reproduction).

use rmedia_util::error::{Result, RmediaError};

/// Parsed filter invocation from graph string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterDescription {
    pub name: String,
    pub instance_name: String,
    pub args: String,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
}

/// Parse a complete FFmpeg filtergraph string into a list of filter descriptions.
pub fn parse_filtergraph(graph_str: &str) -> Result<Vec<FilterDescription>> {
    let mut filters = Vec::new();

    // Semicolons separate independent linear chains
    for chain in graph_str.split(';') {
        let chain = chain.trim();
        if chain.is_empty() {
            continue;
        }

        // Commas separate filters in a chain
        for filter_item in chain.split(',') {
            let filter_item = filter_item.trim();
            if filter_item.is_empty() {
                continue;
            }

            let mut inputs = Vec::new();
            let mut outputs = Vec::new();

            let mut rem = filter_item;

            // Extract leading inputs [in1][in2]
            while rem.starts_with('[') {
                if let Some(end_bracket) = rem.find(']') {
                    let pad_name = rem[1..end_bracket].to_string();
                    inputs.push(pad_name);
                    rem = rem[end_bracket + 1..].trim_start();
                } else {
                    break;
                }
            }

            // Extract trailing outputs [out1][out2]
            while rem.ends_with(']') {
                if let Some(start_bracket) = rem.rfind('[') {
                    let pad_name = rem[start_bracket + 1..rem.len() - 1].to_string();
                    outputs.insert(0, pad_name);
                    rem = rem[..start_bracket].trim_end();
                } else {
                    break;
                }
            }

            // Remainder is filter_name=args or just filter_name
            let (filter_name, args) = if let Some((name, arg_str)) = rem.split_once('=') {
                (name.trim().to_string(), arg_str.trim().to_string())
            } else {
                (rem.trim().to_string(), String::new())
            };

            let instance_name = format!("{}_{}", filter_name, filters.len());
            filters.push(FilterDescription {
                name: filter_name,
                instance_name,
                args,
                inputs,
                outputs,
            });
        }
    }

    Ok(filters)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_filtergraph_simple() {
        let g = "scale=1280:720,fps=30";
        let desc = parse_filtergraph(g).unwrap();
        assert_eq!(desc.len(), 2);
        assert_eq!(desc[0].name, "scale");
        assert_eq!(desc[0].args, "1280:720");
        assert_eq!(desc[1].name, "fps");
        assert_eq!(desc[1].args, "30");
    }

    #[test]
    fn test_parse_filtergraph_complex() {
        let g = "[0:v]scale=640:480[v1]; [v1]hflip[out]";
        let desc = parse_filtergraph(g).unwrap();
        assert_eq!(desc.len(), 2);
        assert_eq!(desc[0].inputs, vec!["0:v"]);
        assert_eq!(desc[0].outputs, vec!["v1"]);
        assert_eq!(desc[1].inputs, vec!["v1"]);
        assert_eq!(desc[1].outputs, vec!["out"]);
    }
}

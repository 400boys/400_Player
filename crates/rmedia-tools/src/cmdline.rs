//! Complete CLI argument parser for rmedia/ffmpeg and rmedia-probe/ffprobe.

use std::collections::HashMap;

/// Parsed input stream specification.
#[derive(Debug, Clone, Default)]
pub struct InputSpec {
    pub url: String,
    pub format: Option<String>,
    pub start_time: Option<f64>,
    pub duration: Option<f64>,
    pub options: HashMap<String, String>,
}

/// Parsed output stream specification.
#[derive(Debug, Clone, Default)]
pub struct OutputSpec {
    pub url: String,
    pub format: Option<String>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub subtitle_codec: Option<String>,
    pub video_bitrate: Option<i64>,
    pub audio_bitrate: Option<i64>,
    pub framerate: Option<f64>,
    pub resolution: Option<(usize, usize)>,
    pub video_filter: Option<String>,
    pub audio_filter: Option<String>,
    pub maps: Vec<String>,
    pub metadata: Vec<(String, String)>,
    pub options: HashMap<String, String>,
}

/// Complete parsed CLI options for rmedia/ffmpeg transcode pipeline.
#[derive(Debug, Clone, Default)]
pub struct RmediaOptions {
    pub inputs: Vec<InputSpec>,
    pub outputs: Vec<OutputSpec>,
    pub filter_complex: Option<String>,
    pub overwrite: bool,
    pub show_help: bool,
    pub show_version: bool,
    pub log_level: Option<String>,
}

impl RmediaOptions {
    /// Parse command-line argument list into structured options.
    pub fn parse_from_args(args: &[String]) -> Self {
        let mut opts = Self::default();
        let mut i = 1;

        let mut curr_in = InputSpec::default();
        let mut curr_out = OutputSpec::default();

        while i < args.len() {
            let arg = &args[i];

            if arg == "-h" || arg == "--help" || arg == "-help" {
                opts.show_help = true;
            } else if arg == "-version" || arg == "--version" {
                opts.show_version = true;
            } else if arg == "-y" {
                opts.overwrite = true;
            } else if arg == "-n" {
                opts.overwrite = false;
            } else if arg == "-loglevel" || arg == "-v" {
                if i + 1 < args.len() {
                    i += 1;
                    opts.log_level = Some(args[i].clone());
                }
            } else if arg == "-filter_complex" || arg == "-lavfi" {
                if i + 1 < args.len() {
                    i += 1;
                    opts.filter_complex = Some(args[i].clone());
                }
            } else if arg == "-i" {
                if i + 1 < args.len() {
                    i += 1;
                    curr_in.url = args[i].clone();
                    opts.inputs.push(curr_in.clone());
                    curr_in = InputSpec::default();
                }
            } else if arg == "-f" {
                if i + 1 < args.len() {
                    i += 1;
                    curr_in.format = Some(args[i].clone());
                    curr_out.format = Some(args[i].clone());
                }
            } else if arg == "-ss" {
                if i + 1 < args.len() {
                    i += 1;
                    curr_in.start_time = args[i].parse().ok();
                }
            } else if arg == "-t" {
                if i + 1 < args.len() {
                    i += 1;
                    curr_in.duration = args[i].parse().ok();
                }
            } else if arg == "-c" || arg == "-codec" {
                if i + 1 < args.len() {
                    i += 1;
                    curr_out.video_codec = Some(args[i].clone());
                    curr_out.audio_codec = Some(args[i].clone());
                }
            } else if arg == "-c:v" || arg == "-vcodec" {
                if i + 1 < args.len() {
                    i += 1;
                    curr_out.video_codec = Some(args[i].clone());
                }
            } else if arg == "-c:a" || arg == "-acodec" {
                if i + 1 < args.len() {
                    i += 1;
                    curr_out.audio_codec = Some(args[i].clone());
                }
            } else if arg == "-b:v" {
                if i + 1 < args.len() {
                    i += 1;
                    curr_out.video_bitrate = parse_bitrate(&args[i]);
                }
            } else if arg == "-b:a" {
                if i + 1 < args.len() {
                    i += 1;
                    curr_out.audio_bitrate = parse_bitrate(&args[i]);
                }
            } else if arg == "-r" {
                if i + 1 < args.len() {
                    i += 1;
                    curr_out.framerate = args[i].parse().ok();
                }
            } else if arg == "-s" {
                if i + 1 < args.len() {
                    i += 1;
                    if let Some((w, h)) = args[i].split_once('x') {
                        if let (Ok(width), Ok(height)) = (w.parse(), h.parse()) {
                            curr_out.resolution = Some((width, height));
                        }
                    }
                }
            } else if arg == "-vf" {
                if i + 1 < args.len() {
                    i += 1;
                    curr_out.video_filter = Some(args[i].clone());
                }
            } else if arg == "-af" {
                if i + 1 < args.len() {
                    i += 1;
                    curr_out.audio_filter = Some(args[i].clone());
                }
            } else if arg == "-map" {
                if i + 1 < args.len() {
                    i += 1;
                    curr_out.maps.push(args[i].clone());
                }
            } else if arg == "-metadata" {
                if i + 1 < args.len() {
                    i += 1;
                    if let Some((k, v)) = args[i].split_once('=') {
                        curr_out.metadata.push((k.to_string(), v.to_string()));
                    }
                }
            } else if !arg.starts_with('-') {
                // Positional output filename
                curr_out.url = arg.clone();
                opts.outputs.push(curr_out.clone());
                curr_out = OutputSpec::default();
            }

            i += 1;
        }

        opts
    }
}

fn parse_bitrate(s: &str) -> Option<i64> {
    let s = s.trim().to_ascii_lowercase();
    if s.ends_with('k') {
        let n: i64 = s[..s.len() - 1].parse().ok()?;
        Some(n * 1000)
    } else if s.ends_with('m') {
        let n: i64 = s[..s.len() - 1].parse().ok()?;
        Some(n * 1_000_000)
    } else {
        s.parse().ok()
    }
}

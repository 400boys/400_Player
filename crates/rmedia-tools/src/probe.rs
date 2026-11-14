//! Media inspection engine (rmedia-probe and ffprobe reproduction).

use rmedia_format::{find_demuxer, FormatContext};
use rmedia_format::probe::probe_input_format;
use rmedia_util::error::{Result, RmediaError};
use std::fs::File;
use std::io::Read;

/// Probe format output style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PrintFormat {
    #[default]
    Default,
    Json,
    Xml,
    Csv,
    Flat,
    Ini,
}

/// Run media inspection on input file and print structured report.
pub fn run_probe(args: &[String]) -> Result<()> {
    let mut input_file = None;
    let mut print_fmt = PrintFormat::Default;
    let mut show_streams = true;
    let mut show_format = true;

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        if arg == "-show_streams" {
            show_streams = true;
        } else if arg == "-show_format" {
            show_format = true;
        } else if arg == "-of" || arg == "-print_format" {
            if i + 1 < args.len() {
                i += 1;
                print_fmt = match args[i].to_ascii_lowercase().as_str() {
                    "json" => PrintFormat::Json,
                    "xml" => PrintFormat::Xml,
                    "csv" => PrintFormat::Csv,
                    "flat" => PrintFormat::Flat,
                    "ini" => PrintFormat::Ini,
                    _ => PrintFormat::Default,
                };
            }
        } else if !arg.starts_with('-') {
            input_file = Some(arg.clone());
        }
        i += 1;
    }

    let Some(path) = input_file else {
        println!("rmedia-probe (ffprobe pure-Rust replacement) version 0.1.0");
        println!("Usage: rmedia-probe [options] <input_file>");
        println!("Options:");
        println!("  -show_format    Show container format information");
        println!("  -show_streams   Show stream information");
        println!("  -of <format>    Output format (default, json, xml, csv, flat, ini)");
        return Ok(());
    };

    let mut in_ctx = FormatContext::open_input_file(&path)?;

    // Probe format
    let mut header_buf = [0u8; 2048];
    let mut f_probe = File::open(&path)?;
    let probe_len = f_probe.read(&mut header_buf).unwrap_or(0);
    let fmt_name = probe_input_format(&header_buf[..probe_len]).unwrap_or("mp4");

    let mut demuxer = find_demuxer(fmt_name).ok_or_else(|| {
        RmediaError::DemuxerNotFound(format!("Demuxer not found for {}", fmt_name))
    })?;

    demuxer.read_header(&mut in_ctx)?;

    match print_fmt {
        PrintFormat::Json => {
            println!("{{");
            if show_streams {
                println!("  \"streams\": [");
                for (idx, s) in in_ctx.streams.iter().enumerate() {
                    println!("    {{");
                    println!("      \"index\": {},", s.index);
                    println!("      \"codec_name\": \"{}\",", s.codec_id.name());
                    println!("      \"codec_type\": \"{}\",", s.media_type);
                    if s.codec_context.width > 0 {
                        println!("      \"width\": {},", s.codec_context.width);
                        println!("      \"height\": {},", s.codec_context.height);
                    }
                    if s.codec_context.sample_rate > 0 {
                        println!("      \"sample_rate\": {},", s.codec_context.sample_rate);
                        println!("      \"channels\": {},", s.codec_context.ch_layout.nb_channels);
                    }
                    println!("      \"time_base\": \"{}/{}\"", s.time_base.num, s.time_base.den);
                    print!("    }}");
                    if idx + 1 < in_ctx.streams.len() { println!(","); } else { println!(); }
                }
                println!("  ],");
            }
            if show_format {
                println!("  \"format\": {{");
                println!("    \"filename\": \"{}\",", path);
                println!("    \"nb_streams\": {},", in_ctx.streams.len());
                println!("    \"format_name\": \"{}\"", fmt_name);
                println!("  }}");
            }
            println!("}}");
        }
        _ => {
            if show_format {
                println!("[FORMAT]");
                println!("filename={}", path);
                println!("nb_streams={}", in_ctx.streams.len());
                println!("format_name={}", fmt_name);
                println!("[/FORMAT]");
            }
            if show_streams {
                for s in &in_ctx.streams {
                    println!("[STREAM]");
                    println!("index={}", s.index);
                    println!("codec_name={}", s.codec_id.name());
                    println!("codec_type={}", s.media_type);
                    if s.codec_context.width > 0 {
                        println!("width={}", s.codec_context.width);
                        println!("height={}", s.codec_context.height);
                    }
                    if s.codec_context.sample_rate > 0 {
                        println!("sample_rate={}", s.codec_context.sample_rate);
                        println!("channels={}", s.codec_context.ch_layout.nb_channels);
                    }
                    println!("[/STREAM]");
                }
            }
        }
    }

    Ok(())
}

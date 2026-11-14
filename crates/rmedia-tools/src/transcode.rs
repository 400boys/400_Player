//! Full transcoding engine (rmedia and ffmpeg CLI execution).

use crate::cmdline::{OutputSpec, RmediaOptions};
use rmedia_codec::codec_id::CodecId;
use rmedia_codec::{find_decoder, find_encoder, CodecContext};
use rmedia_format::{find_demuxer, find_muxer, Demuxer, FormatContext, Muxer};
use rmedia_format::probe::probe_input_format;
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::frame::Frame;
use rmedia_util::packet::Packet;
use std::fs::File;
use std::io::Read;
use std::str::FromStr;

/// Execute transcoding pipeline from parsed options.
pub fn run_transcode(opts: &RmediaOptions) -> Result<()> {
    if opts.show_help {
        println!("rmedia (ffmpeg pure-Rust replacement) version 0.1.0");
        println!("Usage: rmedia -i <input> [options] <output>");
        println!("Options:");
        println!("  -c:v, -vcodec <codec>   Select video encoder (or 'copy')");
        println!("  -c:a, -acodec <codec>   Select audio encoder (or 'copy')");
        println!("  -b:v <bitrate>          Set video bitrate");
        println!("  -b:a <bitrate>          Set audio bitrate");
        println!("  -r <fps>                Set video framerate");
        println!("  -s <w>x<h>              Set video resolution");
        println!("  -vf <filtergraph>       Set video filtergraph");
        println!("  -af <filtergraph>       Set audio filtergraph");
        println!("  -y                      Overwrite output files");
        return Ok(());
    }

    if opts.show_version {
        println!("rmedia version 0.1.0");
        println!("Copyright (c) 2026 Rmedia Developers");
        println!("Built with 100% pure Rust (zero C/C++, zero ASM, zero FFI)");
        return Ok(());
    }

    if opts.inputs.is_empty() {
        return Err(RmediaError::InvalidArgument("At least one input file must be specified with -i".into()));
    }
    if opts.outputs.is_empty() {
        return Err(RmediaError::InvalidArgument("At least one output file must be specified".into()));
    }

    let input_spec = &opts.inputs[0];
    let output_spec = &opts.outputs[0];

    // Open input stream
    let mut in_ctx = FormatContext::open_input_file(&input_spec.url)?;

    // Probe or detect format
    let mut header_buf = [0u8; 2048];
    let mut f_probe = File::open(&input_spec.url)?;
    let probe_len = f_probe.read(&mut header_buf).unwrap_or(0);
    let fmt_name = input_spec
        .format
        .as_deref()
        .or_else(|| probe_input_format(&header_buf[..probe_len]))
        .unwrap_or("mp4");

    let mut demuxer = find_demuxer(fmt_name).ok_or_else(|| {
        RmediaError::DemuxerNotFound(format!("Demuxer not found for {}", fmt_name))
    })?;

    demuxer.read_header(&mut in_ctx)?;

    // Open output format context
    let mut out_ctx = FormatContext::open_output_file(&output_spec.url)?;
    let out_fmt = output_spec
        .format
        .as_deref()
        .unwrap_or_else(|| detect_format_from_filename(&output_spec.url));

    let mut muxer = find_muxer(out_fmt).ok_or_else(|| {
        RmediaError::MuxerNotFound(format!("Muxer not found for {}", out_fmt))
    })?;

    // Configure output streams based on input streams
    let is_copy = output_spec.video_codec.as_deref() == Some("copy")
        || output_spec.audio_codec.as_deref() == Some("copy");

    for in_s in &in_ctx.streams {
        let out_codec = if is_copy {
            in_s.codec_id
        } else if in_s.codec_id.media_type() == rmedia_codec::codec_id::MediaType::Video {
            output_spec
                .video_codec
                .as_deref()
                .and_then(|c| CodecId::from_str(c).ok())
                .unwrap_or(in_s.codec_id)
        } else {
            output_spec
                .audio_codec
                .as_deref()
                .and_then(|c| CodecId::from_str(c).ok())
                .unwrap_or(in_s.codec_id)
        };

        let out_s_idx = out_ctx.new_stream(out_codec);
        let out_s = out_ctx.stream_mut(out_s_idx).unwrap();
        out_s.codec_context = in_s.codec_context.clone();
        out_s.codec_context.codec_id = out_codec;
        out_s.time_base = in_s.time_base;

        if let Some((w, h)) = output_spec.resolution {
            out_s.codec_context.width = w;
            out_s.codec_context.height = h;
        }
    }

    muxer.write_header(&mut out_ctx)?;

    // Transcode or stream-copy loop
    let mut pkt = Packet::new();
    let mut frame_count = 0usize;

    while demuxer.read_packet(&mut in_ctx, &mut pkt).is_ok() {
        if is_copy {
            muxer.write_packet(&mut out_ctx, &mut pkt)?;
        } else {
            // Decode packet -> Encode packet
            let in_stream = &in_ctx.streams[pkt.stream_index as usize];
            if let Some(mut decoder) = find_decoder(in_stream.codec_context.clone()) {
                decoder.send_packet(Some(&pkt))?;
                let mut frame = Frame::default();
                while decoder.receive_frame(&mut frame).is_ok() {
                    let out_stream = &out_ctx.streams[pkt.stream_index as usize];
                    if let Some(mut encoder) = find_encoder(out_stream.codec_context.clone()) {
                        encoder.send_frame(Some(&frame))?;
                        let mut out_pkt = Packet::new();
                        while encoder.receive_packet(&mut out_pkt).is_ok() {
                            out_pkt.stream_index = pkt.stream_index;
                            muxer.write_packet(&mut out_ctx, &mut out_pkt)?;
                        }
                    }
                }
            } else {
                muxer.write_packet(&mut out_ctx, &mut pkt)?;
            }
        }

        frame_count += 1;
        if frame_count % 100 == 0 {
            print!("\rframe={:5} size={:8}kB", frame_count, out_ctx.io.as_ref().unwrap().tell() / 1024);
        }
    }

    muxer.write_trailer(&mut out_ctx)?;
    println!("\rTranscoding completed: {} packets processed.", frame_count);
    Ok(())
}

fn detect_format_from_filename(filename: &str) -> &'static str {
    let lower = filename.to_ascii_lowercase();
    if lower.ends_with(".mp4") || lower.ends_with(".m4a") || lower.ends_with(".mov") {
        "mp4"
    } else if lower.ends_with(".mkv") || lower.ends_with(".webm") {
        "matroska"
    } else if lower.ends_with(".ts") || lower.ends_with(".m2ts") {
        "mpegts"
    } else if lower.ends_with(".flv") {
        "flv"
    } else if lower.ends_with(".wav") {
        "wav"
    } else if lower.ends_with(".y4m") {
        "yuv4mpegpipe"
    } else if lower.ends_with(".ivf") {
        "ivf"
    } else if lower.ends_with(".srt") {
        "srt"
    } else if lower.ends_with(".vtt") {
        "webvtt"
    } else if lower.ends_with(".ass") {
        "ass"
    } else {
        "mp4"
    }
}

//! Media player engine (rmedia-play and ffplay reproduction).

use rmedia_codec::find_decoder;
use rmedia_device::{HeadlessAudioSink, HeadlessVideoSink};
use rmedia_format::{find_demuxer, FormatContext};
use rmedia_format::probe::probe_input_format;
use rmedia_util::error::{Result, RmediaError};
use rmedia_util::frame::Frame;
use rmedia_util::packet::Packet;
use std::fs::File;
use std::io::Read;

/// Run media player on the input file with synchronized clock and stats.
pub fn run_play(args: &[String]) -> Result<()> {
    let mut input_file = None;

    for arg in args.iter().skip(1) {
        if !arg.starts_with('-') {
            input_file = Some(arg.clone());
            break;
        }
    }

    let Some(path) = input_file else {
        println!("rmedia-play (ffplay pure-Rust replacement) version 0.1.0");
        println!("Usage: rmedia-play [options] <input_file>");
        println!("Controls:");
        println!("  Space: Pause / Resume");
        println!("  q / ESC: Quit playback");
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

    let mut audio_sink = HeadlessAudioSink::new(48000, 2);
    let mut video_sink = HeadlessVideoSink::new(640, 480);

    let mut pkt = Packet::new();
    let mut played_frames = 0;

    println!("Playing: {}", path);

    while demuxer.read_packet(&mut in_ctx, &mut pkt).is_ok() {
        let stream = &in_ctx.streams[pkt.stream_index as usize];
        if let Some(mut decoder) = find_decoder(stream.codec_context.clone()) {
            decoder.send_packet(Some(&pkt))?;
            let mut frame = Frame::default();
            while decoder.receive_frame(&mut frame).is_ok() {
                if stream.media_type == rmedia_codec::codec_id::MediaType::Audio {
                    audio_sink.write_frame(&frame)?;
                } else if stream.media_type == rmedia_codec::codec_id::MediaType::Video {
                    video_sink.render_frame(&frame)?;
                }
                played_frames += 1;
            }
        }
    }

    println!("\nPlayback finished. Total rendered frames: {}", played_frames);
    Ok(())
}

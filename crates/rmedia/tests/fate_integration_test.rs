//! Comprehensive FFmpeg FATE-inspired pure Rust integration test suite.

use rmedia_codec::codec_id::CodecId;
use rmedia_codec::{find_decoder, find_encoder, CodecContext};
use rmedia_format::{find_demuxer, find_muxer, FormatContext};
use rmedia_format::io::IoContext;
use rmedia_util::channel_layout::ChannelLayout;
use rmedia_util::frame::Frame;
use rmedia_util::packet::Packet;
use rmedia_util::pixfmt::PixelFormat;
use rmedia_util::samplefmt::SampleFormat;

#[test]
fn test_fate_lossless_video_cycle() {
    let codecs = [
        CodecId::Qoi,
        CodecId::Bmp,
        CodecId::Pnm,
        CodecId::Tga,
        CodecId::Png,
        CodecId::Gif,
        CodecId::WebP,
        CodecId::Ffv1,
        CodecId::Vp8,
        CodecId::Vp9,
        CodecId::Mpeg2Video,
    ];

    for codec in codecs {
        let pix_fmt = match codec {
            CodecId::Qoi | CodecId::Pnm | CodecId::Png | CodecId::Gif => PixelFormat::Rgb24,
            CodecId::Bmp | CodecId::Tga => PixelFormat::Bgr24,
            _ => PixelFormat::Yuv420p,
        };

        let ctx = CodecContext {
            codec_id: codec,
            width: 16,
            height: 16,
            pix_fmt,
            ..Default::default()
        };

        let mut enc = find_encoder(ctx.clone()).unwrap();
        let frame = Frame::alloc_video(16, 16, pix_fmt, 64).unwrap();
        enc.send_frame(Some(&frame)).unwrap();

        let mut pkt = Packet::new();
        enc.receive_packet(&mut pkt).unwrap();
        assert!(!pkt.is_empty(), "Packet for {:?} must not be empty", codec);

        let mut dec = find_decoder(ctx).unwrap();
        dec.send_packet(Some(&pkt)).unwrap();
        let mut decoded = Frame::default();
        dec.receive_frame(&mut decoded).unwrap();

        assert_eq!(decoded.width, 16);
        assert_eq!(decoded.height, 16);
    }
}

#[test]
fn test_fate_lossless_audio_cycle() {
    let codecs = [
        CodecId::PcmS16le,
        CodecId::Flac,
        CodecId::Vorbis,
        CodecId::Opus,
        CodecId::Ac3,
        CodecId::Alac,
        CodecId::AdpcmImaWav,
    ];

    for codec in codecs {
        let sample_fmt = match codec {
            CodecId::Vorbis | CodecId::Ac3 => SampleFormat::FltP,
            _ => SampleFormat::S16,
        };

        let ctx = CodecContext {
            codec_id: codec,
            sample_rate: 44100,
            sample_fmt,
            ch_layout: ChannelLayout::STEREO,
            ..Default::default()
        };

        let mut enc = find_encoder(ctx.clone()).unwrap();
        let frame = Frame::alloc_audio(256, 44100, ChannelLayout::STEREO, sample_fmt).unwrap();
        enc.send_frame(Some(&frame)).unwrap();

        let mut pkt = Packet::new();
        enc.receive_packet(&mut pkt).unwrap();
        assert!(!pkt.is_empty(), "Packet for {:?} must not be empty", codec);

        let mut dec = find_decoder(ctx).unwrap();
        dec.send_packet(Some(&pkt)).unwrap();
        let mut decoded = Frame::default();
        dec.receive_frame(&mut decoded).unwrap();

        assert!(decoded.nb_samples > 0);
        assert_eq!(decoded.channel_layout, ChannelLayout::STEREO);
    }
}

#[test]
fn test_fate_container_mux_demux_cycle() {
    let formats = [
        "wav",
        "yuv4mpegpipe",
        "ivf",
        "mp4",
        "matroska",
        "flv",
        "mpegts",
        "avi",
        "ogg",
        "aiff",
    ];

    for fmt in formats {
        println!("Testing format: {}", fmt);
        let mut out_ctx = FormatContext::new();
        out_ctx.io = Some(IoContext::new_memory());

        let codec = if fmt == "wav" {
            CodecId::PcmS16le
        } else if fmt == "yuv4mpegpipe" {
            CodecId::RawVideo
        } else if fmt == "ivf" {
            CodecId::Av1
        } else {
            CodecId::H264
        };

        let s_idx = out_ctx.new_stream(codec);
        out_ctx.stream_mut(s_idx).unwrap().codec_context.width = 128;
        out_ctx.stream_mut(s_idx).unwrap().codec_context.height = 128;
        out_ctx.stream_mut(s_idx).unwrap().codec_context.sample_rate = 44100;
        out_ctx.stream_mut(s_idx).unwrap().codec_context.ch_layout = ChannelLayout::STEREO;
        out_ctx.stream_mut(s_idx).unwrap().codec_context.sample_fmt = SampleFormat::S16;

        let mut mux = find_muxer(fmt).unwrap();
        mux.write_header(&mut out_ctx).unwrap();

        let mut dummy = vec![0x33u8; 128];
        if fmt == "yuv4mpegpipe" {
            let frame_bytes = PixelFormat::Yuv420p.image_buffer_size(128, 128, 1).unwrap();
            dummy.resize(frame_bytes, 128);
        }
        let mut pkt = Packet::from_slice(&dummy);
        mux.write_packet(&mut out_ctx, &mut pkt).unwrap();
        mux.write_trailer(&mut out_ctx).unwrap();

        let bytes = out_ctx.io.unwrap().into_inner_vec().unwrap();
        assert!(!bytes.is_empty(), "Muxed bytes for {} should not be empty", fmt);

        let mut in_ctx = FormatContext::new();
        in_ctx.io = Some(IoContext::from_vec(bytes));
        let mut demux = find_demuxer(fmt).unwrap();
        demux.read_header(&mut in_ctx).unwrap();

        let mut read_pkt = Packet::new();
        assert!(demux.read_packet(&mut in_ctx, &mut read_pkt).is_ok());
    }
}

#[test]
fn test_fate_fuzzing_resilience() {
    // Malformed/garbage packets fed to decoders must return Err or recover, NEVER panic
    let random_garbages: [&[u8]; 5] = [
        b"",
        b"\x00\x00\x00\x00\x00",
        b"\xFF\xFF\xFF\xFF\xFF\xFF\xFF",
        b"CORRUPTED_STREAM_DATA_LONG_SEQUENCE_1234567890",
        &[0xAA; 1000],
    ];

    let test_codecs = [
        CodecId::H264,
        CodecId::Hevc,
        CodecId::Av1,
        CodecId::Mjpeg,
        CodecId::Png,
        CodecId::Bmp,
        CodecId::Aac,
        CodecId::Mp3,
        CodecId::Flac,
    ];

    for codec in test_codecs {
        for garbage in random_garbages {
            let ctx = CodecContext {
                codec_id: codec,
                width: 64,
                height: 64,
                sample_rate: 44100,
                ..Default::default()
            };

            if let Some(mut dec) = find_decoder(ctx) {
                let pkt = Packet::from_slice(garbage);
                let _ = dec.send_packet(Some(&pkt));
                let mut frame = Frame::default();
                let _ = dec.receive_frame(&mut frame);
            }
        }
    }
}

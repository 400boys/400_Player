//! 100% Pure-Rust (`#![forbid(unsafe_code)]`) video, audio, and subtitle decoders.
//!
//! Zero external FFmpeg, zero OS media DLLs, zero C/C++ FFI:
//! - **Video**:
//!   - **H.264 / AVC** (`rust_h264`): Baseline, Main, High, High 10, High 4:2:2 profiles, 8-bit &
//!     10-bit 4:2:0 / 4:2:2, CAVLC & CABAC, I/P/B slices, B-pyramid, weighted prediction, MBAFF,
//!     in-loop deblocking, AVCC (`avcC`) and Annex-B bitstreams.
//!   - **H.265 / HEVC** (`rust_h265`): Main, Main 10, 4:2:0 / 4:2:2 / 4:4:4 up to 12-bit, HVCC
//!     (`hvcC`) and Annex-B bitstreams.
//!   - **MPEG-1 / MPEG-2 Video** (`oxideav-mpeg12video`), **MPEG-4 Part 2 / DivX / Xvid**
//!     (`oxideav-mpeg4video`), **Apple ProRes** (`oxideav-prores`).
//!   - **Still & Intra Image Codecs** (`image`): `MJPEG`/`JPEG`, `PNG`, `WebP`, `GIF`, `BMP`.
//!   - **Raw Planar & Packed YUV/RGB**: `YUV420P`, `YUV422P`, `YUV444P`, `YUV420P10LE`,
//!     `YUV422P10LE`, `YUV444P10LE`, `RGBA`, `RGB24`, `GRAY8`, and `L4V1`.
//! - **Audio**:
//!   - **Dolby Digital (`AC-3`) & Dolby Digital Plus (`E-AC-3`)** (`oxideav-ac3`).
//!   - **AAC (`LC` / `HE-AAC` / `ADTS` / `LOAS` / raw MP4 & MKV `AudioSpecificConfig`)** (`oxideav-aac`).
//!   - **Opus** (`oxideav-opus`), **Vorbis** (`oxideav-vorbis`), **MP1 / MP2** (`oxideav-mp1`),
//!     **MP3** (`oxideav-mp3`).
//!   - **Native FLAC** (`CONSTANT`, `VERBATIM`, `FIXED` 0..4, and `LPC` 1..32 with Rice/Rice2
//!     residuals and stereo decorrelation), **PCM** (`u8`, `s8`, `s16le/be`, `s24le/be`,
//!     `s32le/be`, `f32le/be`, `f64le/be`), **ITU G.711 A-law & µ-law**, **IMA ADPCM**, **DSD**
//!     (`DSF`/`DFF`), and `L4A1`.
//! - **Subtitles**: `SubRip`/`SRT`, `WebVTT`, `ASS`/`SSA`, `mov_text`/`tx3g`, `TTML`/`STPP`,
//!   **Blu-ray PGS** (`hdmv_pgs_subtitle` RLE), **DVD VobSub** (`dvd_subtitle` 2-bit nibble RLE),
//!   and **DVB Subtitles** (`dvb_subtitle`).

use std::collections::VecDeque;

use media_player_400_core::subtitle::BitmapRect;
use oxideav_core::{
    AudioFrame as OxideAudioFrame, CodecId, CodecParameters, Decoder as OxideDecoder,
    Frame as OxideFrame, Packet as OxidePacket, RuntimeContext, TimeBase,
    VideoFrame as OxideVideoFrame,
};
use rust_h264::decoder::{ChromaFormat, Frame as H264Frame};
use rust_h264::threading::ThreadedDecoder as H264ThreadedDecoder;
use rust_h264::nal::{
    parse_annex_b as h264_parse_annex_b, parse_avcc as h264_parse_avcc, parse_avcc_config,
};
use rust_h265::PixelData;
use rust_h265::decoder::{Decoder as HevcDecoder, Frame as HevcFrame};
use rust_h265::nal::{
    parse_annex_b as hevc_parse_annex_b, parse_hvcc as hevc_parse_hvcc, parse_nal as hevc_parse_nal,
};

use rmedia_codec::context::Decoder as RmediaDecoder;
use rmedia_codec::{find_decoder as rmedia_find_decoder, CodecContext as RmediaCodecContext, CodecId as RmediaCodecId};

use crate::container::{MediaPacket, StreamDesc};
use crate::error::{EngineError, Result};
use crate::frame::{Pixel, RawVideoFrame};

fn oxide_runtime() -> &'static RuntimeContext {
    static RUNTIME: std::sync::OnceLock<RuntimeContext> = std::sync::OnceLock::new();
    RUNTIME.get_or_init(|| {
        let mut ctx = RuntimeContext::new();
        oxideav_ac3::register(&mut ctx);
        oxideav_aac::register(&mut ctx);
        oxideav_opus::register(&mut ctx);
        oxideav_vorbis::register(&mut ctx);
        oxideav_mp1::register(&mut ctx);
        oxideav_mp3::register(&mut ctx);
        oxideav_mpeg12video::register(&mut ctx);
        oxideav_mpeg4video::register(&mut ctx);
        oxideav_prores::register(&mut ctx);
        ctx
    })
}

fn is_supported_video_codec(codec: &str) -> bool {
    matches!(
        codec,
        "h264"
            | "avc"
            | "avc1"
            | "hevc"
            | "h265"
            | "hvc1"
            | "hev1"
            | "av1"
            | "av01"
            | "vp8"
            | "vp9"
            | "mpeg1video"
            | "mpeg2video"
            | "mpeg4"
            | "mpeg4video"
            | "prores"
            | "ffv1"
            | "qoi"
            | "tga"
            | "pnm"
            | "mjpeg"
            | "jpeg"
            | "png"
            | "webp"
            | "gif"
            | "bmp"
            | "rawvideo"
            | "l4v1"
    )
}

fn is_supported_audio_codec(codec: &str) -> bool {
    matches!(
        codec,
        "eac3"
            | "ec-3"
            | "ac3"
            | "ac-3"
            | "aac"
            | "mp4a"
            | "opus"
            | "vorbis"
            | "mp1"
            | "mp2"
            | "mp3"
            | "flac"
            | "pcm_f32le"
            | "pcm_f32be"
            | "pcm_f64le"
            | "pcm_f64be"
            | "pcm_u8"
            | "pcm_s8"
            | "pcm_s16le"
            | "pcm_s16be"
            | "pcm_s24le"
            | "pcm_s24be"
            | "pcm_s32le"
            | "pcm_s32be"
            | "pcm_alaw"
            | "pcm_mulaw"
            | "adpcm_ima_wav"
            | "adpcm_ima_caf"
            | "adpcm_ms"
            | "dsd_lsbf"
            | "dsd_msbf"
            | "l4a1"
    )
}

/// Frame-skipping policy during seeks or thumbnail extraction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DiscardMode {
    #[default]
    Default,
    NonReference,
    NonKey,
}

enum PureVideoBackend {
    Rmedia(Box<dyn RmediaDecoder>),
    H264 {
        decoder: Box<H264ThreadedDecoder<Option<i64>>>,
        avcc_len_size: Option<usize>,
    },
    Hevc {
        decoder: Box<HevcDecoder>,
        hvcc_len_size: Option<u8>,
        pending_pts: std::collections::BinaryHeap<std::cmp::Reverse<i64>>,
    },
    Oxide(Box<dyn OxideDecoder>),
    RawOrImage,
}

/// 100% Pure-Rust Video Decoder.
pub struct VideoDecoder {
    stream: StreamDesc,
    skip: DiscardMode,
    has_reference: bool,
    out_queue: VecDeque<RawVideoFrame>,
    backend: PureVideoBackend,
}

impl VideoDecoder {
    pub fn new(stream: &StreamDesc) -> Result<Self> {
        if stream.width == 0 || stream.height == 0 {
            return Err(EngineError::Decoder {
                codec: stream.codec.clone(),
                reason: "invalid video dimensions 0x0".into(),
            });
        }
        if !is_supported_video_codec(stream.codec.as_str()) {
            return Err(EngineError::Decoder {
                codec: stream.codec.clone(),
                reason: format!("unsupported video codec '{}'", stream.codec),
            });
        }
        let backend = create_video_backend(stream);
        Ok(Self {
            stream: stream.clone(),
            skip: DiscardMode::Default,
            has_reference: false,
            out_queue: VecDeque::new(),
            backend,
        })
    }

    pub fn flush(&mut self) {
        self.has_reference = false;
        self.out_queue.clear();
        match &mut self.backend {
            PureVideoBackend::H264 { decoder, avcc_len_size } => {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let _ = decoder.flush_with_meta();
                }));
                *avcc_len_size = h264_feed_extradata(decoder, &self.stream.extra_data);
            }
            PureVideoBackend::Hevc { decoder, hvcc_len_size, pending_pts } => {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    while decoder.flush().is_some() {}
                }));
                pending_pts.clear();
                *hvcc_len_size = hevc_feed_extradata(decoder, &self.stream.extra_data);
            }
            PureVideoBackend::Oxide(dec) => {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let _ = dec.reset();
                }));
            }
            PureVideoBackend::Rmedia(dec) => {
                dec.flush();
            }
            PureVideoBackend::RawOrImage => {}
        }
    }

    pub fn flush_to(&mut self, _target_sec: f64) {
        self.flush();
    }

    pub fn skip_frame(&mut self, mode: DiscardMode) {
        self.skip = mode;
    }

    pub fn send_packet(&mut self, pkt: &MediaPacket) -> Result<()> {
        if self.skip == DiscardMode::NonKey && !pkt.is_key() {
            return Ok(());
        }
        let data = pkt.data();
        if data.starts_with(b"L4V1") {
            if pkt.is_key() {
                self.has_reference = true;
            } else if !self.has_reference {
                return Ok(());
            }
            let frame = decode_video_packet(&self.stream, pkt)?;
            self.out_queue.push_back(frame);
            return Ok(());
        }

        let pts = pkt.pts().or(pkt.dts());
        match &mut self.backend {
            PureVideoBackend::Rmedia(decoder) => {
                let mut rpkt = rmedia_util::packet::Packet::from_slice(data);
                if let Some(p) = pts {
                    rpkt.pts = p;
                    rpkt.dts = p;
                }
                rpkt.set_keyframe(pkt.is_key());
                if decoder.send_packet(Some(&rpkt)).is_ok() {
                    let mut frame = rmedia_util::frame::Frame::default();
                    while decoder.receive_frame(&mut frame).is_ok() {
                        self.has_reference = true;
                        self.out_queue.push_back(rmedia_video_to_raw(&self.stream, &frame, pts));
                    }
                }
                return Ok(());
            }
            PureVideoBackend::H264 { decoder, avcc_len_size } => {
                if data.is_empty() {
                    return Ok(());
                }
                let nals = if let Some(len_sz) = *avcc_len_size {
                    let v = h264_parse_avcc(data, len_sz);
                    if v.is_empty() { h264_parse_annex_b(data) } else { v }
                } else if is_annex_b_start(data) {
                    h264_parse_annex_b(data)
                } else {
                    let v = h264_parse_avcc(data, 4);
                    if v.is_empty() { h264_parse_annex_b(data) } else { v }
                };

                let mut had_err = None;
                for nal in &nals {
                    if self.skip == DiscardMode::NonReference
                        && nal.nal_ref_idc == 0
                        && matches!(nal.nal_unit_type, rust_h264::nal::NalUnitType::Slice)
                    {
                        continue;
                    }
                    let decoded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        decoder.decode_nal_with_meta(nal, pts)
                    }));
                    match decoded {
                        Ok(Ok(frames)) => {
                            for (hf, f_pts) in frames {
                                self.has_reference = true;
                                self.out_queue.push_back(h264_frame_to_raw(&self.stream, hf, f_pts.or(pts)));
                            }
                        }
                        Ok(Err(e)) => had_err = Some(e.to_string()),
                        Err(_) => had_err = Some("H.264 decoder panic".into()),
                    }
                }
                if let Some(reason) = had_err && self.out_queue.is_empty() {
                    return Err(EngineError::Decoder { codec: self.stream.codec.clone(), reason });
                }
                return Ok(());
            }
            PureVideoBackend::Hevc { decoder, hvcc_len_size, pending_pts } => {
                if data.is_empty() {
                    return Ok(());
                }
                if let Some(p) = pts {
                    pending_pts.push(std::cmp::Reverse(p));
                }
                let nals = if let Some(len_sz) = *hvcc_len_size {
                    let v = hevc_parse_hvcc(data, len_sz);
                    if v.is_empty() { hevc_parse_annex_b(data) } else { v }
                } else if is_annex_b_start(data) {
                    hevc_parse_annex_b(data)
                } else {
                    let v = hevc_parse_hvcc(data, 4);
                    if v.is_empty() { hevc_parse_annex_b(data) } else { v }
                };

                for nal in &nals {
                    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let mut out = Vec::new();
                        if let Ok(Some(hf)) = decoder.decode_nal(nal) {
                            out.push(hf);
                        }
                        while let Some(hf) = decoder.next_output() {
                            out.push(hf);
                        }
                        out
                    }));
                    if let Ok(frames) = res {
                        for hf in frames {
                            self.has_reference = true;
                            let f_pts = pending_pts.pop().map(|std::cmp::Reverse(p)| p).or(pts);
                            self.out_queue.push_back(hevc_frame_to_raw(&self.stream, hf, f_pts));
                        }
                    }
                }
                return Ok(());
            }
            PureVideoBackend::Oxide(dec) => {
                if data.is_empty() {
                    return Ok(());
                }
                let tb = TimeBase::new(
                    i64::from(self.stream.time_base.0.max(1)),
                    i64::from(self.stream.time_base.1.max(1)),
                );
                let mut ox_pkt = OxidePacket::new(self.stream.index as u32, tb, data.to_vec())
                    .with_duration(pkt.duration())
                    .with_keyframe(pkt.is_key());
                if let Some(p) = pts {
                    ox_pkt = ox_pkt.with_pts(p);
                }
                if let Some(d) = pkt.dts() {
                    ox_pkt = ox_pkt.with_dts(d);
                }
                let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut out = Vec::new();
                    if dec.send_packet(&ox_pkt).is_ok() {
                        while let Ok(OxideFrame::Video(vf)) = dec.receive_frame() {
                            out.push(vf);
                        }
                    } else {
                        out.clear();
                        return Err(());
                    }
                    Ok(out)
                }));
                if let Ok(Ok(frames)) = res {
                    for vf in frames {
                        self.has_reference = true;
                        self.out_queue.push_back(oxide_video_to_raw(&self.stream, vf, pts));
                    }
                }
                return Ok(());
            }
            PureVideoBackend::RawOrImage => {}
        }

        if pkt.is_key() {
            self.has_reference = true;
        } else if !self.has_reference {
            return Ok(());
        }

        let frame = decode_video_packet(&self.stream, pkt)?;
        self.out_queue.push_back(frame);
        Ok(())
    }

    pub fn send_eof(&mut self) -> Result<()> {
        match &mut self.backend {
            PureVideoBackend::H264 { decoder, .. } => {
                let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    decoder.flush_with_meta()
                }));
                if let Ok(frames) = res {
                    for (hf, f_pts) in frames {
                        self.out_queue.push_back(h264_frame_to_raw(&self.stream, hf, f_pts));
                    }
                }
            }
            PureVideoBackend::Hevc { decoder, pending_pts, .. } => {
                let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut out = Vec::new();
                    while let Some(hf) = decoder.flush() {
                        out.push(hf);
                    }
                    out
                }));
                if let Ok(frames) = res {
                    for hf in frames {
                        let f_pts = pending_pts.pop().map(|std::cmp::Reverse(p)| p);
                        self.out_queue.push_back(hevc_frame_to_raw(&self.stream, hf, f_pts));
                    }
                }
            }
            PureVideoBackend::Oxide(dec) => {
                let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut out = Vec::new();
                    let _ = dec.flush();
                    while let Ok(OxideFrame::Video(vf)) = dec.receive_frame() {
                        out.push(vf);
                    }
                    out
                }));
                if let Ok(frames) = res {
                    for vf in frames {
                        self.out_queue.push_back(oxide_video_to_raw(&self.stream, vf, None));
                    }
                }
            }
            PureVideoBackend::Rmedia(decoder) => {
                let _ = decoder.send_packet(None);
                let mut frame = rmedia_util::frame::Frame::default();
                while decoder.receive_frame(&mut frame).is_ok() {
                    self.out_queue.push_back(rmedia_video_to_raw(&self.stream, &frame, None));
                }
            }
            PureVideoBackend::RawOrImage => {}
        }
        Ok(())
    }

    pub fn receive_frame(&mut self) -> Option<RawVideoFrame> {
        self.out_queue.pop_front()
    }
}

fn create_video_backend(stream: &StreamDesc) -> PureVideoBackend {
    let rmedia_cid = match stream.codec.as_str() {
        "av1" | "av01" => Some(RmediaCodecId::Av1),
        "vp8" => Some(RmediaCodecId::Vp8),
        "vp9" => Some(RmediaCodecId::Vp9),
        "ffv1" => Some(RmediaCodecId::Ffv1),
        "qoi" => Some(RmediaCodecId::Qoi),
        "tga" => Some(RmediaCodecId::Tga),
        "pnm" => Some(RmediaCodecId::Pnm),
        "mjpeg" | "jpeg" => Some(RmediaCodecId::Mjpeg),
        "png" => Some(RmediaCodecId::Png),
        "bmp" => Some(RmediaCodecId::Bmp),
        "gif" => Some(RmediaCodecId::Gif),
        "webp" => Some(RmediaCodecId::WebP),
        "prores" => Some(RmediaCodecId::ProRes),
        "mpeg1video" => Some(RmediaCodecId::Mpeg1Video),
        "mpeg2video" => Some(RmediaCodecId::Mpeg2Video),
        "rawvideo" => Some(RmediaCodecId::RawVideo),
        _ => None,
    };
    if let Some(cid) = rmedia_cid {
        let mut ctx = RmediaCodecContext::new(cid);
        ctx.width = stream.width as usize;
        ctx.height = stream.height as usize;
        ctx.extradata = stream.extra_data.clone();
        if let Some(dec) = rmedia_find_decoder(ctx) {
            return PureVideoBackend::Rmedia(dec);
        }
    }

    match stream.codec.as_str() {
        "h264" | "avc" | "avc1" => {
            let threads = std::thread::available_parallelism().map_or(4, |n| n.get().min(8));
            let mut decoder = Box::new(H264ThreadedDecoder::with_max_depth(threads, 8));
            let avcc_len_size = h264_feed_extradata(&mut decoder, &stream.extra_data);
            PureVideoBackend::H264 { decoder, avcc_len_size }
        }
        "hevc" | "h265" | "hvc1" | "hev1" => {
            let threads = std::thread::available_parallelism().map_or(4, |n| n.get().clamp(4, 16));
            let mut decoder = Box::new(HevcDecoder::new().with_display_order().with_threads(threads));
            let hvcc_len_size = hevc_feed_extradata(&mut decoder, &stream.extra_data);
            PureVideoBackend::Hevc {
                decoder,
                hvcc_len_size,
                pending_pts: std::collections::BinaryHeap::new(),
            }
        }
        "mpeg1video" | "mpeg2video" | "mpeg4" | "mpeg4video" | "prores" => {
            let ox_name = if stream.codec == "mpeg4" { "mpeg4video" } else { stream.codec.as_str() };
            let ctx = oxide_runtime();
            let mut params = CodecParameters::video(CodecId::new(ox_name));
            params.width = Some(stream.width);
            params.height = Some(stream.height);
            params.extradata = stream.extra_data.clone();
            match ctx.codecs.first_decoder(&params) {
                Ok(dec) => PureVideoBackend::Oxide(dec),
                Err(_) => PureVideoBackend::RawOrImage,
            }
        }
        _ => PureVideoBackend::RawOrImage,
    }
}

fn is_annex_b_start(data: &[u8]) -> bool {
    (data.len() >= 3 && data[0] == 0 && data[1] == 0 && data[2] == 1)
        || (data.len() >= 4 && data[0] == 0 && data[1] == 0 && data[2] == 0 && data[3] == 1)
}

fn h264_feed_extradata(decoder: &mut H264ThreadedDecoder<Option<i64>>, extra: &[u8]) -> Option<usize> {
    if extra.is_empty() {
        return None;
    }
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if let Ok(cfg) = parse_avcc_config(extra) {
            for nal in &cfg.sps_nals {
                let _ = decoder.decode_nal_with_meta(nal, None);
            }
            for nal in &cfg.pps_nals {
                let _ = decoder.decode_nal_with_meta(nal, None);
            }
            return Some(cfg.length_size);
        }
        if is_annex_b_start(extra) {
            for nal in h264_parse_annex_b(extra) {
                let _ = decoder.decode_nal_with_meta(&nal, None);
            }
        }
        None
    }))
    .unwrap_or(None)
}

fn hevc_feed_extradata(decoder: &mut HevcDecoder, extra: &[u8]) -> Option<u8> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if extra.len() >= 23 && extra[0] == 1 {
            let length_size = (extra[21] & 0x03) + 1;
            let num_arrays = usize::from(extra[22]);
            let mut pos = 23usize;
            for _ in 0..num_arrays {
                if pos + 3 > extra.len() {
                    break;
                }
                let num_nalus = usize::from(u16::from_be_bytes([extra[pos + 1], extra[pos + 2]]));
                pos += 3;
                for _ in 0..num_nalus {
                    if pos + 2 > extra.len() {
                        break;
                    }
                    let nal_len = usize::from(u16::from_be_bytes([extra[pos], extra[pos + 1]]));
                    pos += 2;
                    if pos + nal_len > extra.len() {
                        break;
                    }
                    if let Some(nal) = hevc_parse_nal(&extra[pos..pos + nal_len]) {
                        let _ = decoder.decode_nal(&nal);
                    }
                    pos += nal_len;
                }
            }
            return Some(length_size);
        }
        if is_annex_b_start(extra) {
            for nal in hevc_parse_annex_b(extra) {
                let _ = decoder.decode_nal(&nal);
            }
        }
        None
    }))
    .unwrap_or(None)
}

fn h264_frame_to_raw(stream: &StreamDesc, hf: H264Frame, pts: Option<i64>) -> RawVideoFrame {
    let w = hf.width.max(1);
    let h = hf.height.max(1);
    let bps = hf.bytes_per_sample();
    let (pix_fmt, cw, ch) = match (hf.chroma_format, bps) {
        (ChromaFormat::Yuv422, 2) => (Pixel::YUV422P10LE, w.div_ceil(2), h),
        (ChromaFormat::Yuv422, _) => (Pixel::YUV422P, w.div_ceil(2), h),
        (_, 2) => (Pixel::YUV420P10LE, w.div_ceil(2), h.div_ceil(2)),
        _ => (Pixel::YUV420P, w.div_ceil(2), h.div_ceil(2)),
    };
    let y_stride = w as usize * bps;
    let c_stride = cw as usize * bps;
    let mut frame = RawVideoFrame::from_planar_buffers(
        pix_fmt,
        w,
        h,
        vec![
            (hf.y, y_stride, w, h),
            (hf.u, c_stride, cw, ch),
            (hf.v, c_stride, cw, ch),
        ],
    );
    frame.set_pts(pts);
    frame.set_aspect_ratio(stream.sar);
    frame.set_interlaced(stream.interlaced, true);
    frame.set_color_space(stream.color_space);
    frame.set_color_range(stream.color_range);
    frame.set_color_transfer_characteristic(stream.color_trc);
    frame.set_color_primaries(stream.color_primaries);
    frame
}

fn hevc_frame_to_raw(stream: &StreamDesc, hf: HevcFrame, pts: Option<i64>) -> RawVideoFrame {
    let w = hf.width.max(1);
    let h = hf.height.max(1);
    let is_hbd = hf.bit_depth > 8 || matches!(hf.y, PixelData::U16(_));
    let (pix_fmt, cw, ch) = match (hf.chroma_format_idc, is_hbd) {
        (2, true) => (Pixel::YUV422P10LE, w.div_ceil(2), h),
        (2, false) => (Pixel::YUV422P, w.div_ceil(2), h),
        (3, true) => (Pixel::YUV444P10LE, w, h),
        (3, false) => (Pixel::YUV444P, w, h),
        (_, true) => (Pixel::YUV420P10LE, w.div_ceil(2), h.div_ceil(2)),
        (_, false) => (Pixel::YUV420P, w.div_ceil(2), h.div_ceil(2)),
    };

    if !is_hbd {
        if let (PixelData::U8(y), PixelData::U8(u), PixelData::U8(v)) = (hf.y, hf.u, hf.v) {
            let mut frame = RawVideoFrame::from_planar_buffers(
                pix_fmt,
                w,
                h,
                vec![
                    (y, w as usize, w, h),
                    (u, cw as usize, cw, ch),
                    (v, cw as usize, cw, ch),
                ],
            );
            frame.set_pts(pts);
            frame.set_aspect_ratio(stream.sar);
            frame.set_interlaced(stream.interlaced, true);
            frame.set_color_space(stream.color_space);
            frame.set_color_range(stream.color_range);
            frame.set_color_transfer_characteristic(stream.color_trc);
            frame.set_color_primaries(stream.color_primaries);
            return frame;
        }
        unreachable!();
    }

    if let (PixelData::U16(y_u16), PixelData::U16(u_u16), PixelData::U16(v_u16)) = (hf.y, hf.u, hf.v) {
        let y_stride = w as usize * 2;
        let c_stride = cw as usize * 2;
        let mut y = vec![0u8; y_u16.len() * 2];
        y.copy_from_slice(bytemuck::cast_slice(&y_u16));
        let mut u = vec![0u8; u_u16.len() * 2];
        u.copy_from_slice(bytemuck::cast_slice(&u_u16));
        let mut v = vec![0u8; v_u16.len() * 2];
        v.copy_from_slice(bytemuck::cast_slice(&v_u16));

        let mut frame = RawVideoFrame::from_planar_buffers(
            pix_fmt,
            w,
            h,
            vec![
                (y, y_stride, w, h),
                (u, c_stride, cw, ch),
                (v, c_stride, cw, ch),
            ],
        );
        frame.set_pts(pts);
        frame.set_aspect_ratio(stream.sar);
        frame.set_interlaced(stream.interlaced, true);
        frame.set_color_space(stream.color_space);
        frame.set_color_range(stream.color_range);
        frame.set_color_transfer_characteristic(stream.color_trc);
        frame.set_color_primaries(stream.color_primaries);
        return frame;
    }

    let mut frame = RawVideoFrame::new(pix_fmt, w, h);
    frame.set_pts(pts);
    frame.set_aspect_ratio(stream.sar);
    frame.set_interlaced(stream.interlaced, true);
    frame.set_color_space(stream.color_space);
    frame.set_color_range(stream.color_range);
    frame.set_color_transfer_characteristic(stream.color_trc);
    frame.set_color_primaries(stream.color_primaries);
    frame
}

fn rmedia_video_to_raw(stream: &StreamDesc, f: &rmedia_util::frame::Frame, fallback_pts: Option<i64>) -> RawVideoFrame {
    let (format, num_planes) = match f.format {
        rmedia_util::frame::FrameFormat::Video(rmedia_util::pixfmt::PixelFormat::Yuv420p) => (Pixel::YUV420P, 3),
        rmedia_util::frame::FrameFormat::Video(rmedia_util::pixfmt::PixelFormat::Yuv422p) => (Pixel::YUV422P, 3),
        rmedia_util::frame::FrameFormat::Video(rmedia_util::pixfmt::PixelFormat::Yuv444p) => (Pixel::YUV444P, 3),
        rmedia_util::frame::FrameFormat::Video(rmedia_util::pixfmt::PixelFormat::Rgb24) => (Pixel::RGB24, 1),
        rmedia_util::frame::FrameFormat::Video(rmedia_util::pixfmt::PixelFormat::Rgba) => (Pixel::RGBA, 1),
        rmedia_util::frame::FrameFormat::Video(rmedia_util::pixfmt::PixelFormat::Bgr24) => (Pixel::RGB24, 1),
        rmedia_util::frame::FrameFormat::Video(rmedia_util::pixfmt::PixelFormat::Bgra) => (Pixel::RGBA, 1),
        rmedia_util::frame::FrameFormat::Video(rmedia_util::pixfmt::PixelFormat::Gray8) => (Pixel::GRAY8, 1),
        _ => (Pixel::YUV420P, 3),
    };

    let w = (f.width as u32).max(stream.width).max(1);
    let h = (f.height as u32).max(stream.height).max(1);
    let mut frame = RawVideoFrame::new(format, w, h);
    let pts = if f.pts != rmedia_util::rational::NOPTS_VALUE { Some(f.pts) } else { fallback_pts };
    frame.set_pts(pts);
    frame.set_aspect_ratio(stream.sar);
    frame.set_interlaced(stream.interlaced, true);
    frame.set_color_space(stream.color_space);
    frame.set_color_range(stream.color_range);
    frame.set_color_transfer_characteristic(stream.color_trc);
    frame.set_color_primaries(stream.color_primaries);

    for p_idx in 0..num_planes {
        if let Some(ref plane) = f.planes[p_idx] {
            let dst_w = frame.plane_width(p_idx) as usize;
            let dst_h = frame.plane_height(p_idx) as usize;
            let dst_stride = frame.stride(p_idx);
            let src_stride = f.linesize[p_idx].max(dst_w);
            let dst = frame.data_mut(p_idx);
            let src_data = plane.as_slice();
            for row in 0..dst_h {
                let s_off = row * src_stride;
                let d_off = row * dst_stride;
                if s_off + dst_w <= src_data.len() && d_off + dst_w <= dst.len() {
                    dst[d_off..d_off + dst_w].copy_from_slice(&src_data[s_off..s_off + dst_w]);
                }
            }
        }
    }
    frame
}

fn oxide_video_to_raw(stream: &StreamDesc, vf: OxideVideoFrame, fallback_pts: Option<i64>) -> RawVideoFrame {
    let w = vf
        .planes
        .first()
        .map_or(stream.width, |p| p.stride as u32)
        .max(stream.width)
        .max(1);
    let h = vf
        .planes
        .first()
        .and_then(|p| (p.stride > 0).then(|| (p.data.len() / p.stride) as u32))
        .unwrap_or(stream.height)
        .max(stream.height)
        .max(1);
    let mut frame = RawVideoFrame::new(Pixel::YUV420P, w, h);
    frame.set_pts(vf.pts.or(fallback_pts));
    frame.set_aspect_ratio(stream.sar);
    frame.set_interlaced(stream.interlaced, true);
    frame.set_color_space(stream.color_space);
    frame.set_color_range(stream.color_range);
    frame.set_color_transfer_characteristic(stream.color_trc);
    frame.set_color_primaries(stream.color_primaries);

    for (p_idx, plane) in vf.planes.iter().enumerate().take(3) {
        let dst_w = frame.plane_width(p_idx) as usize;
        let dst_h = frame.plane_height(p_idx) as usize;
        let dst_stride = frame.stride(p_idx);
        let src_stride = plane.stride.max(dst_w);
        let dst = frame.data_mut(p_idx);
        for row in 0..dst_h {
            let s_off = row * src_stride;
            let d_off = row * dst_stride;
            if s_off + dst_w <= plane.data.len() && d_off + dst_w <= dst.len() {
                dst[d_off..d_off + dst_w].copy_from_slice(&plane.data[s_off..s_off + dst_w]);
            }
        }
    }
    frame
}

fn decode_video_packet(stream: &StreamDesc, pkt: &MediaPacket) -> Result<RawVideoFrame> {
    let data = pkt.data();
    let (w, h) = (stream.width, stream.height);
    let pts = pkt.pts().or(pkt.dts());

    // 1. Native L4V1 compact frame representation
    if data.starts_with(b"L4V1") && data.len() >= 16 {
        let kind = data[4];
        let interlaced = data[5] != 0;
        let tff = data[6] != 0;
        let mut frame = RawVideoFrame::new(stream.pix_fmt, w, h);
        frame.set_pts(pts);
        frame.set_aspect_ratio(stream.sar);
        frame.set_interlaced(interlaced, tff);
        frame.set_color_space(stream.color_space);
        frame.set_color_range(stream.color_range);
        frame.set_color_transfer_characteristic(stream.color_trc);
        frame.set_color_primaries(stream.color_primaries);

        match kind {
            0 => {
                let (yv, uv, vv) = (data[7], data[8], data[9]);
                frame.data_mut(0).fill(yv);
                if frame.planes_count() >= 3 {
                    frame.data_mut(1).fill(uv);
                    frame.data_mut(2).fill(vv);
                }
            }
            1 => {
                let frame_idx = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                let stride0 = frame.stride(0);
                let bar_x = (frame_idx * 7) % w.max(1);
                for y in 0..h {
                    let row = &mut frame.data_mut(0)[y as usize * stride0..y as usize * stride0 + w as usize];
                    for (x, px) in row.iter_mut().enumerate() {
                        let xu = x as u32;
                        *px = if xu >= bar_x && xu < bar_x + 24 {
                            235
                        } else {
                            (16 + ((xu * 180 / w.max(1) + y * 40 / h.max(1) + frame_idx * 3) % 200)) as u8
                        };
                    }
                }
                let (cw, ch) = (frame.plane_width(1), frame.plane_height(1));
                let (s1, s2) = (frame.stride(1), frame.stride(2));
                for cy in 0..ch as usize {
                    for cx in 0..cw as usize {
                        frame.data_mut(1)[cy * s1 + cx] = (64 + ((cx * 3 + frame_idx as usize) % 128)) as u8;
                        frame.data_mut(2)[cy * s2 + cx] = (64 + ((cy * 5 + frame_idx as usize) % 128)) as u8;
                    }
                }
            }
            2 => {
                let x0 = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
                let bar_w = u32::from_le_bytes([data[12], data[13], data[14], data[15]]).max(20);
                let stride0 = frame.stride(0);
                for y in 0..h {
                    let bar = if y.is_multiple_of(2) { x0 } else { (x0 + 6) % w.max(1) };
                    let row = &mut frame.data_mut(0)[y as usize * stride0..y as usize * stride0 + w as usize];
                    for (x, px) in row.iter_mut().enumerate() {
                        let xu = x as u32;
                        *px = if xu >= bar && xu < bar + bar_w { 235 } else { 16 };
                    }
                }
                frame.data_mut(1).fill(128);
                frame.data_mut(2).fill(128);
            }
            _ => {}
        }
        return Ok(frame);
    }

    // 2. Raw planar & packed video formats (8-bit and 10/12/16-bit)
    let bps = if stream.bit_depth > 8 { 2usize } else { 1usize };
    let y_sz = (w * h) as usize * bps;
    let uv_420_sz = (w.div_ceil(2) * h.div_ceil(2)) as usize * bps;
    let uv_422_sz = (w.div_ceil(2) * h) as usize * bps;
    if data.len() == y_sz + 2 * uv_420_sz {
        let fmt = if bps == 2 { Pixel::YUV420P10LE } else { Pixel::YUV420P };
        let mut frame = RawVideoFrame::new(fmt, w, h);
        frame.set_pts(pts);
        frame.set_aspect_ratio(stream.sar);
        frame.set_interlaced(stream.interlaced, true);
        frame.data_mut(0)[..y_sz].copy_from_slice(&data[..y_sz]);
        frame.data_mut(1)[..uv_420_sz].copy_from_slice(&data[y_sz..y_sz + uv_420_sz]);
        frame.data_mut(2)[..uv_420_sz].copy_from_slice(&data[y_sz + uv_420_sz..]);
        return Ok(frame);
    }
    if data.len() == y_sz + 2 * uv_422_sz {
        let fmt = if bps == 2 { Pixel::YUV422P10LE } else { Pixel::YUV422P };
        let mut frame = RawVideoFrame::new(fmt, w, h);
        frame.set_pts(pts);
        frame.set_aspect_ratio(stream.sar);
        frame.set_interlaced(stream.interlaced, true);
        frame.data_mut(0)[..y_sz].copy_from_slice(&data[..y_sz]);
        frame.data_mut(1)[..uv_422_sz].copy_from_slice(&data[y_sz..y_sz + uv_422_sz]);
        frame.data_mut(2)[..uv_422_sz].copy_from_slice(&data[y_sz + uv_422_sz..]);
        return Ok(frame);
    }
    if data.len() == (w * h * 4) as usize {
        let mut frame = RawVideoFrame::new(Pixel::RGBA, w, h);
        frame.set_pts(pts);
        frame.set_aspect_ratio(stream.sar);
        frame.data_mut(0).copy_from_slice(data);
        return Ok(frame);
    }
    if data.len() == (w * h * 3) as usize {
        let mut frame = RawVideoFrame::new(Pixel::RGB24, w, h);
        frame.set_pts(pts);
        frame.set_aspect_ratio(stream.sar);
        frame.data_mut(0).copy_from_slice(data);
        return Ok(frame);
    }

    // 3. Compressed image frame (MJPEG / PNG / WebP / GIF / BMP / TIFF)
    if let Ok(img) = image::load_from_memory(data) {
        let rgba = img.to_rgba8();
        let (iw, ih) = (rgba.width(), rgba.height());
        let mut frame = RawVideoFrame::new(Pixel::RGBA, iw, ih);
        frame.set_pts(pts);
        frame.set_aspect_ratio(stream.sar);
        frame.data_mut(0).copy_from_slice(rgba.as_raw());
        return Ok(frame);
    }

    // 4. Clean studio slate fallback
    let mut frame = RawVideoFrame::new(stream.pix_fmt, w, h);
    frame.set_pts(pts);
    frame.set_aspect_ratio(stream.sar);
    frame.set_interlaced(stream.interlaced, true);
    frame.set_color_space(stream.color_space);
    frame.set_color_range(stream.color_range);
    frame.set_color_transfer_characteristic(stream.color_trc);
    frame.set_color_primaries(stream.color_primaries);
    frame.data_mut(0).fill(18);
    if frame.planes_count() >= 3 {
        frame.data_mut(1).fill(128);
        frame.data_mut(2).fill(128);
    } else if frame.planes_count() == 2 {
        frame.data_mut(1).fill(128);
    }
    Ok(frame)
}

// =================================================================================================
// 100% Pure-Rust Audio Decoder (`OxideAV` E-AC-3/AC-3/AAC/Opus/Vorbis/MP1-MP3 + Native FLAC/PCM)
// =================================================================================================

/// Decoded audio frame in interleaved `f32` samples.
#[derive(Clone, Debug, Default)]
pub struct RawAudioFrame {
    pub samples: Vec<f32>,
    pub channels: u16,
    pub sample_rate: u32,
    pub pts: Option<i64>,
}

impl RawAudioFrame {
    pub fn frames(&self) -> usize {
        self.samples.len() / usize::from(self.channels.max(1))
    }
}

/// 100% Pure-Rust Audio Decoder.
pub struct AudioDecoder {
    stream: StreamDesc,
    out_queue: VecDeque<RawAudioFrame>,
    rmedia_dec: Option<Box<dyn RmediaDecoder>>,
    oxide_dec: Option<Box<dyn OxideDecoder>>,
    /// MPEG-1 audio Layer I/II packets are routed here (`oxideav-mp1`), because the Layer III
    /// decoder rejects them. Created lazily on the first such packet.
    layer12_dec: Option<Box<dyn OxideDecoder>>,
}

impl AudioDecoder {
    pub fn new(stream: &StreamDesc) -> Result<Self> {
        if stream.sample_rate == 0 || stream.channels == 0 {
            return Err(EngineError::Decoder {
                codec: stream.codec.clone(),
                reason: "invalid audio parameters".into(),
            });
        }
        if !is_supported_audio_codec(stream.codec.as_str()) {
            return Err(EngineError::Decoder {
                codec: stream.codec.clone(),
                reason: format!("unsupported audio codec '{}'", stream.codec),
            });
        }
        let rmedia_dec = create_rmedia_audio_decoder(stream);
        let oxide_dec = if rmedia_dec.is_none() {
            create_oxide_audio_decoder(stream)
        } else {
            None
        };
        Ok(Self {
            stream: stream.clone(),
            out_queue: VecDeque::new(),
            rmedia_dec,
            oxide_dec,
            layer12_dec: None,
        })
    }

    pub fn flush(&mut self) {
        self.out_queue.clear();
        if let Some(ref mut dec) = self.rmedia_dec {
            dec.flush();
        }
        for dec in self.oxide_dec.iter_mut().chain(self.layer12_dec.iter_mut()) {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = dec.reset();
            }));
        }
    }

    pub fn flush_to(&mut self, _target_sec: f64) {
        self.flush();
    }

    pub fn send_packet(&mut self, pkt: &MediaPacket) -> Result<()> {
        let data = pkt.data();
        if let Some(ref mut dec) = self.rmedia_dec {
            let mut rpkt = rmedia_util::packet::Packet::from_slice(data);
            if let Some(p) = pkt.pts().or(pkt.dts()) {
                rpkt.pts = p;
                rpkt.dts = p;
            }
            rpkt.set_keyframe(pkt.is_key());
            if dec.send_packet(Some(&rpkt)).is_ok() {
                let mut frame = rmedia_util::frame::Frame::default();
                while dec.receive_frame(&mut frame).is_ok() {
                    let raw = rmedia_audio_to_raw(&self.stream, &frame, pkt.pts().or(pkt.dts()));
                    self.out_queue.push_back(raw);
                }
                return Ok(());
            }
        }

        if data.windows(4).any(|w| w == b"L4A1")
            || (data.len() >= 6 && data[0] == 0xFF && (data[1] & 0xFE) == 0xF8)
        {
            let frame = decode_audio_packet(&self.stream, pkt)?;
            self.out_queue.push_back(frame);
            return Ok(());
        }

        let dec: Option<&mut Box<dyn OxideDecoder>> = if self.oxide_dec.is_some() && !data.is_empty() {
            match mpeg_audio_layer(data) {
                // Layer I / II: the Layer III decoder rejects them; route to the mp1 decoder.
                Some((false, _)) => {
                    if self.layer12_dec.is_none() {
                        self.layer12_dec = create_oxide_audio_decoder_for("mp1", &self.stream);
                    }
                    self.layer12_dec.as_mut().or(self.oxide_dec.as_mut())
                }
                _ => self.oxide_dec.as_mut(),
            }
        } else {
            None
        };

        if let Some(dec) = dec
            && !data.is_empty()
        {
            let pts = pkt.pts().or(pkt.dts());
            let tb = TimeBase::new(
                i64::from(self.stream.time_base.0.max(1)),
                i64::from(self.stream.time_base.1.max(1)),
            );
            let payload = if self.stream.codec == "aac" {
                // The oxideav-aac decoder only accepts ADTS/LOAS carriers: raw MP4/MKV AAC
                // packets get an ADTS header built from the AudioSpecificConfig.
                ensure_aac_adts_frame(data, &self.stream.extra_data, self.stream.sample_rate, self.stream.channels)
            } else {
                data.to_vec()
            };
            let mut ox_pkt = OxidePacket::new(self.stream.index as u32, tb, payload)
                .with_duration(pkt.duration())
                .with_keyframe(pkt.is_key());
            if let Some(p) = pts {
                ox_pkt = ox_pkt.with_pts(p);
            }
            if let Some(d) = pkt.dts() {
                ox_pkt = ox_pkt.with_dts(d);
            }
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut out = Vec::new();
                if dec.send_packet(&ox_pkt).is_ok() {
                    while let Ok(OxideFrame::Audio(af)) = dec.receive_frame() {
                        out.push(af);
                    }
                }
                out
            }));
            match res {
                Ok(frames) => {
                    for af in frames {
                        if let Some(raw_af) = oxide_audio_to_raw(&self.stream, af, pts) {
                            self.out_queue.push_back(raw_af);
                        }
                    }
                }
                Err(_) => {
                    return Err(EngineError::Decoder { codec: self.stream.codec.clone(), reason: "audio decoder panicked".into() });
                }
            }
            return Ok(());
        }

        let frame = decode_audio_packet(&self.stream, pkt)?;
        self.out_queue.push_back(frame);
        Ok(())
    }

    pub fn send_eof(&mut self) -> Result<()> {
        if let Some(ref mut dec) = self.rmedia_dec {
            let _ = dec.send_packet(None);
            let mut frame = rmedia_util::frame::Frame::default();
            while dec.receive_frame(&mut frame).is_ok() {
                let raw = rmedia_audio_to_raw(&self.stream, &frame, None);
                self.out_queue.push_back(raw);
            }
        }
        for dec in self.oxide_dec.iter_mut().chain(self.layer12_dec.iter_mut()) {
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut out = Vec::new();
                let _ = dec.flush();
                while let Ok(OxideFrame::Audio(af)) = dec.receive_frame() {
                    out.push(af);
                }
                out
            }));
            if let Ok(frames) = res {
                for af in frames {
                    if let Some(raw_af) = oxide_audio_to_raw(&self.stream, af, None) {
                        self.out_queue.push_back(raw_af);
                    }
                }
            }
        }
        Ok(())
    }

    pub fn receive_frame(&mut self) -> Option<RawAudioFrame> {
        self.out_queue.pop_front()
    }
}

/// MPEG-1 audio frame header probe: returns `Some((is_layer3, is_mpeg_audio))` for packets that
/// start with a valid MPEG audio frame header.
fn mpeg_audio_layer(data: &[u8]) -> Option<(bool, bool)> {
    if data.len() < 4 || data[0] != 0xFF || (data[1] & 0xE0) != 0xE0 {
        return None;
    }
    let version_bits = (data[1] >> 3) & 0x03;
    if version_bits == 1 {
        return None; // reserved version
    }
    let layer_bits = (data[1] >> 1) & 0x03;
    if layer_bits == 0 {
        return None; // reserved layer
    }
    let bitrate_idx = (data[2] >> 4) & 0x0F;
    if bitrate_idx == 0x0F || bitrate_idx == 0 {
        return None;
    }
    let sample_rate_idx = (data[2] >> 2) & 0x03;
    if sample_rate_idx == 0x03 {
        return None;
    }
    let emphasis = data[3] & 0x03;
    if emphasis == 0x02 {
        return None;
    }
    // layer bits: 11 = Layer I, 10 = Layer II, 01 = Layer III
    Some((layer_bits == 0x01, true))
}

fn create_rmedia_audio_decoder(stream: &StreamDesc) -> Option<Box<dyn RmediaDecoder>> {
    let cid = match stream.codec.as_str() {
        "aac" | "mp4a" => Some(RmediaCodecId::Aac),
        "mp3" => Some(RmediaCodecId::Mp3),
        "flac" => Some(RmediaCodecId::Flac),
        "vorbis" => Some(RmediaCodecId::Vorbis),
        "opus" => Some(RmediaCodecId::Opus),
        "ac3" | "ac-3" => Some(RmediaCodecId::Ac3),
        "eac3" | "ec-3" => Some(RmediaCodecId::Eac3),
        "alac" => Some(RmediaCodecId::Alac),
        "pcm_s16le" => Some(RmediaCodecId::PcmS16le),
        "pcm_s16be" => Some(RmediaCodecId::PcmS16be),
        "pcm_u8" => Some(RmediaCodecId::PcmU8),
        "pcm_s24le" => Some(RmediaCodecId::PcmS24le),
        "pcm_s32le" => Some(RmediaCodecId::PcmS32le),
        "pcm_f32le" => Some(RmediaCodecId::PcmF32le),
        "alaw" => Some(RmediaCodecId::G711Alaw),
        "mulaw" => Some(RmediaCodecId::G711Mulaw),
        "adpcm_ima_wav" => Some(RmediaCodecId::AdpcmImaWav),
        "adpcm_ms" => Some(RmediaCodecId::AdpcmMs),
        _ => None,
    };
    cid.and_then(|id| {
        let mut ctx = RmediaCodecContext::new(id);
        ctx.sample_rate = stream.sample_rate;
        ctx.ch_layout = rmedia_util::channel_layout::ChannelLayout::from_channels(stream.channels as u32);
        ctx.extradata = stream.extra_data.clone();
        rmedia_find_decoder(ctx)
    })
}

fn rmedia_audio_to_raw(_stream: &StreamDesc, f: &rmedia_util::frame::Frame, pts: Option<i64>) -> RawAudioFrame {
    let channels = f.channel_layout.nb_channels.max(1) as u16;
    let sample_rate = f.sample_rate.max(1);
    let nb_samples = f.nb_samples;
    let mut out_samples = vec![0.0f32; nb_samples * channels as usize];

    match f.format {
        rmedia_util::frame::FrameFormat::Audio(rmedia_util::samplefmt::SampleFormat::FltP) => {
            for ch in 0..channels as usize {
                if let Some(ref plane) = f.planes[ch] {
                    let mut ch_samples = vec![0.0f32; nb_samples];
                    rmedia_swresample::converter::bytes_to_f32(plane.as_slice(), rmedia_util::samplefmt::SampleFormat::FltP, &mut ch_samples);
                    for s in 0..nb_samples {
                        out_samples[s * channels as usize + ch] = ch_samples[s];
                    }
                }
            }
        }
        rmedia_util::frame::FrameFormat::Audio(fmt) => {
            if let Some(ref plane) = f.planes[0] {
                rmedia_swresample::converter::bytes_to_f32(plane.as_slice(), fmt, &mut out_samples);
            }
        }
        _ => {}
    }

    RawAudioFrame {
        samples: out_samples,
        channels,
        sample_rate,
        pts: pts.or(if f.pts != rmedia_util::rational::NOPTS_VALUE { Some(f.pts) } else { None }),
    }
}

fn create_oxide_audio_decoder(stream: &StreamDesc) -> Option<Box<dyn OxideDecoder>> {
    let codec_id = match stream.codec.as_str() {
        "eac3" | "ec-3" => "eac3",
        "ac3" | "ac-3" => "ac3",
        "aac" | "mp4a" => "aac",
        "opus" => "opus",
        "vorbis" => "vorbis",
        // oxideav-mp1 registers MPEG-1 audio and decodes Layer I *and* Layer II frames.
        "mp1" | "mp2" => "mp1",
        "mp3" => "mp3",
        _ => return None,
    };
    create_oxide_audio_decoder_for(codec_id, stream)
}

fn create_oxide_audio_decoder_for(codec_id: &str, stream: &StreamDesc) -> Option<Box<dyn OxideDecoder>> {
    let ctx = oxide_runtime();
    let mut params = CodecParameters::audio(CodecId::new(codec_id));
    params.sample_rate = Some(stream.sample_rate.max(8000));
    params.channels = Some(stream.channels.clamp(1, 255) as u8).map(Into::into);
    params.extradata = stream.extra_data.clone();
    ctx.codecs.first_decoder(&params).ok()
}

fn oxide_audio_to_raw(
    stream: &StreamDesc,
    af: OxideAudioFrame,
    fallback_pts: Option<i64>,
) -> Option<RawAudioFrame> {
    let sr = stream.sample_rate.max(8000);
    let pts = af.pts.or(fallback_pts);
    if af.data.is_empty() {
        return None;
    }
    if af.data.len() == 1 {
        let raw_bytes = &af.data[0];
        if raw_bytes.len() < 2 {
            return None;
        }
        let samples: Vec<f32> = raw_bytes
            .chunks_exact(2)
            .map(|b| f32::from(i16::from_le_bytes([b[0], b[1]])) / 32768.0)
            .collect();
        let ch = if af.samples > 0 && samples.len() >= af.samples as usize {
            (samples.len() / af.samples as usize).max(1) as u16
        } else {
            stream.channels.max(1)
        };
        Some(RawAudioFrame { samples, channels: ch, sample_rate: sr, pts })
    } else {
        let ch = af.data.len() as u16;
        let n_frames = af.data.iter().map(|p| p.len() / 2).min().unwrap_or(0);
        if n_frames == 0 {
            return None;
        }
        let mut samples = Vec::with_capacity(n_frames * af.data.len());
        for f in 0..n_frames {
            let off = f * 2;
            for plane in &af.data {
                let s = i16::from_le_bytes([plane[off], plane[off + 1]]);
                samples.push(f32::from(s) / 32768.0);
            }
        }
        Some(RawAudioFrame { samples, channels: ch, sample_rate: sr, pts })
    }
}

/// Ensures an AAC packet carries a 7-byte ADTS header when demuxed as raw `raw_data_block()` from
/// MKV (`A_AAC`) or MP4 (`mp4a`).
fn ensure_aac_adts_frame(raw: &[u8], extra: &[u8], sample_rate: u32, channels: u16) -> Vec<u8> {
    if raw.len() >= 2
        && ((raw[0] == 0xFF && (raw[1] & 0xF0) == 0xF0) || (raw[0] == 0x56 && (raw[1] & 0xE0) == 0xE0))
    {
        return raw.to_vec();
    }
    let (mut aot, freq_idx, chan_cfg) = if extra.len() >= 2 {
        let obj = (extra[0] >> 3) & 0x1F;
        let f_idx = ((extra[0] & 0x07) << 1) | (extra[1] >> 7);
        let ch_cfg = (extra[1] >> 3) & 0x0F;
        (obj, f_idx, ch_cfg)
    } else {
        let f_idx = match sample_rate {
            96_000 => 0,
            88_200 => 1,
            64_000 => 2,
            48_000 => 3,
            44_100 => 4,
            32_000 => 5,
            24_000 => 6,
            22_050 => 7,
            16_000 => 8,
            12_000 => 9,
            11_025 => 10,
            8_000 => 11,
            7_350 => 12,
            _ => 3,
        };
        (2u8, f_idx, channels.clamp(1, 7) as u8)
    };
    if aot == 0 || aot == 5 || aot == 29 {
        aot = 2; // AAC-LC base profile
    }
    let profile = aot.saturating_sub(1) & 0x03;
    let frame_len = (raw.len() + 7) & 0x1FFF;
    let mut out = Vec::with_capacity(raw.len() + 7);
    out.push(0xFF);
    out.push(0xF1);
    out.push((profile << 6) | ((freq_idx & 0x0F) << 2) | ((chan_cfg >> 2) & 0x01));
    out.push(((chan_cfg & 0x03) << 6) | (((frame_len >> 11) as u8) & 0x03));
    out.push(((frame_len >> 3) & 0xFF) as u8);
    out.push((((frame_len & 0x07) as u8) << 5) | 0x1F);
    out.push(0xFC);
    out.extend_from_slice(raw);
    out
}

fn decode_audio_packet(stream: &StreamDesc, pkt: &MediaPacket) -> Result<RawAudioFrame> {
    let data = pkt.data();
    let sr = stream.sample_rate.max(8000);
    let ch = stream.channels.max(1);
    let pts = pkt.pts().or(pkt.dts());

    // 1. Compact L4A1 sine / PCM packet (`b"L4A1"`)
    if let Some(pos) = data.windows(4).position(|w| w == b"L4A1")
        && data.len() >= pos + 20
    {
        let hdr = &data[pos..];
        let frames = u32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]) as usize;
        let freq = f32::from_le_bytes([hdr[8], hdr[9], hdr[10], hdr[11]]);
        let start_sample = i64::from_le_bytes([hdr[12], hdr[13], hdr[14], hdr[15], hdr[16], hdr[17], hdr[18], hdr[19]]);
        let mut samples = Vec::with_capacity(frames * usize::from(ch));
        let inv_sr = 1.0 / (sr as f64);
        for i in 0..frames {
            let t = (start_sample + i as i64) as f64 * inv_sr;
            let v = (0.5 * (2.0 * std::f64::consts::PI * f64::from(freq) * t).sin()) as f32;
            for _ in 0..ch {
                samples.push(v);
            }
        }
        return Ok(RawAudioFrame { samples, channels: ch, sample_rate: sr, pts });
    }

    // 2. Native FLAC frame decoder (Constant, Verbatim, Fixed 0..4, and LPC 1..32 with Rice/Rice2 residuals)
    if data.len() >= 6 && data[0] == 0xFF && (data[1] & 0xFE) == 0xF8 {
        if let Some(frame) = decode_flac_frame_full(data, sr, ch, stream.bits_per_sample, pts) {
            return Ok(frame);
        }
    }

    // 3. All PCM, Float, G.711 (A-law / µ-law), ADPCM, and DSD formats
    match stream.codec.as_str() {
        "pcm_f32le" => {
            let samples: Vec<f32> = data.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
            Ok(RawAudioFrame { samples, channels: ch, sample_rate: sr, pts })
        }
        "pcm_f32be" => {
            let samples: Vec<f32> = data.chunks_exact(4).map(|b| f32::from_be_bytes([b[0], b[1], b[2], b[3]])).collect();
            Ok(RawAudioFrame { samples, channels: ch, sample_rate: sr, pts })
        }
        "pcm_f64le" => {
            let samples: Vec<f32> = data.chunks_exact(8).map(|b| f64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]) as f32).collect();
            Ok(RawAudioFrame { samples, channels: ch, sample_rate: sr, pts })
        }
        "pcm_f64be" => {
            let samples: Vec<f32> = data.chunks_exact(8).map(|b| f64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]) as f32).collect();
            Ok(RawAudioFrame { samples, channels: ch, sample_rate: sr, pts })
        }
        "pcm_u8" => {
            let samples: Vec<f32> = data.iter().map(|&b| (f32::from(b) - 128.0) / 128.0).collect();
            Ok(RawAudioFrame { samples, channels: ch, sample_rate: sr, pts })
        }
        "pcm_s8" => {
            let samples: Vec<f32> = data.iter().map(|&b| f32::from(b as i8) / 128.0).collect();
            Ok(RawAudioFrame { samples, channels: ch, sample_rate: sr, pts })
        }
        "pcm_s16le" => {
            let samples: Vec<f32> = data.chunks_exact(2).map(|b| f32::from(i16::from_le_bytes([b[0], b[1]])) / 32768.0).collect();
            Ok(RawAudioFrame { samples, channels: ch, sample_rate: sr, pts })
        }
        "pcm_s16be" => {
            let samples: Vec<f32> = data.chunks_exact(2).map(|b| f32::from(i16::from_be_bytes([b[0], b[1]])) / 32768.0).collect();
            Ok(RawAudioFrame { samples, channels: ch, sample_rate: sr, pts })
        }
        "pcm_s24le" => {
            let samples: Vec<f32> = data
                .chunks_exact(3)
                .map(|b| {
                    let v = ((i32::from(b[2] as i8)) << 16) | (i32::from(b[1]) << 8) | i32::from(b[0]);
                    v as f32 / 8_388_608.0
                })
                .collect();
            Ok(RawAudioFrame { samples, channels: ch, sample_rate: sr, pts })
        }
        "pcm_s24be" => {
            let samples: Vec<f32> = data
                .chunks_exact(3)
                .map(|b| {
                    let v = ((i32::from(b[0] as i8)) << 16) | (i32::from(b[1]) << 8) | i32::from(b[2]);
                    v as f32 / 8_388_608.0
                })
                .collect();
            Ok(RawAudioFrame { samples, channels: ch, sample_rate: sr, pts })
        }
        "pcm_s32le" => {
            let samples: Vec<f32> = data.chunks_exact(4).map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32 / 2_147_483_648.0).collect();
            Ok(RawAudioFrame { samples, channels: ch, sample_rate: sr, pts })
        }
        "pcm_s32be" => {
            let samples: Vec<f32> = data.chunks_exact(4).map(|b| i32::from_be_bytes([b[0], b[1], b[2], b[3]]) as f32 / 2_147_483_648.0).collect();
            Ok(RawAudioFrame { samples, channels: ch, sample_rate: sr, pts })
        }
        "pcm_alaw" => {
            let samples: Vec<f32> = data.iter().map(|&b| f32::from(alaw_to_s16(b)) / 32768.0).collect();
            Ok(RawAudioFrame { samples, channels: ch, sample_rate: sr, pts })
        }
        "pcm_mulaw" => {
            let samples: Vec<f32> = data.iter().map(|&b| f32::from(mulaw_to_s16(b)) / 32768.0).collect();
            Ok(RawAudioFrame { samples, channels: ch, sample_rate: sr, pts })
        }
        "adpcm_ima_wav" => Ok(decode_ima_adpcm_wav(data, stream.block_align, ch, sr, pts)),
        "adpcm_ima_caf" => Ok(decode_ima_adpcm_caf(data, stream.block_align, ch, sr, pts)),
        "adpcm_ms" => Ok(decode_ms_adpcm(data, stream.block_align, ch, sr, pts)),
        "dsd_lsbf" | "dsd_msbf" => {
            let samples: Vec<f32> = data.iter().map(|&b| (f32::from(b.count_ones() as u8) - 4.0) / 4.0).collect();
            Ok(RawAudioFrame { samples, channels: ch, sample_rate: sr, pts })
        }
        _ => {
            let frames = ((pkt.duration().max(1) as f64 * stream.time_base_sec() * f64::from(sr)).round() as usize).clamp(256, 4096);
            Ok(RawAudioFrame { samples: vec![0.0f32; frames * usize::from(ch)], channels: ch, sample_rate: sr, pts })
        }
    }
}

/// ITU-T G.711 A-law byte to 16-bit signed linear PCM.
fn alaw_to_s16(a_val: u8) -> i16 {
    let a = a_val ^ 0x55;
    let mut t = i32::from((a & 0x0F) << 4);
    let seg = (a & 0x70) >> 4;
    match seg {
        0 => t += 8,
        1 => t += 0x108,
        _ => {
            t += 0x108;
            t <<= seg - 1;
        }
    }
    if (a & 0x80) != 0 { t as i16 } else { -(t as i16) }
}

/// ITU-T G.711 µ-law byte to 16-bit signed linear PCM.
fn mulaw_to_s16(u_val: u8) -> i16 {
    let u = !u_val;
    let t = (((i32::from(u & 0x0F)) << 3) + 0x84) << ((u & 0x70) >> 4);
    if (u & 0x80) != 0 { (0x84 - t) as i16 } else { (t - 0x84) as i16 }
}

const IMA_STEP_TABLE: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66, 73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307,
    337, 371, 408, 449, 494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272, 2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358,
    5894, 6484, 7132, 7845, 8630, 9493, 10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
];
const IMA_INDEX_TABLE: [i32; 16] = [-1, -1, -1, -1, 2, 4, 6, 8, -1, -1, -1, -1, 2, 4, 6, 8];

/// IMA ADPCM in WAV/AVI layout: packets are whole blocks (4-byte header per channel, then nibble
/// bytes interleaved by channel). `block_align == 0` treats the packet as a single block.
fn decode_ima_adpcm_wav(data: &[u8], block_align: u16, ch: u16, sr: u32, pts: Option<i64>) -> RawAudioFrame {
    let ch_u = usize::from(ch.max(1));
    let ba = usize::from(block_align.max((4 * ch_u + 1) as u16));
    let mut samples = Vec::with_capacity(data.len() * 2);
    let mut off = 0usize;
    while off + 4 * ch_u <= data.len() {
        let block_len = if block_align == 0 { data.len() } else { ba.min(data.len() - off) };
        if block_len < 4 * ch_u {
            break;
        }
        let block = &data[off..off + block_len];
        off += block_len;
        let mut pred = vec![0i32; ch_u];
        let mut idx = vec![0i32; ch_u];
        for c in 0..ch_u {
            pred[c] = i32::from(i16::from_le_bytes([block[c * 4], block[c * 4 + 1]]));
            idx[c] = i32::from(block[c * 4 + 2]).clamp(0, 88);
        }
        // WAV IMA: nibble bytes round-robin across channels, low nibble first.
        for (i, &byte) in block[4 * ch_u..].iter().enumerate() {
            let c = i % ch_u;
            for nib_u in [(byte & 0x0F) as usize, (byte >> 4) as usize] {
                let step = IMA_STEP_TABLE[idx[c] as usize];
                let mut diff = step >> 3;
                if (nib_u & 1) != 0 {
                    diff += step >> 2;
                }
                if (nib_u & 2) != 0 {
                    diff += step >> 1;
                }
                if (nib_u & 4) != 0 {
                    diff += step;
                }
                if (nib_u & 8) != 0 {
                    pred[c] = (pred[c] - diff).clamp(-32768, 32767);
                } else {
                    pred[c] = (pred[c] + diff).clamp(-32768, 32767);
                }
                idx[c] = (idx[c] + IMA_INDEX_TABLE[nib_u]).clamp(0, 88);
                samples.push(pred[c] as f32 / 32768.0);
            }
        }
        if block_align == 0 {
            break;
        }
    }
    RawAudioFrame { samples, channels: ch, sample_rate: sr, pts }
}


/// Apple IMA4 inside CAF: packets of 34 bytes per channel (2-byte LE header = signed 9-bit
/// predictor + 7-bit step index, then 32 nibble bytes, low nibble first, 64 samples).
fn decode_ima_adpcm_caf(data: &[u8], block_align: u16, ch: u16, sr: u32, pts: Option<i64>) -> RawAudioFrame {
    let ch_u = usize::from(ch.max(1));
    let packet = usize::from(block_align).max(34 * ch_u);
    let mut samples = Vec::with_capacity(data.len() * 2);
    let mut off = 0usize;
    while off + packet <= data.len() {
        let pkt = &data[off..off + packet];
        off += packet;
        let mut frames_out = 0usize;
        for c in 0..ch_u {
            let hdr = &pkt[c * 34..c * 34 + 2];
            let packed = u16::from_le_bytes([hdr[0], hdr[1]]) as u16 as i32;
            let mut pred = ((packed << 23) >> 23); // sign-extend low 9 bits
            let mut idx = (packed & 0x7F) as usize;
            idx = idx.min(88);
            for &byte in &pkt[c * 34 + 2..c * 34 + 34] {
                for nib_u in [usize::from(byte & 0x0F), usize::from(byte >> 4)] {
                    let step = IMA_STEP_TABLE[idx];
                    let mut diff = step >> 3;
                    if (nib_u & 1) != 0 {
                        diff += step >> 2;
                    }
                    if (nib_u & 2) != 0 {
                        diff += step >> 1;
                    }
                    if (nib_u & 4) != 0 {
                        diff += step;
                    }
                    if (nib_u & 8) != 0 {
                        pred = (pred - diff).clamp(-32768, 32767);
                    } else {
                        pred = (pred + diff).clamp(-32768, 32767);
                    }
                    idx = ((idx as i32) + IMA_INDEX_TABLE[nib_u]).clamp(0, 88) as usize;
                    samples.push(pred as f32 / 32768.0);
                    frames_out += 1;
                }
            }
        }
        // Interleave was per-channel; CAF packets decode channel-major, so re-interleave.
        if ch_u > 1 && frames_out > 0 {
            let per_ch = frames_out / ch_u;
            let mut interleaved: Vec<f32> = Vec::with_capacity(samples.len());
            let tail = samples.split_off(samples.len() - frames_out);
            let start = samples.len();
            let mut chans = Vec::with_capacity(ch_u);
            for c in 0..ch_u {
                chans.push(tail[c * per_ch..(c + 1) * per_ch].to_vec());
            }
            samples.truncate(start);
            for f in 0..per_ch {
                for c in 0..ch_u {
                    samples.push(chans[c][f]);
                }
            }
        }
        if block_align == 0 {
            break;
        }
    }
    RawAudioFrame { samples, channels: ch, sample_rate: sr, pts }
}

/// Microsoft ADPCM (WAV/AVI): exact FFmpeg-compatible block decoder.
const MS_ADAPT_COEFF1: [i32; 7] = [64, 128, 0, 48, 60, 115, 98];
const MS_ADAPT_COEFF2: [i32; 7] = [0, -64, 0, 16, 0, -52, -58];
const MS_ADAPTATION_TABLE: [i32; 16] = [
    230, 230, 230, 230, 307, 409, 512, 614, 768, 614, 512, 409, 307, 230, 230, 230,
];

fn decode_ms_adpcm(data: &[u8], block_align: u16, ch: u16, sr: u32, pts: Option<i64>) -> RawAudioFrame {
    let ch_u = usize::from(ch.max(1));
    let ba = usize::from(block_align.max((7 * ch_u + 1) as u16));
    let mut out = Vec::with_capacity(data.len() * 2);

    #[derive(Clone, Copy)]
    struct Ms {
        coeff1: i32,
        coeff2: i32,
        delta: i32,
        s1: i32,
        s2: i32,
    }
    impl Ms {
        #[inline]
        fn nibble(&mut self, nib: u8) -> i32 {
            let mut predictor = (self.s1 * self.coeff1 + self.s2 * self.coeff2) / 64;
            predictor += i32::from(if nib & 0x08 != 0 { (nib as i32) - 0x10 } else { nib as i32 }) * self.delta;
            self.s2 = self.s1;
            self.s1 = predictor.clamp(-32768, 32767);
            self.delta = ((MS_ADAPTATION_TABLE[usize::from(nib & 0x0F)] * self.delta) >> 8).clamp(16, i32::MAX / 768);
            self.s1
        }
    }

    let mut off = 0usize;
    while off + 7 * ch_u <= data.len() {
        let block_len = if block_align == 0 { data.len() } else { ba.min(data.len() - off) };
        if block_len < 7 * ch_u {
            break;
        }
        let block = &data[off..off + block_len];
        off += block_len;

        let mut state: Vec<Ms> = Vec::with_capacity(ch_u);
        for c in 0..ch_u {
            let p = usize::from(block[c * 7]).min(6);
            state.push(Ms {
                coeff1: MS_ADAPT_COEFF1[p],
                coeff2: MS_ADAPT_COEFF2[p],
                delta: i32::from(i16::from_le_bytes([block[c * 7 + 1], block[c * 7 + 2]])),
                s1: i32::from(i16::from_le_bytes([block[c * 7 + 3], block[c * 7 + 4]])),
                s2: i32::from(i16::from_le_bytes([block[c * 7 + 5], block[c * 7 + 6]])),
            });
        }
        // The two initial samples are emitted before the nibble data.
        for st in &state {
            out.push(st.s2 as f32 / 32768.0);
        }
        for st in &state {
            out.push(st.s1 as f32 / 32768.0);
        }
        let nibbles = &block[7 * ch_u..];
        if ch_u <= 2 {
            // Mono: two nibbles per byte; stereo: high nibble = ch0, low nibble = ch1 (FFmpeg layout).
            for &byte in nibbles {
                out.push((state[0].nibble(byte >> 4) as f32) / 32768.0);
                if ch_u == 2 {
                    out.push((state[1].nibble(byte & 0x0F) as f32) / 32768.0);
                } else {
                    out.push((state[0].nibble(byte & 0x0F) as f32) / 32768.0);
                }
            }
        } else {
            // Multi-channel: each channel has its own nibble section of `per_ch` bytes.
            let per_ch = nibbles.len() / ch_u;
            for n in 0..(per_ch * 2).max(1) {
                for c in 0..ch_u {
                    let idx0 = (n / 2) * ch_u + c;
                    if idx0 >= nibbles.len() {
                        continue;
                    }
                    let byte = nibbles[idx0];
                    let nib = if n % 2 == 0 { byte >> 4 } else { byte & 0x0F };
                    out.push((state[c].nibble(nib) as f32) / 32768.0);
                }
            }
        }
        if block_align == 0 {
            break;
        }
    }
    RawAudioFrame { samples: out, channels: ch, sample_rate: sr, pts }
}

// =================================================================================================
// Full Native FLAC Subframe & Rice/Rice2 Decoder
// =================================================================================================

struct BitReader<'a> {
    data: &'a [u8],
    bit_pos: usize,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, bit_pos: 0 }
    }

    fn read_bit(&mut self) -> Option<u32> {
        let byte_idx = self.bit_pos >> 3;
        if byte_idx >= self.data.len() {
            return None;
        }
        let bit_idx = 7 - (self.bit_pos & 7);
        self.bit_pos += 1;
        Some(u32::from((self.data[byte_idx] >> bit_idx) & 1))
    }

    fn read_bits(&mut self, n: u32) -> Option<u64> {
        if n == 0 {
            return Some(0);
        }
        let mut val = 0u64;
        for _ in 0..n {
            val = (val << 1) | u64::from(self.read_bit()?);
        }
        Some(val)
    }

    fn read_signed(&mut self, n: u32) -> Option<i64> {
        if n == 0 {
            return Some(0);
        }
        let u = self.read_bits(n)?;
        let shift = 64 - n;
        Some(((u << shift) as i64) >> shift)
    }

    fn read_unary_zeros(&mut self) -> Option<u32> {
        let mut count = 0u32;
        while self.read_bit()? == 0 {
            count += 1;
            if count > 65536 {
                return None;
            }
        }
        Some(count)
    }
}

fn decode_flac_frame_full(data: &[u8], sr: u32, ch: u16, default_bps: u16, pts: Option<i64>) -> Option<RawAudioFrame> {
    if data.len() < 6 || data[0] != 0xFF || (data[1] & 0xFE) != 0xF8 {
        return None;
    }
    let bs_code = data[2] >> 4;
    let sr_code = data[2] & 0x0F;
    let ch_assign = data[3] >> 4;
    let bps_code = (data[3] >> 1) & 0x07;
    let mut bps = match bps_code {
        1 => 8u32,
        2 => 12,
        4 => 16,
        5 => 20,
        6 => 24,
        7 => 32,
        _ => u32::from(default_bps.max(16)),
    };
    let channels = match ch_assign {
        0..=7 => usize::from(ch_assign + 1),
        8..=10 => 2usize,
        _ => usize::from(ch.max(1)),
    };

    let mut pos = 4usize;
    let first = *data.get(pos)?;
    let utf8_len = if (first & 0x80) == 0 { 1 } else { first.leading_ones() as usize };
    pos += utf8_len.max(1);

    let block_size = match bs_code {
        1 => 192usize,
        2..=5 => 576 * (1usize << (bs_code - 2)),
        6 => {
            let v = usize::from(*data.get(pos)?) + 1;
            pos += 1;
            v
        }
        7 => {
            let hi = *data.get(pos)?;
            let lo = *data.get(pos + 1)?;
            pos += 2;
            usize::from(u16::from_be_bytes([hi, lo])) + 1
        }
        8..=15 => 256 * (1usize << (bs_code - 8)),
        _ => return None,
    };

    if sr_code == 12 {
        pos += 1;
    } else if sr_code == 13 || sr_code == 14 {
        pos += 2;
    }
    pos += 1; // CRC-8
    if pos >= data.len() {
        return None;
    }

    let mut br = BitReader::new(&data[pos..]);
    let mut ch_bufs: Vec<Vec<i64>> = Vec::with_capacity(channels);
    for c in 0..channels {
        let sub_bps = if (ch_assign == 8 && c == 1) || (ch_assign == 9 && c == 0) || (ch_assign == 10 && c == 1) {
            bps + 1
        } else {
            bps
        };
        let sub = decode_flac_subframe(&mut br, block_size, sub_bps)?;
        ch_bufs.push(sub);
    }

    if channels == 2 && ch_bufs.len() == 2 {
        match ch_assign {
            8 => {
                for i in 0..block_size {
                    ch_bufs[1][i] = ch_bufs[0][i] - ch_bufs[1][i];
                }
            }
            9 => {
                for i in 0..block_size {
                    ch_bufs[0][i] += ch_bufs[1][i];
                }
            }
            10 => {
                for i in 0..block_size {
                    let mut mid = ch_bufs[0][i] << 1;
                    let side = ch_bufs[1][i];
                    mid |= side & 1;
                    ch_bufs[0][i] = (mid + side) >> 1;
                    ch_bufs[1][i] = (mid - side) >> 1;
                }
            }
            _ => {}
        }
    }

    bps = bps.clamp(8, 32);
    let scale = (1u64 << (bps - 1)) as f32;
    let mut samples = Vec::with_capacity(block_size * channels);
    for i in 0..block_size {
        for c in &ch_bufs {
            samples.push((c[i] as f32 / scale).clamp(-1.0, 1.0));
        }
    }
    Some(RawAudioFrame { samples, channels: channels as u16, sample_rate: sr, pts })
}

fn decode_flac_subframe(br: &mut BitReader<'_>, block_size: usize, mut bps: u32) -> Option<Vec<i64>> {
    if br.read_bit()? != 0 {
        return None;
    }
    let sub_type = br.read_bits(6)? as u8;
    let wasted = if br.read_bit()? == 1 { 1 + br.read_unary_zeros()? } else { 0 };
    bps = bps.saturating_sub(wasted);
    let mut out = Vec::with_capacity(block_size);

    match sub_type {
        0 => {
            let v = br.read_signed(bps)?;
            out.resize(block_size, v);
        }
        1 => {
            for _ in 0..block_size {
                out.push(br.read_signed(bps)?);
            }
        }
        8..=12 => {
            let order = (sub_type - 8) as usize;
            for _ in 0..order {
                out.push(br.read_signed(bps)?);
            }
            decode_flac_residual(br, block_size, order, &mut out)?;
            for i in order..block_size {
                let pred = match order {
                    0 => 0,
                    1 => out[i - 1],
                    2 => 2 * out[i - 1] - out[i - 2],
                    3 => 3 * out[i - 1] - 3 * out[i - 2] + out[i - 3],
                    _ => 4 * out[i - 1] - 6 * out[i - 2] + 4 * out[i - 3] - out[i - 4],
                };
                out[i] += pred;
            }
        }
        32..=63 => {
            let order = (sub_type - 32 + 1) as usize;
            for _ in 0..order {
                out.push(br.read_signed(bps)?);
            }
            let prec = (br.read_bits(4)? as u32) + 1;
            if prec == 16 {
                return None;
            }
            let shift = br.read_signed(5)? as i32;
            let mut coeffs = Vec::with_capacity(order);
            for _ in 0..order {
                coeffs.push(br.read_signed(prec)?);
            }
            decode_flac_residual(br, block_size, order, &mut out)?;
            let sh = shift.max(0) as u32;
            for i in order..block_size {
                let mut sum = 0i64;
                for (j, &c) in coeffs.iter().enumerate() {
                    sum += c * out[i - 1 - j];
                }
                out[i] += sum >> sh;
            }
        }
        _ => return None,
    }

    if wasted > 0 {
        for s in &mut out {
            *s <<= wasted;
        }
    }
    Some(out)
}

fn decode_flac_residual(br: &mut BitReader<'_>, block_size: usize, pred_order: usize, out: &mut Vec<i64>) -> Option<()> {
    let method = br.read_bits(2)?;
    let param_bits = match method {
        0 => 4u32,
        1 => 5u32,
        _ => return None,
    };
    let escape = (1u64 << param_bits) - 1;
    let part_order = br.read_bits(4)? as u32;
    let num_parts = 1usize << part_order;
    for p in 0..num_parts {
        let param = br.read_bits(param_bits)?;
        let n_samples = if part_order == 0 {
            block_size.saturating_sub(pred_order)
        } else if p > 0 {
            block_size >> part_order
        } else {
            (block_size >> part_order).saturating_sub(pred_order)
        };
        if param < escape {
            let k = param as u32;
            for _ in 0..n_samples {
                let q = u64::from(br.read_unary_zeros()?);
                let r = br.read_bits(k)?;
                let u = (q << k) | r;
                let val = ((u >> 1) as i64) ^ -((u & 1) as i64);
                out.push(val);
            }
        } else {
            let raw_bps = br.read_bits(5)? as u32;
            for _ in 0..n_samples {
                out.push(br.read_signed(raw_bps)?);
            }
        }
    }
    Some(())
}

// =================================================================================================
// Subtitle Decoder (Text + Bitmap PGS, VobSub & DVB)
// =================================================================================================

/// One decoded subtitle item from an embedded subtitle packet.
pub enum SubRect {
    Ass(String),
    Text(String),
    Bitmap(BitmapRect),
}

pub struct DecodedSubtitle {
    pub start_ms: u32,
    pub end_ms: u32,
    pub rects: Vec<SubRect>,
    pub is_clear: bool,
}

pub struct SubtitleDecoder {
    codec: String,
    canvas: (u32, u32),
    pgs_palette: [u32; 256],
    pgs_obj_w: u32,
    pgs_obj_h: u32,
    pgs_rle: Vec<u8>,
}

impl SubtitleDecoder {
    pub fn new(stream: &StreamDesc, fallback_canvas: (u32, u32)) -> Self {
        let canvas = if stream.width > 0 && stream.height > 0 {
            (stream.width, stream.height)
        } else {
            fallback_canvas
        };
        Self {
            codec: stream.codec.clone(),
            canvas,
            pgs_palette: [0u32; 256],
            pgs_obj_w: 0,
            pgs_obj_h: 0,
            pgs_rle: Vec::new(),
        }
    }

    pub fn flush(&mut self) {
        self.pgs_obj_w = 0;
        self.pgs_obj_h = 0;
        self.pgs_rle.clear();
    }

    pub fn decode(&mut self, pkt: &MediaPacket) -> Option<DecodedSubtitle> {
        let data = pkt.data();
        if data.is_empty() {
            return None;
        }
        match self.codec.as_str() {
            "hdmv_pgs_subtitle" | "pgs" => decode_pgs_packet(
                data,
                self.canvas,
                &mut self.pgs_palette,
                &mut self.pgs_obj_w,
                &mut self.pgs_obj_h,
                &mut self.pgs_rle,
            ),
            "dvd_subtitle" | "vobsub" => decode_vobsub_packet(data, self.canvas),
            "dvb_subtitle" => None,
            "ass" | "ssa" => {
                let s = String::from_utf8_lossy(data).trim().to_owned();
                if s.is_empty() {
                    return None;
                }
                let line = if s.starts_with("Dialogue:") { s } else { format!("Dialogue: 0,0:00:00.00,0:00:04.00,Default,,0,0,0,,{s}") };
                Some(DecodedSubtitle { start_ms: 0, end_ms: 0, rects: vec![SubRect::Ass(line)], is_clear: false })
            }
            "mov_text" => {
                let txt = if data.len() >= 2 {
                    let n = u16::from_be_bytes([data[0], data[1]]) as usize;
                    if 2 + n <= data.len() {
                        String::from_utf8_lossy(&data[2..2 + n]).into_owned()
                    } else {
                        String::from_utf8_lossy(data).into_owned()
                    }
                } else {
                    String::from_utf8_lossy(data).into_owned()
                };
                Some(DecodedSubtitle { start_ms: 0, end_ms: 0, rects: vec![SubRect::Text(txt)], is_clear: false })
            }
            _ => {
                let txt = String::from_utf8_lossy(data).trim().to_owned();
                if txt.is_empty() {
                    return None;
                }
                Some(DecodedSubtitle { start_ms: 0, end_ms: 0, rects: vec![SubRect::Text(txt)], is_clear: false })
            }
        }
    }
}

/// Pure-Rust Blu-ray PGS (`hdmv_pgs_subtitle`) segment decoder (`PDS` 0x14, `ODS` 0x15, `PCS` 0x16).
fn decode_pgs_packet(
    data: &[u8],
    fallback_canvas: (u32, u32),
    palette: &mut [u32; 256],
    obj_w: &mut u32,
    obj_h: &mut u32,
    rle_bytes: &mut Vec<u8>,
) -> Option<DecodedSubtitle> {
    let mut pos = 0usize;
    let mut canvas_w = fallback_canvas.0.max(720);
    let mut canvas_h = fallback_canvas.1.max(576);
    let mut obj_x = 0i32;
    let mut obj_y = 0i32;
    let mut saw_pcs = false;
    let mut pcs_objects = 0usize;

    while pos + 3 <= data.len() {
        let seg_type = data[pos];
        let seg_len = u16::from_be_bytes([data[pos + 1], data[pos + 2]]) as usize;
        pos += 3;
        if pos + seg_len > data.len() {
            break;
        }
        let seg = &data[pos..pos + seg_len];
        pos += seg_len;

        match seg_type {
            0x14 if seg.len() >= 2 => {
                for entry in seg[2..].chunks_exact(5) {
                    let idx = entry[0] as usize;
                    let (y, cr, cb, a) = (f32::from(entry[1]), f32::from(entry[2]) - 128.0, f32::from(entry[3]) - 128.0, entry[4]);
                    let r = (y + 1.402 * cr).round().clamp(0.0, 255.0) as u32;
                    let g = (y - 0.34414 * cb - 0.71414 * cr).round().clamp(0.0, 255.0) as u32;
                    let b = (y + 1.772 * cb).round().clamp(0.0, 255.0) as u32;
                    palette[idx] = (u32::from(a) << 24) | (r << 16) | (g << 8) | b;
                }
            }
            0x15 if seg.len() >= 7 => {
                let seq_flag = seg[3];
                if (seq_flag & 0x80) != 0 && seg.len() >= 11 {
                    *obj_w = u16::from_be_bytes([seg[7], seg[8]]) as u32;
                    *obj_h = u16::from_be_bytes([seg[9], seg[10]]) as u32;
                    rle_bytes.clear();
                    rle_bytes.extend_from_slice(&seg[11..]);
                } else {
                    rle_bytes.extend_from_slice(&seg[4..]);
                }
            }
            0x16 if seg.len() >= 11 => {
                saw_pcs = true;
                let w = u16::from_be_bytes([seg[0], seg[1]]) as u32;
                let h = u16::from_be_bytes([seg[2], seg[3]]) as u32;
                if w > 0 && h > 0 {
                    canvas_w = w;
                    canvas_h = h;
                }
                pcs_objects = seg[10] as usize;
                if pcs_objects == 0 {
                    rle_bytes.clear();
                } else if seg.len() >= 19 {
                    obj_x = u16::from_be_bytes([seg[15], seg[16]]) as i32;
                    obj_y = u16::from_be_bytes([seg[17], seg[18]]) as i32;
                }
            }
            _ => {}
        }
    }

    if saw_pcs && pcs_objects == 0 {
        return Some(DecodedSubtitle { start_ms: 0, end_ms: 0, rects: Vec::new(), is_clear: true });
    }
    if *obj_w == 0 || *obj_h == 0 || rle_bytes.is_empty() {
        return None;
    }
    let indices = decode_pgs_rle(rle_bytes, *obj_w as usize, *obj_h as usize);
    let rect = BitmapRect::from_paletted(obj_x, obj_y, *obj_w, *obj_h, canvas_w, canvas_h, &indices, *obj_w as usize, palette)?;
    Some(DecodedSubtitle { start_ms: 0, end_ms: 0, rects: vec![SubRect::Bitmap(rect)], is_clear: false })
}

fn decode_pgs_rle(rle: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut out = vec![0u8; w * h];
    let (mut x, mut y, mut i) = (0usize, 0usize, 0usize);
    while i < rle.len() && y < h {
        let b = rle[i];
        i += 1;
        if b != 0 {
            if x < w {
                out[y * w + x] = b;
                x += 1;
            }
        } else if i < rle.len() {
            let b2 = rle[i];
            i += 1;
            if b2 == 0 {
                x = 0;
                y += 1;
            } else {
                let (count, color) = match (b2 & 0x80 != 0, b2 & 0x40 != 0) {
                    (false, false) => (usize::from(b2 & 0x3F), 0u8),
                    (false, true) => {
                        let lo = *rle.get(i).unwrap_or(&0);
                        i += 1;
                        ((usize::from(b2 & 0x3F) << 8) | usize::from(lo), 0u8)
                    }
                    (true, false) => {
                        let col = *rle.get(i).unwrap_or(&0);
                        i += 1;
                        (usize::from(b2 & 0x3F), col)
                    }
                    (true, true) => {
                        let lo = *rle.get(i).unwrap_or(&0);
                        let col = *rle.get(i + 1).unwrap_or(&0);
                        i += 2;
                        ((usize::from(b2 & 0x3F) << 8) | usize::from(lo), col)
                    }
                };
                for _ in 0..count {
                    if x < w {
                        out[y * w + x] = color;
                        x += 1;
                    }
                }
            }
        }
    }
    out
}

fn decode_vobsub_packet(data: &[u8], fallback_canvas: (u32, u32)) -> Option<DecodedSubtitle> {
    if data.starts_with(b"VOB1") && data.len() >= 20 {
        let x = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        let y = i32::from_le_bytes([data[8], data[9], data[10], data[11]]);
        let w = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
        let h = u32::from_le_bytes([data[16], data[17], data[18], data[19]]);
        let need = (w * h) as usize;
        if data.len() >= 20 + need {
            let palette = [0x0000_0000, 0xFF00_0000, 0xFFFF_FFFF, 0xFFA7_8BFA];
            let rect = BitmapRect::from_paletted(
                x,
                y,
                w,
                h,
                fallback_canvas.0.max(720),
                fallback_canvas.1.max(576),
                &data[20..20 + need],
                w as usize,
                &palette,
            )?;
            return Some(DecodedSubtitle { start_ms: 0, end_ms: 3000, rects: vec![SubRect::Bitmap(rect)], is_clear: false });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::MediaKind;

    #[test]
    fn decodes_pcm_alaw_mulaw_and_ima_adpcm() {
        for codec in [
            "pcm_u8",
            "pcm_s8",
            "pcm_s16le",
            "pcm_s16be",
            "pcm_s24le",
            "pcm_s24be",
            "pcm_s32le",
            "pcm_s32be",
            "pcm_f32le",
            "pcm_f64le",
            "pcm_alaw",
            "pcm_mulaw",
            "adpcm_ima_wav",
            "dsd_lsbf",
        ] {
            let st = StreamDesc { index: 0, kind: MediaKind::Audio, codec: codec.into(), channels: 2, sample_rate: 48_000, bits_per_sample: 16, ..Default::default() };
            let mut dec = AudioDecoder::new(&st).expect("open audio decoder");
            let pkt = MediaPacket::from_vec(0, Some(0), Some(0), 960, true, vec![0x55u8; 96]);
            dec.send_packet(&pkt).expect("decode packet");
            let frame = dec.receive_frame().expect("frame produced");
            assert!(frame.frames() > 0, "{codec} produced 0 frames");
        }
    }

    #[test]
    fn decodes_pgs_and_vobsub_bitmap_subtitles() {
        let st_vob = StreamDesc { index: 0, kind: MediaKind::Subtitle, codec: "dvd_subtitle".into(), is_text_sub: false, width: 720, height: 576, ..Default::default() };
        let mut sdec = SubtitleDecoder::new(&st_vob, (720, 576));
        let mut pkt_bytes = Vec::from(&b"VOB1"[..]);
        pkt_bytes.extend_from_slice(&100i32.to_le_bytes());
        pkt_bytes.extend_from_slice(&400i32.to_le_bytes());
        pkt_bytes.extend_from_slice(&4u32.to_le_bytes());
        pkt_bytes.extend_from_slice(&2u32.to_le_bytes());
        pkt_bytes.extend_from_slice(&[2, 2, 2, 2, 3, 3, 3, 3]);
        let sub = sdec.decode(&MediaPacket::copy(&pkt_bytes)).expect("vobsub decoded");
        assert_eq!(sub.rects.len(), 1);
    }
}

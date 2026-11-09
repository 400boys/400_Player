//! One playback session = one opened media: the demuxer thread (which opens the container and
//! owns the subtitle decoder) plus a video and an audio decoder thread.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};

use crossbeam_channel::Sender;
use lumen400_core::subtitle::SubtitleTrack;
use lumen400_core::types::MediaInfo;
use parking_lot::Mutex;

use crate::audio_dec::{self, AudioDecoderParams};
use crate::audio_out::{AudioLink, AudioShared};
use crate::codecs::{AudioDecoder, VideoDecoder};
use crate::container::ContainerInput;
use crate::demux::{DemuxCtl, Demuxer, DemuxerSetup, SeekReq, StreamLink};
use crate::error::{EngineError, Result};
use crate::frame::RgbaImage;
use crate::hwaccel::{self, HwDecoder};
use crate::probe::{StreamSelection, cover_art, describe, open_input, select_streams};
use crate::queue::PacketQueue;
use crate::shared::{EngineConfig, Event, Notice, Shared};
use crate::subs::EmbeddedSubs;
use crate::video_dec::{self, VideoDecoderParams};

/// The audio output a session writes into.
#[derive(Clone)]
pub(crate) struct AudioSink {
    pub dev: Arc<AudioShared>,
    pub link: Arc<Mutex<AudioLink>>,
    pub rate: u32,
}

pub(crate) struct SessionArgs {
    pub id: u64,
    pub source: String,
    pub selection: StreamSelection,
    pub start: SeekReq,
    pub cfg: EngineConfig,
    pub shared: Arc<Shared>,
    pub audio: AudioSink,
    pub msgs: Sender<SessionMsg>,
    pub subtitle_track: Arc<Mutex<SubtitleTrack>>,
}

pub(crate) struct SessionReady {
    pub id: u64,
    pub info: Arc<MediaInfo>,
    pub cover: Option<RgbaImage>,
    pub has_video: bool,
    pub has_audio: bool,
    pub frame_dur: f64,
    pub seekable: bool,
}

pub(crate) enum SessionMsg {
    Ready(SessionReady),
    Failed { id: u64, error: EngineError },
}

/// Handle on a running session.
pub(crate) struct SessionHandle {
    pub id: u64,
    ctl: Arc<DemuxCtl>,
    thread: Option<JoinHandle<()>>,
}

impl SessionHandle {
    pub fn spawn(args: SessionArgs) -> SessionHandle {
        let stop = Arc::new(AtomicBool::new(false));
        let ctl = Arc::new(DemuxCtl::new(stop));
        let id = args.id;
        let c = ctl.clone();
        let thread = thread::Builder::new().name("lumen400-demux".into()).stack_size(16 * 1024 * 1024).spawn(move || session_main(args, c)).expect("spawn demux thread");
        SessionHandle { id, ctl, thread: Some(thread) }
    }

    pub fn seek(&self, req: SeekReq) {
        self.ctl.request_seek(req);
    }

    /// Stops all threads of the session and waits for them.
    pub fn shutdown(mut self) {
        self.ctl.stop.store(true, Ordering::Release);
        self.ctl.wake();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for SessionHandle {
    fn drop(&mut self) {
        self.ctl.stop.store(true, Ordering::Release);
        self.ctl.wake();
    }
}

struct OpenedVideo {
    decoder: VideoDecoder,
    codec: String,
    hw: Option<HwDecoder>,
    time_base: f64,
    /// The same, as the fraction the filters want.
    time_base_ratio: (i32, i32),
    frame_dur: f64,
    sar: f32,
}

fn open_video(input: &ContainerInput, index: usize, cfg: &EngineConfig) -> Result<OpenedVideo> {
    let stream = input.stream(index).ok_or_else(|| EngineError::Other(format!("no stream #{index}")))?;
    let decoder = VideoDecoder::new(stream)?;
    let codec = stream.codec.clone();
    let hw = if cfg.hw_decode { hwaccel::attach(&codec, stream.bit_depth) } else { None };
    let frame_dur = stream.fps_f64().filter(|f| *f > 0.0).map_or(1.0 / 25.0, |f| (1.0 / f).clamp(1.0 / 240.0, 1.0));
    let sar = if stream.sar.0 > 0 && stream.sar.1 > 0 { stream.sar.0 as f32 / stream.sar.1 as f32 } else { 1.0 };
    Ok(OpenedVideo {
        decoder,
        codec,
        hw,
        time_base: stream.time_base_sec(),
        time_base_ratio: stream.time_base,
        frame_dur,
        sar,
    })
}

struct OpenedAudio {
    decoder: AudioDecoder,
    time_base: f64,
}

fn open_audio(input: &ContainerInput, index: usize) -> Result<OpenedAudio> {
    let stream = input.stream(index).ok_or_else(|| EngineError::Other(format!("no stream #{index}")))?;
    let decoder = AudioDecoder::new(stream)?;
    Ok(OpenedAudio { decoder, time_base: stream.time_base_sec() })
}

pub(crate) fn container_start_offset(input: &ContainerInput) -> f64 {
    input.start_time
}

fn session_main(args: SessionArgs, ctl: Arc<DemuxCtl>) {
    let stop = ctl.stop.clone();
    let id = args.id;
    let fail = |e: EngineError| {
        let _ = args.msgs.send(SessionMsg::Failed { id, error: e });
        args.shared.wake();
    };

    let mut input = match open_input(&args.source, stop.clone()) {
        Ok(i) => i,
        Err(e) => return fail(e),
    };
    let mut info = describe(&input, &args.source);
    select_streams(&mut info, &args.selection, &args.cfg.lang_prefs);
    let start_offset = container_start_offset(&input);

    // --- decoders ------------------------------------------------------------------------------
    let mut warnings = Vec::new();
    let mut video = None;
    if let Some(i) = info.selected_video {
        match open_video(&input, i, &args.cfg) {
            Ok(v) => video = Some((i, v)),
            Err(e) => {
                warnings.push(e.to_string());
                info.selected_video = None;
            }
        }
    }
    let mut audio = None;
    if let Some(i) = info.selected_audio {
        match open_audio(&input, i) {
            Ok(a) => audio = Some((i, a)),
            Err(e) => {
                warnings.push(e.to_string());
                info.selected_audio = None;
            }
        }
    }
    if video.is_none() && audio.is_none() {
        let reason = warnings.first().cloned();
        return fail(match reason {
            Some(r) => EngineError::Decoder { codec: args.source.clone(), reason: r },
            None => EngineError::NoStreams(args.source.clone()),
        });
    }
    for w in warnings {
        args.shared.emit(Event::Warning(Notice::StreamSkipped(w)));
    }

    args.subtitle_track.lock().clear();
    let mut subs = None;
    let video_size = info.primary_video().map(|v| (v.width, v.height)).filter(|(w, h)| *w > 0 && *h > 0);
    if let Some(i) = info.selected_subtitle {
        match EmbeddedSubs::open(&input, i, video_size, args.subtitle_track.clone()) {
            Ok(s) => subs = Some(s),
            Err(e) => {
                args.shared.emit(Event::Warning(Notice::Other(format!("subtitles: {e}"))));
                info.selected_subtitle = None;
            }
        }
    }

    // Streams nobody reads are not even demuxed.
    let keep: Vec<usize> = [video.as_ref().map(|v| v.0), audio.as_ref().map(|a| a.0), info.selected_subtitle].into_iter().flatten().collect();
    input.discard_except(&keep);

    let cover = if video.is_none() { cover_art(&input) } else { None };
    let frame_dur = video.as_ref().map_or(1.0 / 25.0, |v| v.1.frame_dur);
    let seekable = info.is_seekable;
    let info = Arc::new(info);
    let _ = args.msgs.send(SessionMsg::Ready(SessionReady { id, info: info.clone(), cover, has_video: video.is_some(), has_audio: audio.is_some(), frame_dur, seekable }));
    args.shared.wake();

    // --- threads -------------------------------------------------------------------------------
    let mut workers: Vec<JoinHandle<()>> = Vec::new();
    let mut vlink = None;
    let mut alink = None;
    let mut queues: Vec<Arc<PacketQueue>> = Vec::new();

    if let Some((index, v)) = video {
        let queue = Arc::new(PacketQueue::new(v.time_base));
        queues.push(queue.clone());
        let params = VideoDecoderParams {
            decoder: v.decoder,
            codec_name: v.codec,
            hw: v.hw,
            queue: queue.clone(),
            shared: args.shared.clone(),
            stop: stop.clone(),
            time_base: v.time_base,
            time_base_ratio: v.time_base_ratio,
            start_offset,
            frame_dur: v.frame_dur,
            stream_sar: v.sar,
            allow_16bit: args.cfg.allow_16bit_textures,
            queue_bytes: args.cfg.frame_queue_bytes,
        };
        workers.push(thread::Builder::new().name("lumen400-vdec".into()).stack_size(16 * 1024 * 1024).spawn(move || video_dec::run(params)).expect("spawn video decoder"));
        vlink = Some(StreamLink { index, queue });
    }
    if let Some((index, a)) = audio {
        let queue = Arc::new(PacketQueue::new(a.time_base));
        queues.push(queue.clone());
        let params = AudioDecoderParams {
            decoder: a.decoder,
            queue: queue.clone(),
            shared: args.shared.clone(),
            dev: args.audio.dev.clone(),
            link: args.audio.link.clone(),
            out_rate: args.audio.rate,
            stop: stop.clone(),
            time_base: a.time_base,
            start_offset,
            buffer_ms: args.cfg.audio_buffer_ms,
        };
        workers.push(thread::Builder::new().name("lumen400-adec".into()).stack_size(16 * 1024 * 1024).spawn(move || audio_dec::run(params)).expect("spawn audio decoder"));
        alink = Some(StreamLink { index, queue });
    }

    Demuxer::new(DemuxerSetup { input, video: vlink, audio: alink, subs, ctl: ctl.clone(), shared: args.shared.clone(), start_offset, seekable }).run(args.start);

    // --- shutdown ------------------------------------------------------------------------------
    stop.store(true, Ordering::Release);
    for q in &queues {
        q.wake();
    }
    args.shared.frames.wake();
    for w in workers {
        let _ = w.join();
    }
}

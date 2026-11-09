//! The video decoding thread: packets in, presentable [`VideoFrame`]s out.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use lumen400_core::settings::Deinterlace;

use crate::codecs::{DiscardMode, VideoDecoder};
use crate::container::MediaPacket;
use crate::deinterlace::Deinterlacer;
use crate::frame::{Pixel, Plan, RawVideoFrame, VideoFrame, color_info, plan};
use crate::hwaccel::{self, HwDecoder};
use crate::queue::{Item, PacketQueue};
use crate::shared::Shared;

/// Frames carry process-wide unique serials so a new session can never collide with the "last
/// shown" marker of the previous one.
static NEXT_SERIAL: AtomicU64 = AtomicU64::new(1);

/// During an accurate seek, non-reference frames are skipped until this close to the target.
const SKIP_MARGIN: f64 = 0.5;

/// Surfaces that must fail to download, with none ever succeeding, before hardware decoding is given up.
const HW_GIVE_UP_AFTER: u32 = 3;
/// Decoder errors, with no picture ever produced, that give hardware decoding up as well.
const HW_ERRORS_GIVE_UP_AFTER: u32 = 24;

pub(crate) struct VideoDecoderParams {
    pub decoder: VideoDecoder,
    pub codec_name: String,
    /// The hardware device attached to `decoder`, if any.
    pub hw: Option<HwDecoder>,
    pub queue: Arc<PacketQueue>,
    pub shared: Arc<Shared>,
    pub stop: Arc<AtomicBool>,
    pub time_base: f64,
    /// The same, as the fraction the filters want.
    pub time_base_ratio: (i32, i32),
    pub start_offset: f64,
    /// Nominal duration of one frame.
    pub frame_dur: f64,
    pub stream_sar: f32,
    pub allow_16bit: bool,
    pub queue_bytes: usize,
}

struct Worker {
    p: VideoDecoderParams,
    epoch: u32,
    /// Accurate seek in progress: frames ending before this time are discarded.
    target: Option<f64>,
    skipping: bool,
    can_skip: bool,
    last_pts: Option<f64>,
    sized_queue: bool,
    /// Hardware pictures downloaded / that failed to download.
    hw_ok: u32,
    hw_failed: u32,
    /// Decoder errors since the last picture.
    errors_in_a_row: u32,
    hw_given_up: bool,
    deint: Deinterlacer,
    /// The policy the deinterlacer was last given, and the name it last reported.
    deint_mode: Deinterlace,
    deint_name: Option<&'static str>,
    /// Pictures the deinterlacer has finished, on their way to the queue (kept to reuse its memory).
    ready: Vec<RawVideoFrame>,
}

/// Codecs whose decoders honour `skip_frame = NonReference` reliably.
fn supports_skip(codec: &str) -> bool {
    matches!(codec, "h264" | "hevc" | "mpeg2video" | "mpeg4" | "vp8" | "vp9" | "l4v1")
}

pub(crate) fn run(p: VideoDecoderParams) {
    let can_skip = supports_skip(&p.codec_name);
    let mut w = Worker {
        p,
        epoch: 0,
        target: None,
        skipping: false,
        can_skip,
        last_pts: None,
        sized_queue: false,
        hw_ok: 0,
        hw_failed: 0,
        errors_in_a_row: 0,
        hw_given_up: false,
        deint: Deinterlacer::new(),
        deint_mode: Deinterlace::default(),
        deint_name: None,
        ready: Vec::new(),
    };
    w.main();
}

impl Worker {
    fn main(&mut self) {
        let q = self.p.queue.clone();
        while !self.p.stop.load(Ordering::Relaxed) {
            let Some(item) = q.pop(Duration::from_millis(20)) else { continue };
            match item {
                Item::Flush { epoch, target, accurate } => {
                    self.p.decoder.flush_to(target + self.p.start_offset);
                    self.epoch = epoch;
                    self.target = accurate.then_some(target);
                    self.last_pts = None;
                    // The pictures it was holding belong to the old position.
                    self.deint.reset();
                    self.ready.clear();
                    self.publish_deinterlacer();
                    let skip = accurate && self.can_skip && target > 1.0;
                    self.set_skip(skip);
                }
                Item::Packet(pkt) => {
                    if self.epoch != self.p.shared.current_epoch() {
                        continue; // queued before a seek we have not been told about yet
                    }
                    self.maybe_stop_skipping(&pkt);
                    self.feed(&pkt);
                }
                Item::Eof => {
                    if self.epoch == self.p.shared.current_epoch() {
                        let _ = self.p.decoder.send_eof();
                        self.drain();
                        // The last picture was waiting for a next one that will never come.
                        let mut ready = std::mem::take(&mut self.ready);
                        self.deint.finish(&mut ready);
                        self.publish_deinterlacer();
                        self.present_all(&mut ready);
                        self.ready = ready;
                        self.p.shared.frames.push_eos(self.epoch);
                    }
                }
            }
        }
    }

    fn set_skip(&mut self, on: bool) {
        if on != self.skipping {
            self.p.decoder.skip_frame(if on { DiscardMode::NonReference } else { DiscardMode::Default });
            self.skipping = on;
        }
    }

    fn maybe_stop_skipping(&mut self, pkt: &MediaPacket) {
        if !self.skipping {
            return;
        }
        if let (Some(t), Some(ts)) = (self.target, pkt.pts().or(pkt.dts())) {
            let s = ts as f64 * self.p.time_base - self.p.start_offset;
            if s >= t - SKIP_MARGIN {
                self.set_skip(false);
            }
        }
    }

    fn feed(&mut self, pkt: &MediaPacket) {
        if let Err(e) = self.p.decoder.send_packet(pkt) {
            self.p.shared.counters.decode_errors.fetch_add(1, Ordering::Relaxed);
            tracing::debug!("video send_packet: {e}");
            self.errors_in_a_row += 1;
            if self.errors_in_a_row >= HW_ERRORS_GIVE_UP_AFTER && self.hw_ok == 0 {
                self.give_up_hardware();
            }
            return;
        }
        self.drain();
    }

    /// Pulls every available frame. Returns `false` when the epoch changed or we must stop.
    fn drain(&mut self) -> bool {
        while let Some(frame) = self.p.decoder.receive_frame() {
            if !self.handle(frame) {
                return false;
            }
        }
        true
    }

    /// Returns `false` when the frame could not be queued because the epoch is stale / stopping.
    fn handle(&mut self, frame: RawVideoFrame) -> bool {
        self.errors_in_a_row = 0;
        let Some(frame) = self.download(frame) else { return true };

        // Pictures before the target of an accurate seek are decoded only to get there: they are not
        // worth filtering.
        if let (Some(target), Some(stamp)) = (self.target, frame.timestamp()) {
            let pts = stamp as f64 * self.p.time_base - self.p.start_offset;
            if pts + 0.5 * self.p.frame_dur < target {
                self.last_pts = Some(pts);
                return true;
            }
        }

        let mode = self.p.shared.deinterlace();
        let mut ready = std::mem::take(&mut self.ready);
        if mode != self.deint_mode {
            // The policy changed: the picture the filter still holds goes out the way it was meant to.
            self.deint.finish(&mut ready);
            self.deint_mode = mode;
        }
        self.deint.push(mode, self.p.time_base_ratio, frame, &mut ready);
        self.publish_deinterlacer();
        let queued = self.present_all(&mut ready);
        self.ready = ready;
        queued
    }

    fn download(&mut self, frame: RawVideoFrame) -> Option<RawVideoFrame> {
        let Some(hw) = self.p.hw.filter(|hw| frame.format() == hw.surface) else { return Some(frame) };
        match hwaccel::download(&frame) {
            Some(copy) => {
                self.hw_ok += 1;
                if self.hw_ok == 1 {
                    tracing::info!("decoding in hardware ({})", hw.name());
                    *self.p.shared.counters.hw_device.lock() = Some(hw.name());
                }
                Some(copy)
            }
            None => {
                self.hw_failed += 1;
                self.p.shared.counters.decode_errors.fetch_add(1, Ordering::Relaxed);
                if self.hw_ok == 0 && self.hw_failed >= HW_GIVE_UP_AFTER {
                    self.give_up_hardware();
                }
                None
            }
        }
    }

    /// Tells the statistics which deinterlacer (if any) is in the way, when that changed.
    fn publish_deinterlacer(&mut self) {
        let now = self.deint.active();
        if now != self.deint_name {
            self.deint_name = now;
            *self.p.shared.counters.deint.lock() = now;
        }
    }

    /// Queues what the deinterlacer finished, in order. `false`: the epoch is stale or we are stopping.
    fn present_all(&mut self, ready: &mut Vec<RawVideoFrame>) -> bool {
        for frame in ready.drain(..) {
            if !self.present(frame) {
                return false;
            }
        }
        true
    }

    /// Turns a finished picture into a [`VideoFrame`] and queues it for the renderer.
    fn present(&mut self, frame: RawVideoFrame) -> bool {
        let dur = self.p.frame_dur;
        let pts = match frame.timestamp() {
            Some(t) => t as f64 * self.p.time_base - self.p.start_offset,
            None => self.last_pts.map_or(0.0, |l| l + dur),
        };
        self.last_pts = Some(pts);

        if let Some(t) = self.target {
            if pts + 0.5 * dur < t {
                return true; // before the seek target: decoded only to reach it
            }
            self.target = None;
            self.set_skip(false);
        }

        let (w, h) = (frame.width(), frame.height());
        if w == 0 || h == 0 {
            return true;
        }
        let sar = {
            let r = frame.aspect_ratio();
            if r.0 > 0 && r.1 > 0 { r.0 as f32 / r.1 as f32 } else { self.p.stream_sar }
        };
        let (out, native) = match plan(frame.format(), self.p.allow_16bit) {
            Plan::Direct(n) => (None, n),
            Plan::Convert { target, native } => match self.convert(&frame, target) {
                Some(f) => (Some(f), native),
                None => return true,
            },
        };
        let color = color_info(frame.color_space(), frame.color_range(), frame.color_transfer_characteristic(), frame.color_primaries(), w, h, native.full_range);
        let serial = NEXT_SERIAL.fetch_add(1, Ordering::Relaxed);
        let vf = VideoFrame::new(out.unwrap_or(frame), pts, dur, sar, native, color, serial);

        if !self.sized_queue {
            // Size the queue from the real frame size: ~192 MiB regardless of resolution.
            let per = vf.byte_size().max(1);
            self.p.shared.frames.set_capacity((self.p.queue_bytes / per).clamp(6, 36));
            self.sized_queue = true;
        }
        self.p.shared.counters.decoded_frames.fetch_add(1, Ordering::Relaxed);
        self.p.shared.video_size.store((u64::from(vf.width) << 32) | u64::from(vf.height), Ordering::Relaxed);
        let ok = self.p.shared.frames.push(Arc::new(vf), self.epoch, &self.p.stop);
        if ok {
            self.p.shared.wake();
        }
        ok
    }

    /// The hardware path produced nothing usable: ask the player to restart the media in software.
    fn give_up_hardware(&mut self) {
        if self.p.hw.is_none() || self.hw_given_up {
            return;
        }
        self.hw_given_up = true;
        tracing::warn!("hardware decoding does not work for this media; asking for a software restart");
        *self.p.shared.counters.hw_device.lock() = None;
        self.p.shared.hw_failed.store(true, Ordering::Release);
        self.p.shared.wake();
    }

    fn convert(&mut self, src: &RawVideoFrame, dst: Pixel) -> Option<RawVideoFrame> {
        match src.convert_to(dst) {
            Some(out) => Some(out),
            None => {
                self.p.shared.counters.decode_errors.fetch_add(1, Ordering::Relaxed);
                None
            }
        }
    }
}

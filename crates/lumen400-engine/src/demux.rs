//! The demuxer thread: reads packets from the container, routes them to the decoder queues and
//! carries out seeks.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use parking_lot::{Condvar, Mutex};

use crate::container::{ContainerInput, MediaPacket};
use crate::queue::{Item, PacketQueue};
use crate::shared::{Event, Notice, Shared};
use crate::subs::EmbeddedSubs;

/// A request to reposition the whole pipeline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SeekReq {
    /// Epoch the new data belongs to.
    pub epoch: u32,
    /// Media time (seconds) to land on.
    pub target: f64,
    /// Decoders discard everything before `target` when set.
    pub accurate: bool,
}

/// Cross-thread control of one demuxer.
pub(crate) struct DemuxCtl {
    pub stop: Arc<AtomicBool>,
    seek: Mutex<Option<SeekReq>>,
    cv: Condvar,
}

impl DemuxCtl {
    pub fn new(stop: Arc<AtomicBool>) -> Self {
        Self { stop, seek: Mutex::new(None), cv: Condvar::new() }
    }

    /// Queues a seek; a newer request replaces an older one that was not started yet.
    pub fn request_seek(&self, req: SeekReq) {
        *self.seek.lock() = Some(req);
        self.cv.notify_all();
    }

    fn take_seek(&self) -> Option<SeekReq> {
        self.seek.lock().take()
    }

    pub fn wake(&self) {
        self.cv.notify_all();
    }

    fn park(&self, timeout: Duration) {
        let mut g = self.seek.lock();
        if g.is_none() && !self.stop.load(Ordering::Relaxed) {
            self.cv.wait_for(&mut g, timeout);
        }
    }
}

/// Where the packets of one elementary stream go.
pub(crate) struct StreamLink {
    pub index: usize,
    pub queue: Arc<PacketQueue>,
}

/// Total packet bytes buffered ahead of the decoders.
const MAX_QUEUE_BYTES: usize = 48 << 20;
/// A stream has "enough" when it holds this many packets *and* this many seconds.
const ENOUGH_PACKETS: usize = 24;
const ENOUGH_SECONDS: f64 = 1.0;

/// A keyframe this far after the target of a seek is a late landing.
const LATE_BY: f64 = 0.001;
/// How often a late landing is tried again from further back, and how far back the first retry goes
/// (every further one doubles it).
const MAX_RESEEKS: u32 = 4;
const FIRST_BACK: f64 = 0.5;

/// A seek whose landing spot is still to be checked.
#[derive(Clone, Copy)]
struct Landing {
    req: SeekReq,
    /// How far before the target the attempt aimed, in seconds.
    back: f64,
    attempts: u32,
}

/// What the demuxer thread is given to work with.
pub(crate) struct DemuxerSetup {
    pub input: ContainerInput,
    pub video: Option<StreamLink>,
    pub audio: Option<StreamLink>,
    pub subs: Option<EmbeddedSubs>,
    pub ctl: Arc<DemuxCtl>,
    pub shared: Arc<Shared>,
    /// Container start time in seconds (subtracted from every timestamp).
    pub start_offset: f64,
    pub seekable: bool,
}

pub(crate) struct Demuxer {
    input: ContainerInput,
    video: Option<StreamLink>,
    audio: Option<StreamLink>,
    subs: Option<EmbeddedSubs>,
    ctl: Arc<DemuxCtl>,
    shared: Arc<Shared>,
    start_offset: f64,
    seekable: bool,
    eof: bool,
    at_start: bool,
    /// Seconds per tick of the video stream's timestamps.
    video_tb: f64,
    landing: Option<Landing>,
}

impl Demuxer {
    pub fn new(setup: DemuxerSetup) -> Self {
        let DemuxerSetup { input, video, audio, subs, ctl, shared, start_offset, seekable } = setup;
        let video_tb = video.as_ref().and_then(|v| input.stream(v.index)).map_or(0.0, |s| s.time_base_sec());
        Self { input, video, audio, subs, ctl, shared, start_offset, seekable, eof: false, at_start: true, video_tb, landing: None }
    }

    fn links(&self) -> impl Iterator<Item = &StreamLink> {
        self.video.iter().chain(self.audio.iter())
    }

    fn should_wait(&self) -> bool {
        let bytes: usize = self.links().map(|l| l.queue.level().bytes).sum();
        if bytes > MAX_QUEUE_BYTES {
            return true;
        }
        let enough = |l: &Option<StreamLink>| {
            l.as_ref().is_none_or(|l| {
                let lv = l.queue.level();
                lv.packets > ENOUGH_PACKETS && lv.seconds > ENOUGH_SECONDS
            })
        };
        enough(&self.video) && enough(&self.audio)
    }

    fn on_eof(&mut self) {
        for l in self.video.iter().chain(self.audio.iter()) {
            l.queue.push(Item::Eof);
        }
        self.eof = true;
    }

    fn route(&mut self, pkt: MediaPacket) {
        let idx = pkt.stream();
        self.shared.counters.bytes_read.fetch_add(pkt.size() as u64, Ordering::Relaxed);
        if self.video.as_ref().is_some_and(|v| v.index == idx) {
            if self.landing.is_some() && pkt.is_key() && self.landed_late(&pkt) {
                return; // the seek was made again; this packet belongs to the old landing
            }
            if let Some(v) = self.video.as_ref() {
                v.queue.push(Item::Packet(pkt));
            }
        } else if let Some(a) = self.audio.as_ref().filter(|a| a.index == idx) {
            a.queue.push(Item::Packet(pkt));
        } else if let Some(s) = self.subs.as_mut().filter(|s| s.index == idx) {
            s.feed(&pkt, self.start_offset);
        }
    }

    fn do_seek(&mut self, req: SeekReq) {
        self.seek_from(req, 0.0, 0);
    }

    /// The first keyframe after a seek has come. `true`: it lies after the target, the seek was made
    /// again from further back and the packet is to be dropped.
    fn landed_late(&mut self, pkt: &MediaPacket) -> bool {
        let Some(landing) = self.landing.take() else { return false };
        let Some(stamp) = pkt.pts().or(pkt.dts()) else { return false };
        let at = stamp as f64 * self.video_tb - self.start_offset;
        let aimed_at_the_start = landing.req.target - landing.back <= 0.0;
        if at <= landing.req.target + LATE_BY || landing.attempts >= MAX_RESEEKS || aimed_at_the_start {
            return false;
        }
        let back = if landing.back == 0.0 { FIRST_BACK } else { landing.back * 2.0 };
        tracing::debug!("seek to {:.3}s landed on a keyframe at {at:.3}s: again, {back} s earlier", landing.req.target);
        self.seek_from(landing.req, back, landing.attempts + 1);
        true
    }

    /// Positions the container `back` seconds before the target of `req` and tells the decoders.
    fn seek_from(&mut self, req: SeekReq, back: f64, attempts: u32) {
        let marker = || Item::Flush { epoch: req.epoch, target: req.target, accurate: req.accurate };
        for l in self.video.iter().chain(self.audio.iter()) {
            l.queue.reset_with(marker());
        }
        if let Some(s) = self.subs.as_mut() {
            s.flush();
        }
        self.landing = None;
        let wants_seek = !(self.at_start && req.target < 1e-3);
        if wants_seek && self.seekable {
            let aim = (req.target - back).max(0.0);
            match self.input.seek_seconds(aim + self.start_offset) {
                Ok(()) if self.video.is_some() && self.video_tb > 0.0 => self.landing = Some(Landing { req, back, attempts }),
                Ok(()) => {}
                Err(e) => {
                    tracing::warn!("seek to {:.3}s failed: {e}", req.target);
                    self.shared.emit(Event::Warning(Notice::Other(format!("seek failed: {e}"))));
                }
            }
        } else if self.seekable && req.target < 1e-3 {
            let _ = self.input.seek_seconds(self.start_offset);
        }
        self.eof = false;
        self.at_start = req.target < 1e-3;
    }

    /// Runs until the session is stopped or the input ends.
    pub fn run(mut self, first: SeekReq) {
        self.do_seek(first);
        let stop = self.ctl.stop.clone();
        while !stop.load(Ordering::Relaxed) {
            if let Some(req) = self.ctl.take_seek() {
                self.do_seek(req);
                continue;
            }
            if self.eof || self.should_wait() {
                self.ctl.park(Duration::from_millis(5));
                continue;
            }
            match self.input.read_packet() {
                Some(pkt) => {
                    self.at_start = false;
                    self.route(pkt);
                }
                None => self.on_eof(),
            }
        }
        for l in self.links() {
            l.queue.wake();
        }
    }
}

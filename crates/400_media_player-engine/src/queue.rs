//! Packet and frame queues with seek-generation handling.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use parking_lot::{Condvar, Mutex};

use crate::container::MediaPacket;
use crate::frame::VideoFrame;

/// Unit of work travelling from the demuxer to a decoder thread.
pub(crate) enum Item {
    Packet(MediaPacket),
    /// A seek happened: drop decoder state. `target` is the position to land on; when `accurate`
    /// the decoder discards everything before it.
    Flush {
        epoch: u32,
        target: f64,
        accurate: bool,
    },
    /// End of stream: drain the decoder.
    Eof,
}

impl Item {
    fn bytes(&self) -> usize {
        match self {
            Item::Packet(p) => p.size(),
            _ => 0,
        }
    }
    fn ticks(&self) -> i64 {
        match self {
            Item::Packet(p) => p.duration().max(0),
            _ => 0,
        }
    }
}

#[derive(Default)]
struct PqInner {
    items: VecDeque<Item>,
    bytes: usize,
    packets: usize,
    ticks: i64,
}

/// How much is waiting in a [`PacketQueue`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct QueueLevel {
    pub packets: usize,
    pub bytes: usize,
    /// Sum of the packet durations, in seconds.
    pub seconds: f64,
}

/// Unbounded FIFO of packets (the demuxer throttles itself by looking at [`level`](Self::level)).
pub(crate) struct PacketQueue {
    inner: Mutex<PqInner>,
    cv: Condvar,
    /// Seconds per tick of the packet durations.
    tb: f64,
}

impl PacketQueue {
    pub fn new(time_base: f64) -> Self {
        Self { inner: Mutex::new(PqInner::default()), cv: Condvar::new(), tb: time_base }
    }

    pub fn push(&self, item: Item) {
        let mut g = self.inner.lock();
        g.bytes += item.bytes();
        g.ticks += item.ticks();
        if matches!(item, Item::Packet(_)) {
            g.packets += 1;
        }
        g.items.push_back(item);
        self.cv.notify_one();
    }

    /// Waits up to `timeout` for the next item.
    pub fn pop(&self, timeout: Duration) -> Option<Item> {
        let mut g = self.inner.lock();
        if g.items.is_empty() {
            self.cv.wait_for(&mut g, timeout);
        }
        let item = g.items.pop_front()?;
        g.bytes -= item.bytes();
        g.ticks -= item.ticks();
        if matches!(item, Item::Packet(_)) {
            g.packets -= 1;
        }
        Some(item)
    }

    /// Drops everything and queues `marker` first.
    pub fn reset_with(&self, marker: Item) {
        let mut g = self.inner.lock();
        g.items.clear();
        g.bytes = 0;
        g.packets = 0;
        g.ticks = 0;
        g.items.push_back(marker);
        self.cv.notify_all();
    }

    pub fn level(&self) -> QueueLevel {
        let g = self.inner.lock();
        QueueLevel { packets: g.packets, bytes: g.bytes, seconds: g.ticks as f64 * self.tb }
    }

    pub fn wake(&self) {
        self.cv.notify_all();
    }
}

struct FqInner {
    q: VecDeque<Arc<VideoFrame>>,
    cap: usize,
    epoch: u32,
    eos: bool,
    shown: u64,
}

/// What [`FrameQueue::take_due`] decided.
#[derive(Default)]
pub struct Taken {
    /// A frame that was not shown before and is due now.
    pub frame: Option<Arc<VideoFrame>>,
    /// Frames discarded because they were already late.
    pub dropped: u32,
}

/// Bounded queue of decoded frames; the producer blocks when it is full.
pub struct FrameQueue {
    inner: Mutex<FqInner>,
    space: Condvar,
    len_mirror: AtomicUsize,
}

impl FrameQueue {
    pub fn new(capacity: usize) -> Self {
        Self { inner: Mutex::new(FqInner { q: VecDeque::new(), cap: capacity.max(2), epoch: 1, eos: false, shown: 0 }), space: Condvar::new(), len_mirror: AtomicUsize::new(0) }
    }

    pub fn set_capacity(&self, cap: usize) {
        let mut g = self.inner.lock();
        g.cap = cap.clamp(2, 512);
        self.space.notify_all();
    }

    /// Blocks while the queue is full. Returns `false` when the frame was rejected because the
    /// generation changed (a seek happened) or the session is stopping.
    pub fn push(&self, f: Arc<VideoFrame>, epoch: u32, stop: &AtomicBool) -> bool {
        let mut g = self.inner.lock();
        loop {
            if stop.load(Ordering::Relaxed) || g.epoch != epoch {
                return false;
            }
            if g.q.len() < g.cap {
                break;
            }
            self.space.wait_for(&mut g, Duration::from_millis(20));
        }
        g.q.push_back(f);
        self.len_mirror.store(g.q.len(), Ordering::Release);
        true
    }

    pub fn push_eos(&self, epoch: u32) {
        let mut g = self.inner.lock();
        if g.epoch == epoch {
            g.eos = true;
        }
    }

    /// Drops all frames and moves to generation `epoch`.
    pub fn flush(&self, epoch: u32) {
        let mut g = self.inner.lock();
        g.q.clear();
        g.epoch = epoch;
        g.eos = false;
        self.len_mirror.store(0, Ordering::Release);
        self.space.notify_all();
    }

    pub fn wake(&self) {
        self.space.notify_all();
    }

    /// Number of queued frames (lock-free read).
    pub fn len(&self) -> usize {
        self.len_mirror.load(Ordering::Acquire)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// `true` once the decoder signalled the end and every frame was consumed.
    pub fn drained(&self) -> bool {
        let g = self.inner.lock();
        g.eos && g.q.is_empty()
    }

    /// PTS of the next queued frame.
    pub fn peek_pts(&self) -> Option<f64> {
        self.inner.lock().q.front().map(|f| f.pts)
    }

    /// Presentation logic: pops every frame whose `pts <= deadline` and returns the newest of
    /// them (if it was not shown yet). Frames skipped on the way count as dropped.
    pub fn take_due(&self, deadline: f64) -> Taken {
        let mut g = self.inner.lock();
        let mut chosen: Option<Arc<VideoFrame>> = None;
        let mut popped = 0u32;
        while g.q.front().is_some_and(|f| f.pts <= deadline) {
            chosen = g.q.pop_front();
            popped += 1;
        }
        if popped > 0 {
            self.len_mirror.store(g.q.len(), Ordering::Release);
            self.space.notify_all();
        }
        match chosen {
            Some(f) if f.serial != g.shown => {
                g.shown = f.serial;
                Taken { frame: Some(f), dropped: popped.saturating_sub(1) }
            }
            _ => Taken { frame: None, dropped: 0 },
        }
    }

    /// Pops the oldest frame regardless of its timestamp (first frame after a seek, frame step).
    pub fn take_front(&self) -> Option<Arc<VideoFrame>> {
        let mut g = self.inner.lock();
        let f = g.q.pop_front()?;
        g.shown = f.serial;
        self.len_mirror.store(g.q.len(), Ordering::Release);
        self.space.notify_all();
        Some(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_queue_levels_and_reset() {
        let q = PacketQueue::new(0.001);
        let mut a = MediaPacket::copy(&[1, 2, 3]);
        a.set_duration(40);
        let mut b = MediaPacket::copy(&[4, 5]);
        b.set_duration(60);
        q.push(Item::Packet(a));
        q.push(Item::Packet(b));
        let l = q.level();
        assert_eq!((l.packets, l.bytes), (2, 5));
        assert!((l.seconds - 0.1).abs() < 1e-9);
        assert!(matches!(q.pop(Duration::from_millis(1)), Some(Item::Packet(_))));
        let l = q.level();
        assert_eq!((l.packets, l.bytes), (1, 2));
        assert!((l.seconds - 0.06).abs() < 1e-9);
        q.reset_with(Item::Flush { epoch: 2, target: 1.0, accurate: true });
        assert_eq!(q.level(), QueueLevel::default());
        assert!(matches!(q.pop(Duration::from_millis(1)), Some(Item::Flush { epoch: 2, .. })));
        assert!(q.pop(Duration::from_millis(1)).is_none());
    }
}

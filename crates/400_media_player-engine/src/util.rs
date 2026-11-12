//! Monotonic time and the lock-free playback clock.

use std::hint::spin_loop;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering, fence};
use std::time::Instant;

static START: OnceLock<Instant> = OnceLock::new();

/// Nanoseconds on a process-wide monotonic clock (cheap, never goes backwards).
#[inline]
pub fn mono_ns() -> u64 {
    START.get_or_init(Instant::now).elapsed().as_nanos() as u64
}

/// How far the audio clock may be extrapolated past its last anchor (callback stalls).
const MAX_EXTRAPOLATE: f64 = 0.30;
/// How far *before* the anchor it may be evaluated (output latency compensation).
const MAX_LOOKBACK: f64 = 0.50;

/// A consistent snapshot of the clock state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anchor {
    /// Media time (seconds) at `at_ns`.
    pub pts: f64,
    pub at_ns: u64,
    /// Media seconds per real second; `0.0` when frozen.
    pub rate: f64,
}

/// The master clock of a playback session.
///
/// It stores an *anchor* — "at monotonic instant `at`, the media position was `pts` and moves
/// at `rate`" — and extrapolates from it. In audio-driven mode the audio callback re-anchors the
/// clock every period with the position of the samples that are *actually being heard*
/// (device latency included); otherwise the controller anchors it on play / pause / seek.
///
/// Reads are wait-free (a seqlock); writers serialise through a tiny spin lock, and the audio
/// callback uses [`try_set_at`](Self::try_set_at) so it never blocks.
#[derive(Debug)]
pub struct PlaybackClock {
    seq: AtomicU32,
    pts: AtomicU64,
    at: AtomicU64,
    rate: AtomicU64,
    writer: AtomicBool,
    audio_driven: AtomicBool,
    /// While `true` (clock held / frozen by the controller) the audio callback may not move it.
    locked: AtomicBool,
}

impl Default for PlaybackClock {
    fn default() -> Self {
        Self::new()
    }
}

impl PlaybackClock {
    pub fn new() -> Self {
        Self {
            seq: AtomicU32::new(0),
            pts: AtomicU64::new(0f64.to_bits()),
            at: AtomicU64::new(mono_ns()),
            rate: AtomicU64::new(0f64.to_bits()),
            writer: AtomicBool::new(false),
            audio_driven: AtomicBool::new(false),
            locked: AtomicBool::new(true),
        }
    }

    pub fn set_audio_driven(&self, v: bool) {
        self.audio_driven.store(v, Ordering::Release);
    }

    pub fn is_audio_driven(&self) -> bool {
        self.audio_driven.load(Ordering::Acquire)
    }

    /// Reads a consistent anchor.
    pub fn anchor(&self) -> Anchor {
        loop {
            let s1 = self.seq.load(Ordering::Acquire);
            if s1 & 1 == 1 {
                spin_loop();
                continue;
            }
            let a =
                Anchor { pts: f64::from_bits(self.pts.load(Ordering::Relaxed)), at_ns: self.at.load(Ordering::Relaxed), rate: f64::from_bits(self.rate.load(Ordering::Relaxed)) };
            fence(Ordering::Acquire);
            if self.seq.load(Ordering::Relaxed) == s1 {
                return a;
            }
        }
    }

    fn write_locked(&self, a: Anchor) {
        self.seq.fetch_add(1, Ordering::AcqRel);
        self.pts.store(a.pts.to_bits(), Ordering::Relaxed);
        self.at.store(a.at_ns, Ordering::Relaxed);
        self.rate.store(a.rate.to_bits(), Ordering::Relaxed);
        self.seq.fetch_add(1, Ordering::Release);
    }

    /// Authoritative write by the controller (blocks for a few nanoseconds if another writer is
    /// active). A stopped clock (`rate == 0`) is *held*: the audio callback cannot move it until
    /// [`release`](Self::release) is called.
    pub fn set_at(&self, pts: f64, at_ns: u64, rate: f64) {
        while self.writer.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            spin_loop();
        }
        self.locked.store(rate == 0.0, Ordering::Release);
        self.write_locked(Anchor { pts, at_ns, rate });
        self.writer.store(false, Ordering::Release);
    }

    /// Non-blocking write for real-time threads. Returns `false` if another writer was active or
    /// if the clock is held by the controller.
    pub fn try_set_at(&self, pts: f64, at_ns: u64, rate: f64) -> bool {
        if self.writer.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            return false;
        }
        let ok = !self.locked.load(Ordering::Acquire);
        if ok {
            self.write_locked(Anchor { pts, at_ns, rate });
        }
        self.writer.store(false, Ordering::Release);
        ok
    }

    /// Stops the clock at `pts` and keeps the audio callback from moving it.
    pub fn hold(&self, pts: f64) {
        self.set_at(pts, mono_ns(), 0.0);
    }

    /// Lets the audio callback move the clock again (it stays stopped until the first anchor).
    pub fn release(&self) {
        while self.writer.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            spin_loop();
        }
        self.locked.store(false, Ordering::Release);
        self.writer.store(false, Ordering::Release);
    }

    #[cfg(test)]
    pub fn is_held(&self) -> bool {
        self.locked.load(Ordering::Acquire)
    }

    /// Anchors at "now".
    pub fn set(&self, pts: f64, rate: f64) {
        self.set_at(pts, mono_ns(), rate);
    }

    /// Current media position in seconds.
    pub fn now(&self) -> f64 {
        let a = self.anchor();
        let dt = (i128::from(mono_ns()) - i128::from(a.at_ns)) as f64 * 1e-9;
        let dt = if self.is_audio_driven() { dt.clamp(-MAX_LOOKBACK, MAX_EXTRAPOLATE) } else { dt.max(-MAX_LOOKBACK) };
        a.pts + dt * a.rate
    }

    /// Stops the clock where it currently is.
    #[cfg(test)]
    pub fn freeze(&self) {
        let p = self.now();
        self.set(p, 0.0);
    }

    /// `true` when the clock is advancing.
    #[cfg(test)]
    pub fn is_running(&self) -> bool {
        self.anchor().rate != 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn runs_freezes_and_scales() {
        let c = PlaybackClock::new();
        c.set(10.0, 1.0);
        thread::sleep(Duration::from_millis(100));
        let t = c.now();
        assert!((10.08..10.25).contains(&t), "{t}");
        c.freeze();
        let f = c.now();
        thread::sleep(Duration::from_millis(50));
        assert_eq!(c.now(), f);
        c.set(0.0, 2.0);
        thread::sleep(Duration::from_millis(100));
        let t = c.now();
        assert!((0.15..0.5).contains(&t), "{t}");
        assert!(c.is_running());
    }

    #[test]
    fn audio_anchor_in_the_future_is_continuous() {
        let c = PlaybackClock::new();
        c.set_audio_driven(true);
        // Buffer starting at pts=5 will be heard 20 ms from now.
        c.set_at(5.0, mono_ns() + 20_000_000, 1.0);
        let t = c.now();
        assert!((4.97..=5.0).contains(&t), "{t}");
    }

    #[test]
    fn audio_extrapolation_is_bounded() {
        let c = PlaybackClock::new();
        c.set_audio_driven(true);
        c.set(1.0, 1.0);
        thread::sleep(Duration::from_millis(500));
        assert!(c.now() <= 1.0 + MAX_EXTRAPOLATE + 1e-3);
    }

    #[test]
    fn readers_never_see_torn_values() {
        // pts and at_ns are always written together: pts == at_ns / 1000 in this test.
        let c = Arc::new(PlaybackClock::new());
        c.set_at(0.0, 0, 1.0);
        let writer = {
            let c = c.clone();
            thread::spawn(move || {
                for i in 1..200_000u64 {
                    c.set_at(i as f64, i * 1000, 1.0);
                }
            })
        };
        let reader = {
            let c = c.clone();
            thread::spawn(move || {
                for _ in 0..200_000 {
                    let a = c.anchor();
                    assert_eq!(a.pts as u64 * 1000, a.at_ns, "torn read");
                }
            })
        };
        writer.join().unwrap();
        reader.join().unwrap();
    }

    #[test]
    fn held_clock_ignores_the_audio_callback() {
        let c = PlaybackClock::new();
        c.set_audio_driven(true);
        assert!(!c.try_set_at(1.0, 5, 1.0), "a fresh clock is held until the controller releases it");
        c.hold(7.0);
        assert!(c.is_held());
        assert!(!c.try_set_at(9.0, 5, 1.0));
        assert_eq!(c.anchor().pts, 7.0);
        assert!(!c.is_running());
        c.release();
        assert!(c.try_set_at(9.0, 5, 1.0));
        assert_eq!(c.anchor().pts, 9.0);
        assert!(c.is_running());
        // freezing (pause) holds again, even right after an anchor
        c.freeze();
        assert!(!c.try_set_at(10.0, 5, 1.0));
        // a running system clock is not held
        c.set(3.0, 1.0);
        assert!(!c.is_held());
    }
}

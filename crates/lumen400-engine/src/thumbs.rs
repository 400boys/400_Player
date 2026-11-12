//! Seek-bar preview thumbnails.
//!
//! A background thread opens its *own* demuxer on the file, so playback is never disturbed, jumps
//! to evenly spaced points, decodes the key frame it lands on and scales it down in pure Rust.
//! Pictures arrive coarse to fine — the middle of the film first, then the quarters, the eighths… —
//! so hovering the seek bar shows something useful within a fraction of a second.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use parking_lot::Mutex;

use crate::codecs::{DiscardMode, VideoDecoder};
use crate::container::{ContainerInput, MediaPacket};
use crate::frame::{ColorInfo, GamutKind, Matrix, RawVideoFrame, RgbaImage, Transfer, color_info};
use crate::probe::{LangPrefs, StreamSelection, describe, open_input, select_streams};
use crate::session::container_start_offset;
use crate::shared::Shared;

/// Thumbnails fit into a square of this many pixels.
pub const THUMB_BOX: u32 = 256;
/// Upper bound of the number of pictures per media.
const MAX_SLOTS: usize = 120;
/// Lower bound (for short clips).
const MIN_SLOTS: usize = 12;
/// Shorter media get no previews at all.
const MIN_DURATION: f64 = 4.0;

/// A preview picture.
#[derive(Clone, Debug)]
pub struct Thumbnail {
    /// Stable identifier of this picture (unique per media and slot): use it as a texture cache key.
    pub id: u64,
    /// The moment of the media the slot stands for, in seconds.
    pub time: f64,
    pub image: Arc<RgbaImage>,
}

/// Number of preview pictures for a media of `duration` seconds (`0` = none).
pub(crate) fn slot_count(duration: f64) -> usize {
    if !duration.is_finite() || duration < MIN_DURATION {
        return 0;
    }
    ((duration / 8.0).ceil() as usize).clamp(MIN_SLOTS, MAX_SLOTS)
}

/// The moment slot `i` of `count` stands for: the middle of its share of the media.
pub(crate) fn slot_time(i: usize, count: usize, duration: f64) -> f64 {
    (i as f64 + 0.5) * duration / count.max(1) as f64
}

/// The slot that covers `t`.
pub(crate) fn slot_of(t: f64, count: usize, duration: f64) -> usize {
    if count == 0 || duration <= 0.0 {
        return 0;
    }
    ((t / duration * count as f64).floor().max(0.0) as usize).min(count - 1)
}

/// Visits `0..n` coarse to fine: the first slot, the middle, the quarters, the eighths…
pub(crate) fn refinement_order(n: usize) -> Vec<usize> {
    let mut order = Vec::with_capacity(n);
    let mut seen = vec![false; n];
    let mut step = n.next_power_of_two();
    while step >= 1 {
        let mut i = 0;
        while i < n {
            if !seen[i] {
                seen[i] = true;
                order.push(i);
            }
            i += step;
        }
        step /= 2;
    }
    order
}

// -------------------------------------------------------------------------------------------------
// Store
// -------------------------------------------------------------------------------------------------

#[derive(Default)]
struct Inner {
    media: u64,
    duration: f64,
    slots: Vec<Option<Arc<RgbaImage>>>,
    done: usize,
}

/// The pictures of the current media, shared between the generator and the UI.
#[derive(Default)]
pub(crate) struct ThumbStore {
    inner: Mutex<Inner>,
}

impl ThumbStore {
    pub fn reset(&self, media: u64, duration: f64, count: usize) {
        *self.inner.lock() = Inner { media, duration, slots: vec![None; count], done: 0 };
    }

    pub fn clear(&self) {
        *self.inner.lock() = Inner::default();
    }

    /// Stores a picture; `false` when the store has moved on to another media.
    pub fn put(&self, media: u64, slot: usize, img: RgbaImage) -> bool {
        let mut g = self.inner.lock();
        if g.media != media || slot >= g.slots.len() {
            return false;
        }
        if g.slots[slot].is_none() {
            g.done += 1;
        }
        g.slots[slot] = Some(Arc::new(img));
        true
    }

    /// The closest available picture to `t`.
    pub fn nearest(&self, t: f64) -> Option<Thumbnail> {
        let g = self.inner.lock();
        let n = g.slots.len();
        if n == 0 || g.done == 0 {
            return None;
        }
        let i0 = slot_of(t, n, g.duration);
        for d in 0..n {
            for i in [i0.checked_sub(d), Some(i0 + d)].into_iter().flatten() {
                if let Some(Some(img)) = g.slots.get(i) {
                    return Some(Thumbnail { id: (g.media << 12) | i as u64, time: slot_time(i, n, g.duration), image: img.clone() });
                }
            }
        }
        None
    }

    /// `(pictures ready, pictures planned)`.
    pub fn progress(&self) -> (usize, usize) {
        let g = self.inner.lock();
        (g.done, g.slots.len())
    }
}

// -------------------------------------------------------------------------------------------------
// Generator
// -------------------------------------------------------------------------------------------------

/// Handle on the background generator of one media.
pub(crate) struct Thumbnailer {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Thumbnailer {
    /// Starts generating the previews of `source`; `None` when the media is too short.
    pub fn spawn(source: String, duration: f64, store: Arc<ThumbStore>, media: u64, shared: Arc<Shared>) -> Option<Thumbnailer> {
        let count = slot_count(duration);
        if count == 0 {
            return None;
        }
        store.reset(media, duration, count);
        let stop = Arc::new(AtomicBool::new(false));
        let job = Job { source, count, duration, store, media, stop: stop.clone(), shared };
        let thread = thread::Builder::new().name("lumen400-thumbs".into()).stack_size(16 * 1024 * 1024).spawn(move || run(job)).ok()?;
        Some(Thumbnailer { stop, thread: Some(thread) })
    }

    /// Asks the generator to stop; it exits by itself within one decode.
    pub fn cancel(self) {
        self.stop.store(true, Ordering::Release);
    }

    /// Stops the generator and waits for it.
    pub fn join(mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct Job {
    source: String,
    count: usize,
    duration: f64,
    store: Arc<ThumbStore>,
    media: u64,
    stop: Arc<AtomicBool>,
    shared: Arc<Shared>,
}

fn run(job: Job) {
    let Some(mut g) = Grabber::open(&job) else { return };
    // A key frame may stand for a slot when it is at most this far after the slot's middle.
    let tol = (job.duration / job.count as f64 * 0.25).clamp(0.1, 5.0);
    let mut failures = 0;
    for slot in refinement_order(job.count) {
        if job.stop.load(Ordering::Relaxed) {
            return;
        }
        while job.shared.state() == crate::shared::PlayState::Playing && job.shared.frames.len() < 16 {
            if job.stop.load(Ordering::Relaxed) {
                return;
            }
            thread::sleep(Duration::from_millis(50));
        }
        match g.grab(slot_time(slot, job.count, job.duration), tol) {
            Some(img) => {
                failures = 0;
                if !job.store.put(job.media, slot, img) {
                    return;
                }
                job.shared.wake();
            }
            None => {
                failures += 1;
                if failures >= 6 && job.store.progress().0 == 0 {
                    return;
                }
            }
        }
        let pause = if job.shared.state() == crate::shared::PlayState::Playing {
            Duration::from_millis(150)
        } else {
            Duration::from_millis(40)
        };
        thread::sleep(pause);
    }
}

struct Grabber {
    input: ContainerInput,
    decoder: VideoDecoder,
    index: usize,
    time_base: f64,
    start_offset: f64,
    lead: f64,
    stop: Arc<AtomicBool>,
}

enum Scan {
    Found(MediaPacket),
    TooLate,
    Nothing,
}

impl Grabber {
    fn open(job: &Job) -> Option<Grabber> {
        let mut input = open_input(&job.source, job.stop.clone()).ok()?;
        let mut info = describe(&input, &job.source);
        select_streams(&mut info, &StreamSelection::default(), &LangPrefs::default());
        let index = info.selected_video?;
        let start_offset = container_start_offset(&input);
        let stream = input.stream(index)?;
        let time_base = stream.time_base_sec();
        let mut decoder = VideoDecoder::new(stream).ok()?;
        decoder.skip_frame(DiscardMode::NonKey);
        input.discard_except(&[index]);
        Some(Grabber { input, decoder, index, time_base, start_offset, lead: 0.0, stop: job.stop.clone() })
    }

    /// The key frame that best stands for `t` (the last one not after `t + tol`), scaled down.
    fn grab(&mut self, t: f64, tol: f64) -> Option<RgbaImage> {
        let mut lead = self.lead;
        for _ in 0..8 {
            let at = (t - lead).max(0.0);
            match self.scan(at, t + tol, at <= 0.0) {
                Scan::Found(packet) => {
                    self.lead = lead;
                    return self.decode(&packet);
                }
                _ if at <= 0.0 => return None,
                _ => lead = if lead <= 0.0 { 1.0 } else { (lead * 2.5).min(60.0) },
            }
        }
        None
    }

    /// Seeks to `at` and walks the key frame packets up to `limit` without decoding them.
    fn scan(&mut self, at: f64, limit: f64, accept_first: bool) -> Scan {
        if self.input.seek_seconds(at + self.start_offset).is_err() {
            return Scan::Nothing;
        }
        let mut best: Option<MediaPacket> = None;
        let (mut keys, mut reads) = (0, 0);
        while let Some(packet) = self.input.read_packet() {
            if self.stop.load(Ordering::Relaxed) {
                return Scan::Nothing;
            }
            if packet.stream() != self.index {
                continue;
            }
            reads += 1;
            if !packet.is_key() {
                if reads > 20_000 {
                    break;
                }
                continue;
            }
            let Some(ts) = packet.pts().or(packet.dts()) else { continue };
            let pts = ts as f64 * self.time_base - self.start_offset;
            if pts > limit {
                if best.is_none() {
                    if accept_first {
                        best = Some(packet);
                    } else {
                        return Scan::TooLate;
                    }
                }
                break;
            }
            best = Some(packet);
            keys += 1;
            if keys >= 64 {
                break;
            }
        }
        best.map_or(Scan::Nothing, Scan::Found)
    }

    /// Decodes one key frame packet on its own.
    fn decode(&mut self, packet: &MediaPacket) -> Option<RgbaImage> {
        let target_sec = packet.pts().or(packet.dts()).map_or(0.0, |ts| ts as f64 * self.time_base);
        self.decoder.flush_to(target_sec);
        self.decoder.send_packet(packet).ok()?;
        let frame = match self.decoder.receive_frame() {
            Some(f) => f,
            None => {
                let _ = self.decoder.send_eof();
                self.decoder.receive_frame()?
            }
        };
        self.scale(&frame)
    }

    fn scale(&mut self, frame: &RawVideoFrame) -> Option<RgbaImage> {
        let (w, h) = (frame.width(), frame.height());
        if w == 0 || h == 0 {
            return None;
        }
        let sar = {
            let r = frame.aspect_ratio();
            if r.0 > 0 && r.1 > 0 { r.0 as f32 / r.1 as f32 } else { 1.0 }
        };
        let (tw, th) = thumb_size(w, h, sar);
        let color = color_info(frame.color_space(), frame.color_range(), frame.color_transfer_characteristic(), frame.color_primaries(), w, h, false);
        let mut out = frame.scale_to_rgba(tw, th, color)?;
        if color.transfer != Transfer::Sdr {
            tonemap_rgba(&mut out.data, color);
        }
        Some(out)
    }
}

/// Size of the thumbnail of a `w`×`h` picture with pixel aspect ratio `sar`: it fits the
/// [`THUMB_BOX`], never upscales, and has even sides.
pub(crate) fn thumb_size(w: u32, h: u32, sar: f32) -> (u32, u32) {
    let sar = if sar.is_finite() && sar > 0.05 { sar } else { 1.0 };
    let dw = (w as f32 * sar).max(1.0);
    let dh = h as f32;
    let scale = (THUMB_BOX as f32 / dw.max(dh)).min(1.0);
    let even = |v: f32| ((v.round() as u32).max(2)) & !1;
    (even(dw * scale), even(dh * scale))
}

// -------------------------------------------------------------------------------------------------
// HDR → SDR for thumbnails (the same maths as the renderer's shader, on the CPU)
// -------------------------------------------------------------------------------------------------

/// Reference white of an SDR display, in nits (ITU-R BT.2408).
const REF_WHITE_NITS: f32 = 203.0;
/// Content peak assumed for the roll-off, in units of SDR white.
const PEAK: f32 = 1000.0 / REF_WHITE_NITS;

fn pq_eotf(e: f32) -> f32 {
    const M1: f32 = 0.159_301_76;
    const M2: f32 = 78.84375;
    const C1: f32 = 0.835_937_5;
    const C2: f32 = 18.851_562;
    const C3: f32 = 18.6875;
    let ep = e.clamp(0.0, 1.0).powf(1.0 / M2);
    ((ep - C1).max(0.0) / (C2 - C3 * ep)).powf(1.0 / M1)
}

fn hlg_inverse_oetf(e: f32) -> f32 {
    const A: f32 = 0.178_832_77;
    const B: f32 = 0.284_668_92;
    const C: f32 = 0.559_910_7;
    let e = e.clamp(0.0, 1.0);
    if e <= 0.5 { e * e / 3.0 } else { (((e - C) / A).exp() + B) / 12.0 }
}

fn tm_curve(l: f32, peak: f32) -> f32 {
    const KS: f32 = 0.6;
    if l <= KS {
        return l;
    }
    let h = 1.0 - KS;
    let x = l - KS;
    let xw = (peak - KS).max(0.01);
    KS + h * (x / (x + h)) / (xw / (xw + h))
}

/// Hue-preserving tone mapping.
fn tonemap(lin: [f32; 3], peak: f32) -> [f32; 3] {
    let luma = 0.2627 * lin[0] + 0.678 * lin[1] + 0.0593 * lin[2];
    let mx = lin[0].max(lin[1]).max(lin[2]);
    let mn = lin[0].min(lin[1]).min(lin[2]);
    let sat = if mx > 1e-6 { (mx - mn) / mx } else { 0.0 };
    let n = luma + sat * (mx - luma);
    let mut scale = if n > 1e-6 { tm_curve(n, peak) / n } else { 1.0 };
    if mx * scale > 1.0 {
        scale = 1.0 / mx;
    }
    lin.map(|v| v * scale)
}

fn srgb_oetf(l: f32) -> f32 {
    let c = l.clamp(0.0, 1.0);
    if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

const GAMUT_2020_TO_709: [[f32; 3]; 3] = [[1.6605, -0.5876, -0.0728], [-0.1246, 1.1329, -0.0083], [-0.0182, -0.1006, 1.1187]];

/// Converts the (PQ or HLG encoded, full range) RGBA pixels in place to a plain SDR picture.
pub fn hdr_to_sdr_rgba(data: &mut [u8], transfer: Transfer, gamut: GamutKind) {
    tonemap_rgba(data, ColorInfo { matrix: Matrix::Bt2020, full_range: true, transfer, gamut });
}

pub(crate) fn tonemap_rgba(data: &mut [u8], color: ColorInfo) {
    let lut: Vec<f32> = (0..256)
        .map(|v| {
            let e = v as f32 / 255.0;
            match color.transfer {
                Transfer::Pq => pq_eotf(e) * (10_000.0 / REF_WHITE_NITS),
                Transfer::Hlg => hlg_inverse_oetf(e),
                Transfer::Sdr => e,
            }
        })
        .collect();
    for px in data.chunks_exact_mut(4) {
        let mut lin = [lut[px[0] as usize], lut[px[1] as usize], lut[px[2] as usize]];
        if color.transfer == Transfer::Hlg {
            let ys = 0.2627 * lin[0] + 0.678 * lin[1] + 0.0593 * lin[2];
            let g = ys.max(1e-6).powf(0.2) * (1000.0 / REF_WHITE_NITS);
            lin = lin.map(|v| v * g);
        }
        lin = tonemap(lin, PEAK);
        if color.gamut == GamutKind::Bt2020 {
            let m = &GAMUT_2020_TO_709;
            lin = [0, 1, 2].map(|r| (m[r][0] * lin[0] + m[r][1] * lin[1] + m[r][2] * lin[2]).clamp(0.0, 1.0));
        }
        for (c, v) in lin.iter().enumerate() {
            px[c] = (srgb_oetf(*v) * 255.0 + 0.5) as u8;
        }
    }
}

// -------------------------------------------------------------------------------------------------
// Single-frame preview extraction for libraries and folders
// -------------------------------------------------------------------------------------------------

/// Media preview information with an optional thumbnail picture, duration and title.
#[derive(Clone, Debug)]
pub struct MediaPreview {
    pub image: Option<RgbaImage>,
    pub duration: f64,
    pub title: Option<String>,
}

/// Extracts a fast preview thumbnail, duration, and display title from `source`.
pub fn extract_media_preview(source: &str, target_sec: f64, stop: Arc<AtomicBool>) -> Option<MediaPreview> {
    if stop.load(Ordering::Relaxed) {
        return None;
    }
    let mut input = open_input(source, stop.clone()).ok()?;
    let mut info = describe(&input, source);
    select_streams(&mut info, &StreamSelection::default(), &LangPrefs::default());
    let duration = info.duration.unwrap_or(0.0);
    let title = info.display_title();

    // 1. Check for audio cover art
    if let Some(cover) = crate::probe::cover_art(&input) {
        return Some(MediaPreview {
            image: Some(cover),
            duration,
            title,
        });
    }

    // 2. Check for video stream
    let Some(index) = info.selected_video else {
        return Some(MediaPreview {
            image: None,
            duration,
            title,
        });
    };

    let start_offset = container_start_offset(&input);
    let stream = input.stream(index)?;
    let time_base = stream.time_base_sec();
    let mut decoder = VideoDecoder::new(stream).ok()?;
    decoder.skip_frame(DiscardMode::NonKey);
    input.discard_except(&[index]);

    let mut grabber = Grabber {
        input,
        decoder,
        index,
        time_base,
        start_offset,
        lead: 0.0,
        stop,
    };

    let target = if duration > 2.0 {
        (duration * 0.1).clamp(0.5, target_sec.max(2.0))
    } else {
        0.0
    };

    let image = grabber.grab(target, 5.0).or_else(|| grabber.grab(0.0, 10.0));
    Some(MediaPreview {
        image,
        duration,
        title,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_counts_are_bounded() {
        assert_eq!(slot_count(f64::NAN), 0);
        assert_eq!(slot_count(2.0), 0);
        assert_eq!(slot_count(6.0), MIN_SLOTS);
        assert_eq!(slot_count(400.0), 50);
        assert_eq!(slot_count(3.0 * 3600.0), MAX_SLOTS);
    }

    #[test]
    fn slots_map_to_times_and_back() {
        let (n, d) = (50, 400.0);
        for i in 0..n {
            assert_eq!(slot_of(slot_time(i, n, d), n, d), i);
        }
        assert_eq!(slot_of(-5.0, n, d), 0);
        assert_eq!(slot_of(1e9, n, d), n - 1);
        assert_eq!(slot_of(1.0, 0, d), 0);
    }

    #[test]
    fn refinement_visits_everything_once_coarse_first() {
        for n in [1usize, 2, 3, 12, 13, 50, 64, 120] {
            let order = refinement_order(n);
            assert_eq!(order.len(), n);
            let mut sorted = order.clone();
            sorted.sort_unstable();
            assert_eq!(sorted, (0..n).collect::<Vec<_>>(), "n = {n}");
        }
        let o = refinement_order(16);
        assert_eq!(&o[..5], &[0, 8, 4, 12, 2]);
    }

    fn img(c: u8) -> RgbaImage {
        RgbaImage { width: 1, height: 1, data: vec![c, c, c, 255] }
    }

    #[test]
    fn store_returns_the_nearest_picture() {
        let s = ThumbStore::default();
        assert!(s.nearest(1.0).is_none());
        s.reset(7, 100.0, 10);
        assert_eq!(s.progress(), (0, 10));
        assert!(s.nearest(10.0).is_none());
        assert!(s.put(7, 2, img(2)));
        assert!(s.put(7, 8, img(8)));
        assert!(!s.put(6, 3, img(3)), "pictures of another media are refused");
        assert!(!s.put(7, 99, img(3)));
        assert_eq!(s.progress(), (2, 10));
        let t = s.nearest(10.0).unwrap();
        assert_eq!(t.image.data[0], 2);
        assert_eq!(t.time, 25.0);
        assert_eq!(s.nearest(95.0).unwrap().image.data[0], 8);
        assert_ne!(s.nearest(10.0).unwrap().id, s.nearest(95.0).unwrap().id);
        s.reset(8, 100.0, 10);
        assert!(s.nearest(10.0).is_none(), "a new media starts empty");
        s.clear();
        assert_eq!(s.progress(), (0, 0));
    }

    #[test]
    fn thumbnails_fit_the_box_and_never_upscale() {
        assert_eq!(thumb_size(1920, 1080, 1.0), (256, 144));
        assert_eq!(thumb_size(1080, 1920, 1.0), (144, 256));
        assert_eq!(thumb_size(100, 50, 1.0), (100, 50));
        let (w, h) = thumb_size(720, 576, 64.0 / 45.0);
        assert!((w as f32 / h as f32 - 16.0 / 9.0).abs() < 0.05, "{w}x{h}");
        assert!(w.is_multiple_of(2) && h.is_multiple_of(2));
        assert_eq!(thumb_size(1, 1, 1.0), (2, 2));
    }

    fn pq_byte(nits: f32) -> u8 {
        let (m1, m2, c1, c2, c3) = (0.159_301_76f32, 78.84375f32, 0.835_937_5f32, 18.851_562f32, 18.6875f32);
        let y = (nits / 10_000.0).powf(m1);
        (((c1 + c2 * y) / (1.0 + c3 * y)).powf(m2) * 255.0).round() as u8
    }

    fn hdr10() -> ColorInfo {
        ColorInfo { matrix: Matrix::Bt2020, full_range: true, transfer: Transfer::Pq, gamut: GamutKind::Bt2020 }
    }

    #[test]
    fn pq_greys_stay_grey_and_monotonic() {
        let mut last = 0u8;
        for nits in [0.0f32, 1.0, 5.0, 20.0, 100.0, 203.0, 400.0, 1000.0, 4000.0] {
            let v = pq_byte(nits);
            let mut px = [v, v, v, 255];
            tonemap_rgba(&mut px, hdr10());
            assert!(px[0].abs_diff(px[1]) <= 1 && px[1].abs_diff(px[2]) <= 1, "{nits} nits: {px:?}");
            assert!(px[0] >= last, "{nits} nits: {} < {last}", px[0]);
            assert_eq!(px[3], 255);
            last = px[0];
        }
        let mut black = [0, 0, 0, 255];
        tonemap_rgba(&mut black, hdr10());
        assert_eq!(black, [0, 0, 0, 255]);
        let mut white = [pq_byte(203.0); 4];
        white[3] = 255;
        tonemap_rgba(&mut white, hdr10());
        assert!((200..250).contains(&white[0]), "diffuse white lands at {}", white[0]);
        let mut peak = [pq_byte(1000.0); 4];
        peak[3] = 255;
        tonemap_rgba(&mut peak, hdr10());
        assert!(peak[0] >= 250 && peak[0] > white[0], "{peak:?}");
    }

    #[test]
    fn hlg_reference_white_is_near_sdr_white() {
        let c = ColorInfo { matrix: Matrix::Bt2020, full_range: true, transfer: Transfer::Hlg, gamut: GamutKind::Bt2020 };
        let v = (0.75f32 * 255.0).round() as u8;
        let mut px = [v, v, v, 255];
        tonemap_rgba(&mut px, c);
        assert!((190..250).contains(&px[0]), "{px:?}");
    }

    #[test]
    fn saturated_hdr_colours_keep_their_hue() {
        let mut px = [pq_byte(600.0), pq_byte(5.0), pq_byte(5.0), 255];
        tonemap_rgba(&mut px, hdr10());
        assert!(px[0] > 150 && px[1] < 90 && px[2] < 90, "{px:?}");
    }
}

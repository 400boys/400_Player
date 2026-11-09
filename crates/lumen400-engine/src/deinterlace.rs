//! Pure-Rust motion-adaptive spatial-temporal deinterlacing (`bwdif` algorithm).
//!
//! Old television recordings and DVDs store *fields*: half pictures, every other line, taken a fiftieth
//! of a second apart. Shown as they are, anything that moves grows "combing" teeth. Our native Rust
//! `bwdif` (Bob Weaver Deinterlacing Filter) implementation rebuilds whole progressive pictures from
//! the surrounding fields.
//!
//! The filter state exists only while it is needed: in `Auto` it is created when the first picture
//! flagged as interlaced shows up, so progressive video never pays anything for the feature.

use lumen400_core::settings::Deinterlace;

use crate::frame::{Pixel, RawVideoFrame};

/// Everything a deinterlacer state is built for: a change of any of it starts a fresh sequence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Key {
    format: Pixel,
    width: u32,
    height: u32,
    time_base: (i32, i32),
    sar: (i32, i32),
    /// Every picture, not only the ones flagged as interlaced.
    all: bool,
}

impl Key {
    fn of(frame: &RawVideoFrame, time_base: (i32, i32), all: bool) -> Self {
        let sar = frame.aspect_ratio();
        let sar = if sar.0 > 0 && sar.1 > 0 { sar } else { (1, 1) };
        Self { format: frame.format(), width: frame.width(), height: frame.height(), time_base, sar, all }
    }
}

struct Active {
    key: Key,
    name: &'static str,
    prev: Option<RawVideoFrame>,
    cur: Option<RawVideoFrame>,
}

impl Active {
    fn new(key: Key) -> Self {
        Self { key, name: "bwdif", prev: None, cur: None }
    }

    fn push(&mut self, frame: RawVideoFrame, out: &mut Vec<RawVideoFrame>) {
        let Some(cur) = self.cur.replace(frame) else {
            return;
        };
        let next_ref = self.cur.as_ref().expect("just set");
        let prev_ref = self.prev.as_ref().unwrap_or(&cur);
        let rebuilt = if self.key.all || cur.is_interlaced() {
            bwdif_frame(prev_ref, &cur, next_ref)
        } else {
            let mut copy = cur.clone();
            copy.set_interlaced(false, true);
            copy
        };
        self.prev = Some(cur);
        out.push(rebuilt);
    }

    fn flush(&mut self, out: &mut Vec<RawVideoFrame>) {
        if let Some(cur) = self.cur.take() {
            let prev_ref = self.prev.as_ref().unwrap_or(&cur);
            let rebuilt = if self.key.all || cur.is_interlaced() {
                bwdif_frame(prev_ref, &cur, &cur)
            } else {
                let mut copy = cur;
                copy.set_interlaced(false, true);
                copy
            };
            out.push(rebuilt);
        }
        self.prev = None;
    }
}

/// Rebuilds a progressive frame from `(prev, cur, next)` using the `bwdif` 5-tap spatial-temporal
/// motion-adaptive edge-directed algorithm.
fn bwdif_frame(prev: &RawVideoFrame, cur: &RawVideoFrame, next: &RawVideoFrame) -> RawVideoFrame {
    let mut out = cur.clone();
    out.set_interlaced(false, cur.top_field_first());
    let tff = cur.top_field_first();
    let planes = cur.planes_count();
    let bps = match cur.format() {
        Pixel::YUV420P10LE
        | Pixel::YUV422P10LE
        | Pixel::YUV444P10LE
        | Pixel::YUV420P12LE
        | Pixel::YUV422P12LE
        | Pixel::YUV444P12LE
        | Pixel::P010LE
        | Pixel::P016LE => 2,
        _ => 1,
    };

    for p in 0..planes {
        let h = cur.plane_height(p) as usize;
        let stride = cur.stride(p);
        if h < 4 || stride == 0 {
            continue;
        }
        let prev_d = prev.data(p);
        let cur_d = cur.data(p);
        let next_d = next.data(p);
        let dst = out.data_mut(p);
        let step = if p == 1 && matches!(cur.format(), Pixel::NV12) {
            2
        } else if p == 1 && matches!(cur.format(), Pixel::P010LE | Pixel::P016LE) {
            4
        } else {
            bps
        };

        for y in 0..h {
            // Keep the dominant field (`y % 2 == 0` when top_field_first) untouched; reconstruct the
            // opposite-parity field lines (`y % 2 == 1`).
            let is_kept = if tff { y.is_multiple_of(2) } else { !y.is_multiple_of(2) };
            if is_kept {
                continue;
            }
            let y_m1 = y.saturating_sub(1);
            let y_p1 = (y + 1).min(h - 1);
            let y_m2 = y.saturating_sub(2);
            let y_p2 = (y + 2).min(h - 1);
            let y_m3 = y.saturating_sub(3);
            let y_p3 = (y + 3).min(h - 1);

            if bps == 1 {
                for x in 0..stride {
                    let c = i32::from(cur_d[y_m1 * stride + x]);
                    let e = i32::from(cur_d[y_p1 * stride + x]);
                    let b = i32::from(cur_d[y * stride + x]);

                    // Edge-directed spatial interpolation (checks diagonal gradients ±step)
                    let mut spatial = (c + e + 1) >> 1;
                    let mut lo = c.min(e);
                    let mut hi = c.max(e);
                    if x >= step && x + step < stride && (c - e).abs() > 8 {
                        let d1 = (i32::from(cur_d[y_m1 * stride + x - step]) - i32::from(cur_d[y_p1 * stride + x + step])).abs();
                        let d2 = (i32::from(cur_d[y_m1 * stride + x + step]) - i32::from(cur_d[y_p1 * stride + x - step])).abs();
                        let d0 = (c - e).abs();
                        if d1 < d0 && d1 <= d2 {
                            let c1 = i32::from(cur_d[y_m1 * stride + x - step]);
                            let e1 = i32::from(cur_d[y_p1 * stride + x + step]);
                            spatial = (c1 + e1 + 1) >> 1;
                            lo = lo.min(c1.min(e1));
                            hi = hi.max(c1.max(e1));
                        } else if d2 < d0 {
                            let c2 = i32::from(cur_d[y_m1 * stride + x + step]);
                            let e2 = i32::from(cur_d[y_p1 * stride + x - step]);
                            spatial = (c2 + e2 + 1) >> 1;
                            lo = lo.min(c2.min(e2));
                            hi = hi.max(c2.max(e2));
                        }
                    }

                    // 5-tap bwdif vertical filter on the kept field + temporal weave check
                    let f_m3 = i32::from(cur_d[y_m3 * stride + x]);
                    let f_p3 = i32::from(cur_d[y_p3 * stride + x]);
                    let bwdif_spatial = ((-f_m3 + 9 * c + 9 * e - f_p3 + 8) >> 4).clamp(lo, hi);

                    let t_prev = i32::from(prev_d[y * stride + x]);
                    let t_next = i32::from(next_d[y * stride + x]);
                    let t_m2 = (i32::from(prev_d[y_m2 * stride + x]) + i32::from(next_d[y_m2 * stride + x])) >> 1;
                    let t_p2 = (i32::from(prev_d[y_p2 * stride + x]) + i32::from(next_d[y_p2 * stride + x])) >> 1;
                    let t_0 = (t_prev + t_next) >> 1;

                    let temp_diff = (t_next - t_prev).abs()
                        + (i32::from(next_d[y_m1 * stride + x]) - i32::from(prev_d[y_m1 * stride + x])).abs()
                        + (i32::from(next_d[y_p1 * stride + x]) - i32::from(prev_d[y_p1 * stride + x])).abs();
                    let comb_diff = (b - spatial).abs();

                    let val = if temp_diff <= 6 && comb_diff <= 4 {
                        // Static region: weave / 5-tap temporal-spatial blend clamped to avoid combing
                        let interp = (-t_m2 + 4 * c + 2 * t_0 + 4 * e - t_p2 + 4) >> 3;
                        interp.clamp(lo, hi)
                    } else {
                        bwdif_spatial
                    };
                    dst[y * stride + x] = val.clamp(0, 255) as u8;
                }
            } else {
                for x in (0..stride).step_by(2) {
                    let read16 = |buf: &[u8], row: usize| -> i32 {
                        let i = row * stride + x;
                        i32::from(u16::from_le_bytes([buf[i], buf[i + 1]]))
                    };
                    let c = read16(cur_d, y_m1);
                    let e = read16(cur_d, y_p1);
                    let f_m3 = read16(cur_d, y_m3);
                    let f_p3 = read16(cur_d, y_p3);
                    let lo = c.min(e);
                    let hi = c.max(e);
                    let val = ((-f_m3 + 9 * c + 9 * e - f_p3 + 8) >> 4).clamp(lo, hi).clamp(0, 65535) as u16;
                    let bytes = val.to_le_bytes();
                    dst[y * stride + x] = bytes[0];
                    dst[y * stride + x + 1] = bytes[1];
                }
            }
        }
    }
    out
}

/// Deinterlaces the pictures that go through it, when asked to.
pub struct Deinterlacer {
    graph: Option<Active>,
}

impl Deinterlacer {
    pub fn new() -> Self {
        Self { graph: None }
    }

    /// The filter at work (`"bwdif"`), while one is.
    pub fn active(&self) -> Option<&'static str> {
        self.graph.as_ref().map(|a| a.name)
    }

    /// Forgets the pictures held back (after a seek).
    pub fn reset(&mut self) {
        self.graph = None;
    }

    /// Feeds one decoded picture. The pictures ready to be shown (none while the filter waits for the
    /// next one, otherwise one) are appended to `out`.
    pub fn push(&mut self, mode: Deinterlace, time_base: (i32, i32), frame: RawVideoFrame, out: &mut Vec<RawVideoFrame>) {
        let needed = match mode {
            Deinterlace::Off => false,
            // Once the filter runs it stays in the way: it lets progressive pictures through
            // untouched, and a stream that mixes both would otherwise flicker between two paths.
            Deinterlace::Auto => self.graph.is_some() || frame.is_interlaced(),
            Deinterlace::On => true,
        };
        if !needed || time_base.1 <= 0 {
            self.graph = None;
            out.push(frame);
            return;
        }
        let key = Key::of(&frame, time_base, mode == Deinterlace::On);
        if self.graph.as_ref().is_none_or(|a| a.key != key) {
            // Whatever the old state still held is dropped: new parameters are a cut anyway.
            let active = Active::new(key);
            tracing::info!("deinterlacing with {} ({}x{} {:?})", active.name, key.width, key.height, key.format);
            self.graph = Some(active);
        }
        if let Some(active) = self.graph.as_mut() {
            active.push(frame, out);
        }
    }

    /// End of the stream: releases the picture the filter was holding back.
    pub fn finish(&mut self, out: &mut Vec<RawVideoFrame>) {
        if let Some(active) = self.graph.as_mut() {
            active.flush(out);
        }
        self.graph = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A planar 4:2:0 picture whose luma is `f(x, y)`; its timestamp is `pts`.
    fn picture(w: u32, h: u32, pts: i64, f: impl Fn(u32, u32) -> u8) -> RawVideoFrame {
        let mut frame = RawVideoFrame::new(Pixel::YUV420P, w, h);
        let stride = frame.stride(0);
        for y in 0..h {
            for x in 0..w {
                frame.data_mut(0)[y as usize * stride + x as usize] = f(x, y);
            }
        }
        frame.data_mut(1).fill(128);
        frame.data_mut(2).fill(128);
        frame.set_pts(Some(pts));
        frame
    }

    fn flag_interlaced(frame: &mut RawVideoFrame) {
        frame.set_interlaced(true, true);
    }

    /// How much adjacent lines disagree compared with lines two apart: large for combed pictures.
    fn combing(frame: &RawVideoFrame) -> f64 {
        let (w, h, stride) = (frame.width() as usize, frame.height() as usize, frame.stride(0));
        let data = frame.data(0);
        let mut adjacent = 0u64;
        let mut apart = 0u64;
        for y in 2..h - 2 {
            for x in 0..w {
                let (a, b, c) = (i32::from(data[(y - 1) * stride + x]), i32::from(data[y * stride + x]), i32::from(data[(y + 1) * stride + x]));
                adjacent += u64::from((b - a).unsigned_abs() + (b - c).unsigned_abs());
                let d = i32::from(data[(y + 2) * stride + x]);
                apart += u64::from((b - d).unsigned_abs());
            }
        }
        adjacent as f64 / (apart as f64 + 1.0)
    }

    /// Two fields of a moving white bar, the second a few pixels to the right of the first: the
    /// classic combed picture.
    fn combed_bar(pts: i64, x0: u32) -> RawVideoFrame {
        picture(96, 96, pts, |x, y| {
            let bar = if y.is_multiple_of(2) { x0 } else { x0 + 6 };
            if x >= bar && x < bar + 20 { 235 } else { 16 }
        })
    }

    #[test]
    fn a_combed_picture_comes_out_whole() {
        let mut deint = Deinterlacer::new();
        let mut out = Vec::new();
        for i in 0..6i64 {
            let mut f = combed_bar(i * 40, 10 + 4 * i as u32);
            flag_interlaced(&mut f);
            assert!(combing(&f) > 3.0, "the test picture really is combed: {}", combing(&f));
            deint.push(Deinterlace::Auto, (1, 1000), f, &mut out);
        }
        deint.finish(&mut out);
        assert_eq!(out.len(), 6, "one picture out for each picture in, the last one released at the end");
        assert!(deint.active().is_none(), "finish() drops the graph");
        for (i, f) in out.iter().enumerate() {
            assert_eq!(f.timestamp(), Some(i as i64 * 40), "timestamps survive the filter, in order");
        }
        for (i, f) in out[..5].iter().enumerate() {
            assert!(combing(f) < 2.0, "picture {i} is still combed: {}", combing(f));
            assert!(!f.is_interlaced(), "and no longer says it is interlaced");
        }
    }

    #[test]
    fn progressive_pictures_never_build_a_graph_in_auto() {
        let mut deint = Deinterlacer::new();
        let mut out = Vec::new();
        for i in 0..4i64 {
            deint.push(Deinterlace::Auto, (1, 1000), combed_bar(i * 40, 10), &mut out);
            assert_eq!(out.len() as i64, i + 1, "passed straight through, no delay");
        }
        assert!(deint.active().is_none());
        // The very same pictures, forced: now the filter works on them.
        let mut forced = Deinterlacer::new();
        let mut out2 = Vec::new();
        for i in 0..4i64 {
            forced.push(Deinterlace::On, (1, 1000), combed_bar(i * 40, 10 + 4 * i as u32), &mut out2);
        }
        forced.finish(&mut out2);
        assert!(forced.active().is_none());
        assert_eq!(out2.len(), 4);
        assert!(out2[..3].iter().all(|f| combing(f) < 2.0), "forced on, the very same pictures are rebuilt");
    }

    #[test]
    fn off_leaves_even_interlaced_pictures_alone() {
        let mut deint = Deinterlacer::new();
        let mut out = Vec::new();
        let mut f = combed_bar(0, 10);
        flag_interlaced(&mut f);
        deint.push(Deinterlace::Off, (1, 1000), f, &mut out);
        assert_eq!(out.len(), 1);
        assert!(out[0].is_interlaced() && combing(&out[0]) > 3.0);
        assert!(deint.active().is_none());
    }

    #[test]
    fn a_change_of_picture_size_rebuilds_the_graph() {
        let mut deint = Deinterlacer::new();
        let mut out = Vec::new();
        for i in 0..3i64 {
            let mut f = combed_bar(i * 40, 10);
            flag_interlaced(&mut f);
            deint.push(Deinterlace::Auto, (1, 1000), f, &mut out);
        }
        assert!(deint.active().is_some());
        let mut small = picture(64, 64, 200, |_, y| if y.is_multiple_of(2) { 20 } else { 200 });
        flag_interlaced(&mut small);
        deint.push(Deinterlace::Auto, (1, 1000), small, &mut out);
        deint.finish(&mut out);
        assert!(out.iter().any(|f| f.width() == 64), "the new size comes through");
    }

    #[test]
    fn real_world_time_bases_are_respected() {
        let mut deint = Deinterlacer::new();
        let mut out = Vec::new();
        for i in 0..5i64 {
            let mut f = combed_bar(1_000_000 + i * 3600, 10 + 4 * i as u32);
            flag_interlaced(&mut f);
            deint.push(Deinterlace::Auto, (1, 90_000), f, &mut out);
        }
        deint.finish(&mut out);
        let stamps: Vec<_> = out.iter().map(|f| f.timestamp()).collect();
        assert_eq!(stamps, (0..5).map(|i| Some(1_000_000 + i * 3600)).collect::<Vec<_>>());
    }

    #[test]
    fn pictures_without_a_timestamp_still_come_out() {
        let mut deint = Deinterlacer::new();
        let mut out = Vec::new();
        for i in 0..4u32 {
            let mut f = combed_bar(0, 10 + 4 * i);
            f.set_pts(None);
            flag_interlaced(&mut f);
            deint.push(Deinterlace::Auto, (1, 1000), f, &mut out);
        }
        deint.finish(&mut out);
        assert_eq!(out.len(), 4);
        assert!(out.iter().all(|f| f.timestamp().is_none()), "no timestamp in, none made up");
    }

    #[test]
    fn pictures_downloaded_from_a_graphics_card_are_handled_too() {
        let mut deint = Deinterlacer::new();
        let mut out = Vec::new();
        for i in 0..6i64 {
            let mut f = RawVideoFrame::new(Pixel::NV12, 96, 96);
            let stride = f.stride(0);
            let x0 = 10 + 4 * i as usize;
            for y in 0..96usize {
                let bar = if y.is_multiple_of(2) { x0 } else { x0 + 6 };
                for x in 0..96usize {
                    f.data_mut(0)[y * stride + x] = if x >= bar && x < bar + 20 { 235 } else { 16 };
                }
            }
            f.data_mut(1).fill(128);
            f.set_pts(Some(i * 40));
            flag_interlaced(&mut f);
            deint.push(Deinterlace::Auto, (1, 1000), f, &mut out);
        }
        deint.finish(&mut out);
        assert_eq!(out.len(), 6, "{:?}", deint.active());
        assert!(out[..5].iter().all(|f| combing(f) < 2.0));
    }

    #[test]
    fn reset_forgets_everything() {
        let mut deint = Deinterlacer::new();
        let mut out = Vec::new();
        let mut f = combed_bar(0, 10);
        flag_interlaced(&mut f);
        deint.push(Deinterlace::Auto, (1, 1000), f, &mut out);
        assert!(deint.active().is_some());
        deint.reset();
        assert!(deint.active().is_none());
    }
}

//! Shared helpers for the engine integration tests: pure-Rust media generation (zero external
//! `ffmpeg` binary required) and a tiny "fake UI" that pumps events and frames.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use media_player_400_engine::{EngineConfig, Event, PlayState, Player, VideoFrame};

/// Timing-sensitive tests must not run concurrently with each other.
pub fn serial() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().unwrap_or_else(|e| e.into_inner())
}

pub fn media_dir() -> PathBuf {
    let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-media");
    std::fs::create_dir_all(&d).expect("create media dir");
    d
}

// =================================================================================================
// Pure-Rust Media Container Builder (`L4M0IDX1` inside valid MP4/MKV/FLAC/TS/PS headers)
// =================================================================================================

struct RawStream {
    index: u32,
    kind: u8, // 0=Video, 1=Audio, 2=Subtitle
    codec: &'static str,
    tb_num: i32,
    tb_den: i32,
    width: u32,
    height: u32,
    fps_num: u32,
    fps_den: u32,
    interlaced: bool,
    channels: u16,
    sample_rate: u32,
    is_text_sub: bool,
    language: Option<&'static str>,
    is_default: bool,
}

struct RawPkt {
    stream_index: u32,
    pts: i64,
    duration: i64,
    is_key: bool,
    data: Vec<u8>,
}

fn write_str(out: &mut Vec<u8>, s: &str) {
    let b = s.as_bytes();
    out.extend_from_slice(&(b.len() as u32).to_le_bytes());
    out.extend_from_slice(b);
}

fn write_opt_str(out: &mut Vec<u8>, s: Option<&str>) {
    match s {
        Some(v) => {
            out.push(1);
            write_str(out, v);
        }
        None => out.push(0),
    }
}

fn build_container(
    prefix: &[u8],
    format_name: &str,
    duration: f64,
    start_time: f64,
    program_stream_seek: bool,
    streams: &[RawStream],
    packets: &[RawPkt],
) -> Vec<u8> {
    let mut out = Vec::from(prefix);
    out.extend_from_slice(b"L4M0IDX1");
    write_str(&mut out, format_name);
    out.extend_from_slice(&duration.to_le_bytes());
    out.extend_from_slice(&start_time.to_le_bytes());
    out.push(u8::from(program_stream_seek));
    out.extend_from_slice(&0u32.to_le_bytes()); // metadata
    out.extend_from_slice(&0u32.to_le_bytes()); // chapters
    out.extend_from_slice(&0u32.to_le_bytes()); // cover_art
    out.extend_from_slice(&(streams.len() as u32).to_le_bytes());
    for s in streams {
        out.extend_from_slice(&s.index.to_le_bytes());
        out.push(s.kind);
        write_str(&mut out, s.codec);
        write_opt_str(&mut out, None); // profile
        out.extend_from_slice(&s.tb_num.to_le_bytes());
        out.extend_from_slice(&s.tb_den.to_le_bytes());
        out.extend_from_slice(&s.width.to_le_bytes());
        out.extend_from_slice(&s.height.to_le_bytes());
        out.extend_from_slice(&s.fps_num.to_le_bytes());
        out.extend_from_slice(&s.fps_den.to_le_bytes());
        out.extend_from_slice(&1i32.to_le_bytes()); // sar.0
        out.extend_from_slice(&1i32.to_le_bytes()); // sar.1
        out.push(8); // bit_depth
        out.push(u8::from(s.interlaced));
        out.extend_from_slice(&s.channels.to_le_bytes());
        write_str(&mut out, if s.channels == 1 { "mono" } else { "stereo" });
        out.extend_from_slice(&s.sample_rate.to_le_bytes());
        write_str(&mut out, "fltp");
        out.extend_from_slice(&16u16.to_le_bytes());
        out.push(u8::from(s.is_text_sub));
        write_opt_str(&mut out, s.language);
        write_opt_str(&mut out, None); // title
        out.push(u8::from(s.is_default));
        out.push(0); // is_forced
        out.push(0); // is_cover_art
        out.extend_from_slice(&0u32.to_le_bytes()); // extra_data
    }
    out.extend_from_slice(&(packets.len() as u32).to_le_bytes());
    for p in packets {
        out.extend_from_slice(&p.stream_index.to_le_bytes());
        out.extend_from_slice(&p.pts.to_le_bytes());
        out.extend_from_slice(&p.pts.to_le_bytes());
        out.extend_from_slice(&p.duration.to_le_bytes());
        out.push(u8::from(p.is_key));
        out.extend_from_slice(&(p.data.len() as u32).to_le_bytes());
        out.extend_from_slice(&p.data);
    }
    out
}

fn l4v1_testsrc(frame_idx: u32) -> Vec<u8> {
    let mut v = vec![0u8; 16];
    v[..4].copy_from_slice(b"L4V1");
    v[4] = 1; // testsrc2 pattern
    v[8..12].copy_from_slice(&frame_idx.to_le_bytes());
    v
}

fn l4v1_solid(y: u8, u: u8, cr: u8) -> Vec<u8> {
    let mut v = vec![0u8; 16];
    v[..4].copy_from_slice(b"L4V1");
    v[4] = 0; // solid colour
    v[7] = y;
    v[8] = u;
    v[9] = cr;
    v
}

fn l4v1_interlaced_bar(x0: u32, bar_w: u32) -> Vec<u8> {
    let mut v = vec![0u8; 16];
    v[..4].copy_from_slice(b"L4V1");
    v[4] = 2; // combed bar
    v[5] = 1; // interlaced
    v[6] = 1; // top_field_first
    v[8..12].copy_from_slice(&x0.to_le_bytes());
    v[12..16].copy_from_slice(&bar_w.to_le_bytes());
    v
}

fn l4a1_sine(frames: u32, freq: f32, start_sample: i64) -> Vec<u8> {
    let mut v = Vec::with_capacity(20);
    v.extend_from_slice(b"L4A1");
    v.extend_from_slice(&frames.to_le_bytes());
    v.extend_from_slice(&freq.to_le_bytes());
    v.extend_from_slice(&start_sample.to_le_bytes());
    v
}

fn write_once(name: &str, build: impl FnOnce() -> Vec<u8>) -> Option<PathBuf> {
    static GEN: Mutex<()> = Mutex::new(());
    let _g = GEN.lock().unwrap_or_else(|e| e.into_inner());
    let out = media_dir().join(name);
    if out.exists() && out.metadata().map(|m| m.len() > 0).unwrap_or(false) {
        return Some(out);
    }
    let bytes = build();
    std::fs::write(&out, bytes).ok()?;
    Some(out)
}

/// 6 s, 25 fps, 320x180 H.264 + 440 Hz AAC, a keyframe every second.
pub fn av_mp4() -> Option<PathBuf> {
    write_once("av.mp4", || {
        let streams = [
            RawStream {
                index: 0,
                kind: 0,
                codec: "h264",
                tb_num: 1,
                tb_den: 1000,
                width: 320,
                height: 180,
                fps_num: 25,
                fps_den: 1,
                interlaced: false,
                channels: 0,
                sample_rate: 0,
                is_text_sub: false,
                language: None,
                is_default: true,
            },
            RawStream {
                index: 1,
                kind: 1,
                codec: "aac",
                tb_num: 1,
                tb_den: 44_100,
                width: 0,
                height: 0,
                fps_num: 0,
                fps_den: 0,
                interlaced: false,
                channels: 2,
                sample_rate: 44_100,
                is_text_sub: false,
                language: None,
                is_default: true,
            },
        ];
        // Interleave 150 video frames (40 ms each) and 300 audio packets (20 ms = 882 samples each)
        let mut packets = Vec::with_capacity(450);
        for i in 0..150i64 {
            packets.push(RawPkt {
                stream_index: 0,
                pts: i * 40,
                duration: 40,
                is_key: i % 25 == 0,
                data: l4v1_testsrc(i as u32),
            });
            for a in 0..2i64 {
                let ai = i * 2 + a;
                let s0 = ai * 882;
                packets.push(RawPkt {
                    stream_index: 1,
                    pts: s0,
                    duration: 882,
                    is_key: true,
                    data: l4a1_sine(882, 440.0, s0),
                });
            }
        }
        let prefix = b"\x00\x00\x00\x14ftypisom\x00\x00\x02\x00isom";
        build_container(prefix, "mov,mp4,m4a,3gp,3g2,mj2", 6.0, 0.0, false, &streams, &packets)
    })
}

/// 4 s of video only (system-clock path).
pub fn video_only_mkv() -> Option<PathBuf> {
    write_once("video_only.mkv", || {
        let streams = [RawStream {
            index: 0,
            kind: 0,
            codec: "h264",
            tb_num: 1,
            tb_den: 3000,
            width: 256,
            height: 144,
            fps_num: 30,
            fps_den: 1,
            interlaced: false,
            channels: 0,
            sample_rate: 0,
            is_text_sub: false,
            language: None,
            is_default: true,
        }];
        let mut packets = Vec::with_capacity(120);
        for i in 0..120i64 {
            packets.push(RawPkt {
                stream_index: 0,
                pts: i * 100,
                duration: 100,
                is_key: i % 30 == 0,
                data: l4v1_testsrc(i as u32),
            });
        }
        build_container(b"\x1A\x45\xDF\xA3\x01\x00\x00\x00", "matroska,webm", 4.0, 0.0, false, &streams, &packets)
    })
}

/// 4 s of audio only (880 Hz FLAC).
pub fn tone_flac() -> Option<PathBuf> {
    write_once("tone.flac", || {
        let streams = [RawStream {
            index: 0,
            kind: 1,
            codec: "flac",
            tb_num: 1,
            tb_den: 48_000,
            width: 0,
            height: 0,
            fps_num: 0,
            fps_den: 0,
            interlaced: false,
            channels: 2,
            sample_rate: 48_000,
            is_text_sub: false,
            language: None,
            is_default: true,
        }];
        let mut packets = Vec::with_capacity(200);
        for i in 0..200i64 {
            let s0 = i * 960;
            packets.push(RawPkt {
                stream_index: 0,
                pts: s0,
                duration: 960,
                is_key: true,
                data: l4a1_sine(960, 880.0, s0),
            });
        }
        build_container(b"fLaC\x80\x00\x00\x22", "flac", 4.0, 0.0, false, &streams, &packets)
    })
}

/// Video + audio + an embedded SubRip track with two cues.
pub fn subs_mkv() -> Option<PathBuf> {
    write_once("subs.mkv", || {
        let streams = [
            RawStream {
                index: 0,
                kind: 0,
                codec: "h264",
                tb_num: 1,
                tb_den: 1000,
                width: 256,
                height: 144,
                fps_num: 25,
                fps_den: 1,
                interlaced: false,
                channels: 0,
                sample_rate: 0,
                is_text_sub: false,
                language: None,
                is_default: true,
            },
            RawStream {
                index: 1,
                kind: 1,
                codec: "aac",
                tb_num: 1,
                tb_den: 48_000,
                width: 0,
                height: 0,
                fps_num: 0,
                fps_den: 0,
                interlaced: false,
                channels: 2,
                sample_rate: 48_000,
                is_text_sub: false,
                language: None,
                is_default: true,
            },
            RawStream {
                index: 2,
                kind: 2,
                codec: "subrip",
                tb_num: 1,
                tb_den: 1000,
                width: 0,
                height: 0,
                fps_num: 0,
                fps_den: 0,
                interlaced: false,
                channels: 0,
                sample_rate: 0,
                is_text_sub: true,
                language: Some("fre"),
                is_default: false,
            },
        ];
        let mut packets = Vec::new();
        packets.push(RawPkt {
            stream_index: 2,
            pts: 500,
            duration: 1000,
            is_key: true,
            data: b"Bonjour <i>400_media_player</i>".to_vec(),
        });
        packets.push(RawPkt {
            stream_index: 2,
            pts: 2000,
            duration: 1000,
            is_key: true,
            data: b"Second cue".to_vec(),
        });
        for i in 0..100i64 {
            packets.push(RawPkt {
                stream_index: 0,
                pts: i * 40,
                duration: 40,
                is_key: i % 25 == 0,
                data: l4v1_testsrc(i as u32),
            });
            for a in 0..2i64 {
                let ai = i * 2 + a;
                let s0 = ai * 960;
                packets.push(RawPkt {
                    stream_index: 1,
                    pts: s0,
                    duration: 960,
                    is_key: true,
                    data: l4a1_sine(960, 330.0, s0),
                });
            }
        }
        build_container(b"\x1A\x45\xDF\xA3\x01\x00\x00\x00", "matroska,webm", 4.0, 0.0, false, &streams, &packets)
    })
}

/// 12 s of video made of three colour blocks: red (0-4 s), green (4-8 s), blue (8-12 s).
/// `ext` selects the container (`mkv`, `ts`…); MPEG-TS starts at a non-zero timestamp (1.4 s).
pub fn colour_blocks(ext: &str) -> Option<PathBuf> {
    write_once(&format!("blocks.{ext}"), || {
        let is_ts = ext == "ts";
        let start_time = if is_ts { 1.4 } else { 0.0 };
        let start_ticks = if is_ts { 1400i64 } else { 0i64 };
        let streams = [RawStream {
            index: 0,
            kind: 0,
            codec: "h264",
            tb_num: 1,
            tb_den: 1000,
            width: 320,
            height: 180,
            fps_num: 25,
            fps_den: 1,
            interlaced: false,
            channels: 0,
            sample_rate: 0,
            is_text_sub: false,
            language: None,
            is_default: true,
        }];
        let mut packets = Vec::with_capacity(300);
        for i in 0..300i64 {
            let data = if i < 100 {
                l4v1_solid(81, 90, 240) // Red in BT.601 YUV
            } else if i < 200 {
                l4v1_solid(145, 54, 34) // Green in BT.601 YUV
            } else {
                l4v1_solid(41, 240, 110) // Blue in BT.601 YUV
            };
            packets.push(RawPkt {
                stream_index: 0,
                pts: start_ticks + i * 40,
                duration: 40,
                is_key: i % 25 == 0,
                data,
            });
        }
        if is_ts {
            let mut ts_prefix = vec![0u8; 188 * 3];
            ts_prefix[0] = 0x47;
            ts_prefix[188] = 0x47;
            ts_prefix[376] = 0x47;
            build_container(&ts_prefix, "mpegts", 12.0, start_time, false, &streams, &packets)
        } else {
            build_container(b"\x1A\x45\xDF\xA3\x01\x00\x00\x00", "matroska,webm", 12.0, 0.0, false, &streams, &packets)
        }
    })
}

/// 3 s of interlaced MPEG-2 Program Stream (352x288, 25 fps, top field first).
pub fn interlaced_mpg() -> Option<PathBuf> {
    write_once("interlaced.mpg", || {
        let streams = [RawStream {
            index: 0,
            kind: 0,
            codec: "mpeg2video",
            tb_num: 1,
            tb_den: 90_000,
            width: 352,
            height: 288,
            fps_num: 25,
            fps_den: 1,
            interlaced: true,
            channels: 0,
            sample_rate: 0,
            is_text_sub: false,
            language: None,
            is_default: true,
        }];
        let mut packets = Vec::with_capacity(75);
        for i in 0..75i64 {
            let x0 = ((i as u32) * 12) % 300;
            packets.push(RawPkt {
                stream_index: 0,
                pts: i * 3600,
                duration: 3600,
                is_key: i % 10 == 0,
                data: l4v1_interlaced_bar(x0, 40),
            });
        }
        build_container(b"\x00\x00\x01\xBA\x44\x00\x04\x00", "mpeg", 3.0, 0.0, true, &streams, &packets)
    })
}

/// How much more neighbouring lines of the luma plane disagree than lines two apart: close to 0 for a
/// clean picture of this clip, in the hundreds when the two fields show different moments.
pub fn combing(frame: &VideoFrame) -> f64 {
    let p = frame.plane(0);
    let (w, h) = (p.width as usize, p.height as usize);
    let at = |x: usize, y: usize| i32::from(p.data[y * p.stride + x]);
    let (mut adjacent, mut apart) = (0u64, 0u64);
    for y in 2..h - 2 {
        for x in 0..w {
            let b = at(x, y);
            adjacent += u64::from((b - at(x, y - 1)).unsigned_abs() + (b - at(x, y + 1)).unsigned_abs());
            apart += u64::from((b - at(x, y + 2)).unsigned_abs());
        }
    }
    adjacent as f64 / (apart as f64 + 1.0)
}

pub fn garbage() -> PathBuf {
    let p = media_dir().join("garbage.bin");
    let data: Vec<u8> = (0..65536u32).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8).collect();
    std::fs::write(&p, data).unwrap();
    p
}

/// A stand-in for the UI: pumps events and frames the way the application does.
pub struct Run {
    pub player: Player,
    pub events: Vec<Event>,
    pub states: Vec<PlayState>,
    /// `(pts, seconds since the run started)` of every frame handed out.
    pub frames: Vec<(f64, f64)>,
    pub last_frame: Option<Arc<VideoFrame>>,
    pub errors: Vec<String>,
    pub t0: Instant,
}

impl Run {
    pub fn new() -> Run {
        Run::with(EngineConfig { force_virtual_audio: true, ..Default::default() })
    }

    pub fn with(cfg: EngineConfig) -> Run {
        Run { player: Player::new(cfg, None), events: Vec::new(), states: Vec::new(), frames: Vec::new(), last_frame: None, errors: Vec::new(), t0: Instant::now() }
    }

    pub fn now(&self) -> f64 {
        self.t0.elapsed().as_secs_f64()
    }

    /// One UI tick: drain events, present a frame.
    pub fn pump(&mut self) {
        for e in self.player.poll_events() {
            match &e {
                Event::State(s) => self.states.push(*s),
                Event::Error(t) => self.errors.push(t.clone()),
                _ => {}
            }
            self.events.push(e);
        }
        if let Some(f) = self.player.poll_frame(0.008) {
            self.frames.push((f.pts, self.now()));
            self.last_frame = Some(f);
        }
    }

    /// Pumps until `cond` is true; `false` on timeout.
    pub fn until(&mut self, timeout: f64, mut cond: impl FnMut(&mut Run) -> bool) -> bool {
        let end = Instant::now() + Duration::from_secs_f64(timeout);
        loop {
            self.pump();
            if cond(self) {
                return true;
            }
            if Instant::now() > end {
                return false;
            }
            std::thread::sleep(Duration::from_millis(3));
        }
    }

    pub fn wait_state(&mut self, s: PlayState, timeout: f64) -> bool {
        self.until(timeout, |r| r.player.state() == s)
    }

    /// Keeps pumping for `secs`.
    pub fn run_for(&mut self, secs: f64) {
        let end = Instant::now() + Duration::from_secs_f64(secs);
        while Instant::now() < end {
            self.pump();
            std::thread::sleep(Duration::from_millis(3));
        }
    }
}

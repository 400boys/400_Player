//! Headless player: plays a file through the whole engine (virtual audio device, no window) and
//! reports what a UI would see. Used for smoke tests and as a latency / sync benchmark.
//!
//! ```text
//! cargo run --release --example headless -- FILE [--seek T]... [--speed X] [--secs N] [--no-audio-device]
//! ```

use std::time::{Duration, Instant};

use media_player_400_engine::{EngineConfig, Event, PlayState, Player, SeekMode};

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: headless FILE [--seek T]... [--speed X] [--secs N]");
        std::process::exit(2);
    };
    let mut seeks = Vec::new();
    let mut speed = 1.0;
    let mut secs = 6.0;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--seek" => seeks.push(args.next().and_then(|v| v.parse::<f64>().ok()).expect("--seek T")),
            "--speed" => speed = args.next().and_then(|v| v.parse().ok()).expect("--speed X"),
            "--secs" => secs = args.next().and_then(|v| v.parse().ok()).expect("--secs N"),
            other => panic!("unknown argument {other}"),
        }
    }

    let cfg = EngineConfig { force_virtual_audio: false, ..Default::default() };
    let player = Player::new(cfg, None);
    player.set_speed(speed);
    let t0 = Instant::now();
    player.open(&path);

    let mut first_frame: Option<f64> = None;
    let mut frames = 0u64;
    let mut playing_since: Option<(Instant, f64)> = None;
    let mut last_report = Instant::now();
    let mut seek_iter = seeks.into_iter();
    let mut next_seek_at = 1.5;
    let mut pending_seek: Option<(Instant, f64)> = None;
    let mut max_abs_diff: f32 = 0.0;
    let mut diff_sum = 0.0f64;
    let mut diff_n = 0u64;

    loop {
        let now = t0.elapsed().as_secs_f64();
        for ev in player.poll_events() {
            match &ev {
                Event::Opened(info) => println!(
                    "[{now:7.3}] opened: {}v {}a {}s, duration {:?}, container {}",
                    info.video.len(),
                    info.audio.len(),
                    info.subtitles.len(),
                    info.duration,
                    info.container
                ),
                Event::State(s) => {
                    println!("[{now:7.3}] state: {s:?}");
                    if *s == PlayState::Playing && playing_since.is_none() {
                        playing_since = Some((Instant::now(), player.position()));
                    }
                }
                other => println!("[{now:7.3}] {other:?}"),
            }
        }
        if let Some(f) = player.poll_frame(0.008) {
            frames += 1;
            if first_frame.is_none() {
                first_frame = Some(now);
                println!("[{now:7.3}] FIRST FRAME pts={:.3} {}x{} {:?}", f.pts, f.width, f.height, f.layout);
            }
            if let Some((t, target)) = pending_seek.take() {
                println!("[{now:7.3}] seek to {target:.2}s → frame pts {:.3} after {:.0} ms", f.pts, t.elapsed().as_secs_f64() * 1000.0);
            }
        }
        let snap = player.snapshot();
        if snap.state == PlayState::Playing
            && let Some(d) = snap.stats.av_diff_ms
        {
            max_abs_diff = max_abs_diff.max(d.abs());
            diff_sum += f64::from(d);
            diff_n += 1;
        }
        if now >= next_seek_at
            && pending_seek.is_none()
            && let Some(t) = seek_iter.next()
        {
            println!("[{now:7.3}] → seek {t}");
            pending_seek = Some((Instant::now(), t));
            player.seek(t, SeekMode::Accurate);
            next_seek_at = now + 1.5;
        }
        if last_report.elapsed() > Duration::from_millis(1000) {
            last_report = Instant::now();
            println!(
                "[{now:7.3}] {:?} pos={:.3}s dec={} shown={} dropped={} queue={} underruns={} avdiff={:?}ms subs={}",
                snap.state,
                snap.position,
                snap.stats.decoded_frames,
                snap.stats.presented,
                snap.stats.dropped_late,
                snap.stats.frame_queue,
                snap.stats.audio_underruns,
                snap.stats.av_diff_ms,
                player.embedded_subtitles().lock().cues().len()
            );
        }
        if matches!(snap.state, PlayState::Ended | PlayState::Failed) || now > secs {
            break;
        }
        std::thread::sleep(Duration::from_millis(4));
    }

    let snap = player.snapshot();
    println!("---- summary ----");
    println!("first frame after : {:?} s", first_frame);
    println!("frames shown      : {frames}");
    println!("final state       : {:?} at {:.3}s / {:?}", snap.state, snap.position, snap.duration);
    if let Some((t, p0)) = playing_since {
        let wall = t.elapsed().as_secs_f64();
        let media = snap.position - p0;
        println!("clock vs wall     : media {media:.3}s in wall {wall:.3}s → ratio {:.4} (speed {speed})", media / wall.max(1e-9));
    }
    if diff_n > 0 {
        println!("A/V diff          : mean {:+.1} ms, max |{max_abs_diff:.1}| ms over {diff_n} samples", diff_sum / diff_n as f64);
    }
    println!("stats             : {:?}", snap.stats);
    let (ready, planned) = player.thumbnail_progress();
    println!("previews          : {ready}/{planned}");
}

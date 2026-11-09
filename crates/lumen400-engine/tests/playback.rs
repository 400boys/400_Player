//! End-to-end tests of the engine with real (generated) media and the virtual audio device.

mod common;

use std::time::{Duration, Instant};

use common::*;
use lumen400_engine::{Deinterlace, EngineConfig, Event, LangPrefs, PlayState, Player, SeekMode, StreamSelection};

macro_rules! need {
    ($e:expr) => {
        match $e {
            Some(p) => p,
            None => return,
        }
    };
}

fn path(p: &std::path::Path) -> &str {
    p.to_str().unwrap()
}

#[test]
fn plays_an_av_file_to_the_end() {
    let _g = serial();
    let f = need!(av_mp4());
    let mut r = Run::new();
    r.player.open(path(&f));
    assert!(r.wait_state(PlayState::Ended, 15.0), "never ended; states {:?}", r.states);
    r.pump();
    let wall = r.now();
    assert_eq!(&r.states[..4], &[PlayState::Opening, PlayState::Buffering, PlayState::Playing, PlayState::Ended], "{:?}", r.states);
    assert!((5.7..7.2).contains(&wall), "played in {wall:.2}s");
    assert!(r.frames.len() >= 146, "only {} of 150 frames shown", r.frames.len());
    assert!(r.frames.windows(2).all(|w| w[1].0 > w[0].0), "frame timestamps must increase");
    let snap = r.player.snapshot();
    assert_eq!(snap.state, PlayState::Ended);
    assert!((snap.position - 6.0).abs() < 0.2, "ended at {}", snap.position);
    assert_eq!(snap.stats.decode_errors, 0);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(r.events.iter().any(|e| matches!(e, Event::Opened(i) if i.has_video() && !i.audio.is_empty())));
    assert!(r.events.iter().any(|e| matches!(e, Event::Ended)));
}

#[test]
fn audio_and_video_stay_in_sync() {
    let _g = serial();
    let f = need!(av_mp4());
    let mut r = Run::new();
    r.player.open(path(&f));
    assert!(r.wait_state(PlayState::Playing, 5.0));
    r.run_for(0.5);
    let mut diffs = Vec::new();
    let end = Instant::now() + Duration::from_secs_f64(4.0);
    while Instant::now() < end {
        r.pump();
        if let Some(d) = r.player.snapshot().stats.av_diff_ms {
            diffs.push(d);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(diffs.len() > 300, "too few samples: {}", diffs.len());
    let max = diffs.iter().fold(0.0f32, |m, d| m.max(d.abs()));
    let mean = diffs.iter().map(|d| d.abs()).sum::<f32>() / diffs.len() as f32;
    assert!(max < 80.0, "max A/V offset {max} ms");
    assert!(mean < 30.0, "mean A/V offset {mean} ms");
    assert_eq!(r.player.snapshot().stats.audio_underruns, 0);
}

#[test]
fn accurate_seek_lands_on_the_requested_frame() {
    let _g = serial();
    let f = need!(av_mp4());
    let mut r = Run::new();
    r.player.open(path(&f));
    assert!(r.wait_state(PlayState::Playing, 5.0));
    r.run_for(0.3);
    let mark = r.frames.len();
    r.player.seek(3.52, SeekMode::Accurate); // between frames 88 (3.52) — exactly on a frame
    assert!(r.until(5.0, |r| r.frames[mark..].iter().any(|f| f.0 > 2.0)));
    let first = r.frames[mark..].iter().find(|f| f.0 > 2.0).unwrap().0;
    assert!((first - 3.52).abs() < 0.021, "landed on {first}");
    // And it keeps playing from there.
    r.run_for(0.6);
    assert!(r.player.position() > 3.8, "position {}", r.player.position());
    assert_eq!(r.player.state(), PlayState::Playing);
}

#[test]
fn accurate_seek_between_frames_picks_the_next_frame() {
    let _g = serial();
    let f = need!(av_mp4());
    let mut r = Run::new();
    r.player.open(path(&f));
    assert!(r.wait_state(PlayState::Playing, 5.0));
    let mark = r.frames.len();
    r.player.seek(4.51, SeekMode::Accurate);
    assert!(r.until(5.0, |r| r.frames[mark..].iter().any(|f| f.0 > 3.0)));
    let first = r.frames[mark..].iter().find(|f| f.0 > 3.0).unwrap().0;
    assert!((4.48..4.56).contains(&first), "landed on {first}");
}

#[test]
fn fast_seek_lands_on_a_keyframe_at_or_before_the_target() {
    let _g = serial();
    let f = need!(av_mp4());
    let mut r = Run::new();
    r.player.open(path(&f));
    assert!(r.wait_state(PlayState::Playing, 5.0));
    let mark = r.frames.len();
    r.player.seek(3.5, SeekMode::Fast);
    assert!(r.until(5.0, |r| r.frames[mark..].iter().any(|f| f.0 > 2.0)));
    let first = r.frames[mark..].iter().find(|f| f.0 > 2.0).unwrap().0;
    assert!((2.9..=3.51).contains(&first), "landed on {first}");
}

#[test]
fn pause_freezes_the_clock_and_resume_continues() {
    let _g = serial();
    let f = need!(av_mp4());
    let mut r = Run::new();
    r.player.open(path(&f));
    assert!(r.wait_state(PlayState::Playing, 5.0));
    r.run_for(1.0);
    r.player.pause();
    assert!(r.wait_state(PlayState::Paused, 2.0));
    r.run_for(0.2);
    let p0 = r.player.position();
    let shown = r.frames.len();
    r.run_for(1.0);
    assert!((r.player.position() - p0).abs() < 0.002, "moved while paused: {} → {}", p0, r.player.position());
    assert_eq!(r.frames.len(), shown, "frames were presented while paused");
    r.player.play();
    assert!(r.wait_state(PlayState::Playing, 2.0));
    r.run_for(1.0);
    let p1 = r.player.position();
    assert!(p1 - p0 > 0.85 && p1 - p0 < 1.2, "advanced {} s in 1 s", p1 - p0);
}

#[test]
fn speed_changes_scale_the_clock() {
    let _g = serial();
    let f = need!(av_mp4());
    for speed in [0.5, 1.5, 2.0] {
        let mut r = Run::new();
        r.player.set_speed(speed);
        r.player.open(path(&f));
        assert!(r.wait_state(PlayState::Playing, 5.0));
        r.run_for(0.4);
        let (p0, t0) = (r.player.position(), Instant::now());
        r.run_for(2.0);
        let ratio = (r.player.position() - p0) / t0.elapsed().as_secs_f64();
        assert!((ratio - speed).abs() < 0.06 * speed + 0.03, "speed {speed}: clock ran at {ratio}");
    }
}

#[test]
fn speed_can_change_during_playback() {
    let _g = serial();
    let f = need!(av_mp4());
    let mut r = Run::new();
    r.player.open(path(&f));
    assert!(r.wait_state(PlayState::Playing, 5.0));
    r.run_for(0.5);
    r.player.set_speed(2.0);
    r.run_for(0.8); // the stretcher needs a moment to flush the read-ahead
    let (p0, t0) = (r.player.position(), Instant::now());
    r.run_for(1.5);
    let ratio = (r.player.position() - p0) / t0.elapsed().as_secs_f64();
    assert!((ratio - 2.0).abs() < 0.15, "ratio {ratio}");
}

#[test]
fn video_only_files_use_the_system_clock() {
    let _g = serial();
    let f = need!(video_only_mkv());
    let mut r = Run::new();
    r.player.open(path(&f));
    assert!(r.wait_state(PlayState::Ended, 10.0), "{:?}", r.states);
    assert!((3.8..5.0).contains(&r.now()), "took {}", r.now());
    assert!(r.frames.len() >= 117, "{} of 120 frames", r.frames.len());
}

#[test]
fn audio_only_files_play_and_end() {
    let _g = serial();
    let f = need!(tone_flac());
    let mut r = Run::new();
    r.player.open(path(&f));
    assert!(r.wait_state(PlayState::Ended, 10.0), "{:?}", r.states);
    assert!((3.8..5.0).contains(&r.now()), "took {}", r.now());
    assert!(r.frames.is_empty());
    let snap = r.player.snapshot();
    assert!(snap.has_audio && !snap.has_video);
    assert!((snap.position - 4.0).abs() < 0.1);
}

#[test]
fn spectrum_reacts_to_the_audio() {
    let _g = serial();
    let f = need!(tone_flac());
    let mut r = Run::new();
    r.player.set_spectrum_enabled(true);
    r.player.open(path(&f));
    assert!(r.wait_state(PlayState::Playing, 5.0));
    let mut best = 0.0f32;
    let end = Instant::now() + Duration::from_secs_f64(1.5);
    while Instant::now() < end {
        r.pump();
        if let Some(s) = r.player.spectrum(0.016) {
            best = best.max(s.bands.iter().copied().fold(0.0, f32::max));
            assert_eq!(s.bands.len(), s.peaks.len());
            assert!(s.bands.iter().all(|b| (0.0..=1.0).contains(b)));
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    assert!(best > 0.3, "no band ever lit up: {best}");
}

#[test]
fn a_missing_file_fails_cleanly_and_the_player_recovers() {
    let _g = serial();
    let mut r = Run::new();
    r.player.open("/definitely/not/here.mkv");
    assert!(r.wait_state(PlayState::Failed, 5.0), "{:?}", r.states);
    assert!(r.errors.iter().any(|e| e.contains("here.mkv")), "{:?}", r.errors);
    // The same player still works afterwards.
    let f = need!(tone_flac());
    r.player.open(path(&f));
    assert!(r.wait_state(PlayState::Playing, 5.0));
}

#[test]
fn garbage_files_are_rejected() {
    let _g = serial();
    let mut r = Run::new();
    r.player.open(path(&garbage()));
    assert!(r.wait_state(PlayState::Failed, 10.0), "{:?}", r.states);
    assert!(!r.errors.is_empty());
}

#[test]
fn stop_returns_to_idle() {
    let _g = serial();
    let f = need!(av_mp4());
    let mut r = Run::new();
    r.player.open(path(&f));
    assert!(r.wait_state(PlayState::Playing, 5.0));
    r.player.stop();
    assert!(r.wait_state(PlayState::Idle, 3.0));
    assert!(r.player.snapshot().duration.is_none());
}

#[test]
fn opening_another_file_while_playing_switches_cleanly() {
    let _g = serial();
    let a = need!(av_mp4());
    let b = need!(tone_flac());
    let mut r = Run::new();
    r.player.open(path(&a));
    assert!(r.wait_state(PlayState::Playing, 5.0));
    r.run_for(0.4);
    r.player.open(path(&b));
    assert!(r.until(5.0, |r| r.events.iter().rev().find_map(|e| if let Event::Opened(i) = e { Some(i.video.is_empty()) } else { None }) == Some(true)));
    assert!(r.wait_state(PlayState::Ended, 8.0), "{:?}", r.states);
}

#[test]
fn dropping_the_player_while_playing_does_not_hang() {
    let _g = serial();
    let f = need!(av_mp4());
    let t = Instant::now();
    {
        let mut r = Run::new();
        r.player.open(path(&f));
        assert!(r.wait_state(PlayState::Playing, 5.0));
    }
    assert!(t.elapsed() < Duration::from_secs(4), "shutdown took {:?}", t.elapsed());
}

#[test]
fn frame_stepping_forward_and_backward() {
    let _g = serial();
    let f = need!(av_mp4());
    let mut r = Run::new();
    r.player.open_with(path(&f), Some(2.0), true, StreamSelection::default());
    assert!(r.wait_state(PlayState::Paused, 5.0), "{:?}", r.states);
    assert!(r.until(2.0, |r| !r.frames.is_empty()));
    let p0 = r.frames.last().unwrap().0;
    assert!((p0 - 2.0).abs() < 0.021, "first paused frame {p0}");
    for k in 1..=3 {
        let n = r.frames.len();
        r.player.step_frame(true);
        assert!(r.until(2.0, |r| r.frames.len() > n), "step {k} produced no frame");
        let p = r.frames.last().unwrap().0;
        assert!((p - (p0 + 0.04 * f64::from(k))).abs() < 0.021, "step {k}: {p}");
    }
    let n = r.frames.len();
    r.player.step_frame(false);
    assert!(r.until(3.0, |r| r.frames.len() > n), "backward step produced no frame");
    let p = r.frames.last().unwrap().0;
    assert!((p - (p0 + 0.08)).abs() < 0.021, "backward step: {p}");
    assert_eq!(r.player.state(), PlayState::Paused);
}

#[test]
fn resume_position_and_paused_start() {
    let _g = serial();
    let f = need!(av_mp4());
    let mut r = Run::new();
    r.player.open_with(path(&f), Some(4.0), false, StreamSelection::default());
    assert!(r.wait_state(PlayState::Playing, 5.0));
    assert!(r.until(2.0, |r| !r.frames.is_empty()));
    assert!((r.frames[0].0 - 4.0).abs() < 0.021, "{}", r.frames[0].0);
    assert!(r.wait_state(PlayState::Ended, 6.0));
    assert!(r.now() < 4.5, "should only play the last 2 s: {}", r.now());
}

#[test]
fn replay_after_the_end() {
    let _g = serial();
    let f = need!(video_only_mkv());
    let mut r = Run::new();
    r.player.open_with(path(&f), Some(3.2), false, StreamSelection::default());
    assert!(r.wait_state(PlayState::Ended, 6.0));
    r.player.play();
    assert!(r.wait_state(PlayState::Playing, 3.0), "{:?}", r.states);
    r.run_for(0.5);
    assert!(r.player.position() < 1.0);
}

#[test]
fn embedded_subtitles_are_decoded_ahead_of_playback() {
    let _g = serial();
    let f = need!(subs_mkv());
    let mut r = Run::new();
    // Nothing selects the track automatically without language preferences…
    r.player.open(path(&f));
    assert!(r.until(5.0, |r| r.events.iter().any(|e| matches!(e, Event::Opened(_)))));
    let info = r.events.iter().find_map(|e| if let Event::Opened(i) = e { Some(i.clone()) } else { None }).unwrap();
    assert_eq!(info.subtitles.len(), 1, "{info:?}");
    assert_eq!(info.selected_subtitle, None);
    assert_eq!(info.subtitles[0].language.as_deref(), Some("fre"));
    // …the user picks it explicitly.
    r.player.open_with(path(&f), None, false, StreamSelection { subtitle: Some(Some(2)), ..Default::default() });
    assert!(r.until(6.0, |r| r.player.embedded_subtitles().lock().cues().len() >= 2), "cues: {}", r.player.embedded_subtitles().lock().cues().len());
    let track = r.player.embedded_subtitles();
    let track = track.lock();
    let at1 = track.active_at(1.0);
    assert_eq!(at1.len(), 1, "cue at 1.0 s");
    assert!(at1[0].plain_text().contains("Bonjour Lumen400"), "{:?}", at1[0]);
    assert!(track.active_at(1.8).is_empty());
    assert_eq!(track.active_at(2.5).len(), 1);
}

#[test]
fn subtitles_are_picked_from_language_preferences() {
    let _g = serial();
    let f = need!(subs_mkv());
    let cfg = EngineConfig { force_virtual_audio: true, lang_prefs: LangPrefs::from_settings("en", "fr, en", true), ..Default::default() };
    let mut r = Run::with(cfg);
    r.player.open(path(&f));
    assert!(r.until(5.0, |r| r.events.iter().any(|e| matches!(e, Event::Opened(_)))));
    let info = r.events.iter().find_map(|e| if let Event::Opened(i) = e { Some(i.clone()) } else { None }).unwrap();
    assert_eq!(info.selected_subtitle, Some(2));
    assert!(r.until(6.0, |r| !r.player.embedded_subtitles().lock().cues().is_empty()));
}

#[test]
fn seek_storm_does_not_deadlock_or_corrupt_state() {
    let _g = serial();
    let f = need!(av_mp4());
    let mut r = Run::new();
    r.player.open(path(&f));
    assert!(r.wait_state(PlayState::Playing, 5.0));
    let mut x = 12345u32;
    let mut rnd = move || {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        x
    };
    for _ in 0..80 {
        let pos = f64::from(rnd() % 5800) / 1000.0;
        let mode = if rnd() % 2 == 0 { SeekMode::Fast } else { SeekMode::Accurate };
        r.player.seek(pos, mode);
        r.run_for(f64::from(rnd() % 40) / 1000.0);
    }
    // After the storm a final seek must still settle and play normally.
    r.player.seek(1.0, SeekMode::Accurate);
    assert!(r.until(5.0, |r| r.player.state() == PlayState::Playing && (r.player.position() - 1.0).abs() < 0.3), "{:?} {}", r.player.state(), r.player.position());
    let n = r.frames.len();
    r.run_for(1.0);
    assert!(r.frames.len() > n + 15, "playback stalled after the storm");
    assert_eq!(r.player.snapshot().stats.decode_errors, 0);
}

#[test]
fn relative_seeks_accumulate() {
    let _g = serial();
    let f = need!(av_mp4());
    let mut r = Run::new();
    r.player.open_with(path(&f), None, true, StreamSelection::default());
    assert!(r.wait_state(PlayState::Paused, 5.0));
    r.player.seek_relative(2.0);
    r.player.seek_relative(1.0);
    assert!(r.until(3.0, |r| r.player.state() == PlayState::Paused && (r.player.position() - 3.0).abs() < 0.05), "pos {}", r.player.position());
    r.player.seek_relative(-10.0);
    assert!(r.until(3.0, |r| r.player.position() < 0.05 && r.player.state() == PlayState::Paused));
}

#[test]
fn audio_controls_are_safe_at_any_time() {
    let _g = serial();
    let f = need!(av_mp4());
    let mut r = Run::new();
    r.player.set_volume(0.0);
    r.player.open(path(&f));
    r.player.set_muted(true);
    assert!(r.wait_state(PlayState::Playing, 5.0));
    r.player.set_volume(2.0);
    r.player.set_muted(false);
    r.player.set_audio_delay(0.25);
    r.run_for(0.5);
    let snap = r.player.snapshot();
    assert!((snap.volume - 2.0).abs() < 1e-6 && !snap.muted);
    r.player.set_audio_delay(0.0);
}

#[test]
fn asking_for_hardware_decoding_never_breaks_playback() {
    // On a machine without a usable decoder device (CI, containers) the request must fall back to
    // software silently; on one with a device the pictures must come out identical in size and count.
    let _g = serial();
    let f = need!(av_mp4());
    let mut r = Run::with(EngineConfig { force_virtual_audio: true, hw_decode: true, ..Default::default() });
    r.player.open(path(&f));
    assert!(r.wait_state(PlayState::Ended, 15.0), "never ended; states {:?}", r.states);
    assert!(r.frames.len() >= 146, "only {} of 150 frames shown", r.frames.len());
    let snap = r.player.snapshot();
    assert_eq!(snap.stats.decode_errors, 0);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(snap.video_size, Some((320, 180)));
    eprintln!("hardware decoder in use: {:?}", snap.stats.hw_decoder);
}

#[test]
fn engine_config_default_is_sane() {
    let c = EngineConfig::default();
    assert!(!c.hw_decode);
    assert!(c.frame_queue_bytes >= 64 << 20);
    let _ = Player::new(c, None);
}

// -------------------------------------------------------------------------------------------------
// Seek-bar thumbnails
// -------------------------------------------------------------------------------------------------

fn dominant(img: &lumen400_engine::RgbaImage) -> &'static str {
    let [r, g, b] = img.average_color();
    match (r > 180, g > 180, b > 180, r < 70, g < 70, b < 70) {
        (true, _, _, _, true, true) => "red",
        (_, true, _, true, _, true) => "green",
        (_, _, true, true, true, _) => "blue",
        _ => "other",
    }
}

fn check_blocks(file: &std::path::Path) {
    let mut r = Run::new();
    r.player.open(path(file));
    assert!(r.until(30.0, |r| r.player.thumbnail_progress() == (12, 12)), "previews never completed: {:?}", r.player.thumbnail_progress());
    for (t, want) in [(0.3, "red"), (2.0, "red"), (4.7, "green"), (6.0, "green"), (9.0, "blue"), (11.8, "blue")] {
        let th = r.player.thumbnail(t).unwrap_or_else(|| panic!("no preview near {t}"));
        assert_eq!(dominant(&th.image), want, "preview for {t}s is {:?}", th.image.average_color());
        assert!((th.time - t).abs() <= 0.6, "the preview for {t}s stands for {}s", th.time);
        assert!(th.image.width <= 256 && th.image.height <= 256);
        assert_eq!((th.image.width, th.image.height), (256, 144));
        assert_eq!(th.image.data.len(), 256 * 144 * 4);
    }
    // Different slots have different ids; asking twice gives the same one.
    let a = r.player.thumbnail(1.0).unwrap().id;
    let b = r.player.thumbnail(9.0).unwrap().id;
    assert_ne!(a, b);
    assert_eq!(a, r.player.thumbnail(1.0).unwrap().id);
}

#[test]
fn thumbnails_show_the_right_moment() {
    let _g = serial();
    check_blocks(&need!(colour_blocks("mkv")));
}

#[test]
fn thumbnails_respect_the_container_start_offset() {
    let _g = serial();
    // MPEG-TS timestamps start at ~1.4 s; the previews must still match the media time.
    check_blocks(&need!(colour_blocks("ts")));
}

#[test]
fn thumbnails_follow_the_media_and_can_be_disabled() {
    let _g = serial();
    let blocks = need!(colour_blocks("mkv"));
    let tone = need!(tone_flac());
    let av = need!(av_mp4());
    let mut r = Run::new();
    r.player.open(path(&blocks));
    assert!(r.until(30.0, |r| r.player.thumbnail_progress().0 >= 1));
    // An audio file has no pictures, and the previous ones are gone.
    r.player.open(path(&tone));
    assert!(r.until(10.0, |r| r.player.state().is_active()));
    r.run_for(0.6);
    assert_eq!(r.player.thumbnail_progress(), (0, 0));
    assert!(r.player.thumbnail(1.0).is_none());
    // Another video: its own pictures (the test pattern is not a plain colour).
    r.player.open(path(&av));
    assert!(r.until(30.0, |r| r.player.thumbnail_progress() == (12, 12)));
    assert_eq!(dominant(&r.player.thumbnail(3.0).unwrap().image), "other");
    // Switching tracks restarts the session but keeps the pictures.
    let before = r.player.thumbnail(3.0).unwrap().id;
    r.player.select_tracks(StreamSelection { audio: Some(None), ..Default::default() });
    r.run_for(0.8);
    assert_eq!(r.player.thumbnail_progress(), (12, 12));
    assert_eq!(r.player.thumbnail(3.0).unwrap().id, before);

    // Disabled in the configuration: nothing is generated.
    let mut off = Run::with(EngineConfig { force_virtual_audio: true, generate_thumbnails: false, ..Default::default() });
    off.player.open(path(&blocks));
    assert!(off.until(10.0, |r| r.player.state() == PlayState::Playing));
    off.run_for(1.0);
    assert_eq!(off.player.thumbnail_progress(), (0, 0));
}

#[test]
fn previews_do_not_disturb_playback() {
    let _g = serial();
    let f = need!(colour_blocks("mkv"));
    let mut r = Run::new();
    r.player.open(path(&f));
    assert!(r.wait_state(PlayState::Ended, 20.0), "states {:?}", r.states);
    assert!(r.frames.len() >= 290, "only {} of 300 frames shown while previews were generated", r.frames.len());
    assert_eq!(r.player.snapshot().stats.decode_errors, 0);
}

// --- deinterlacing ---------------------------------------------------------------------------------

/// What a run presented: `(pts, combing, still flagged interlaced)` for every picture, and whether
/// the statistics ever named a deinterlacer.
struct Shown {
    pictures: Vec<(f64, f64, bool)>,
    filter: Option<&'static str>,
    ended: bool,
}

/// Plays `file` under `mode` for at most `secs` (less when it ends sooner) and measures what is shown.
fn play_measuring(file: &std::path::Path, mode: Deinterlace, secs: f64) -> Shown {
    let mut r = Run::new();
    r.player.set_deinterlace(mode);
    r.player.open(path(file));
    let mut shown = Shown { pictures: Vec::new(), filter: None, ended: false };
    let end = Instant::now() + Duration::from_secs_f64(secs);
    while Instant::now() < end {
        r.pump();
        if let Some(f) = r.last_frame.take() {
            shown.pictures.push((f.pts, combing(&f), f.interlaced));
        }
        shown.filter = shown.filter.or(r.player.snapshot().stats.deinterlacer);
        if r.player.state() == PlayState::Ended {
            shown.ended = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(3));
    }
    shown
}

fn share_clean(pictures: &[(f64, f64, bool)]) -> f64 {
    pictures.iter().filter(|p| p.1 < 3.0).count() as f64 / pictures.len().max(1) as f64
}

#[test]
fn interlaced_pictures_are_rebuilt_whole_unless_the_player_is_told_not_to() {
    let _g = serial();
    let f = need!(interlaced_mpg());

    let off = play_measuring(&f, Deinterlace::Off, 1.5);
    assert!(off.pictures.len() > 20, "only {} pictures", off.pictures.len());
    assert!(off.pictures.iter().all(|p| p.2), "the stream flags its pictures as interlaced");
    assert!(share_clean(&off.pictures) < 0.15, "left alone, the pictures stay combed: {:.0} % clean", share_clean(&off.pictures) * 100.0);
    assert_eq!(off.filter, None);

    let auto = play_measuring(&f, Deinterlace::Auto, 5.0);
    assert!(auto.ended, "the clip never ended");
    assert!(auto.filter.is_some(), "interlaced pictures were seen but no deinterlacer was reported");
    let (last, rest) = auto.pictures.split_last().expect("pictures");
    assert!(share_clean(rest) > 0.92, "only {:.0} % of the pictures are clean", share_clean(rest) * 100.0);
    assert!(rest.iter().all(|p| !p.2), "rebuilt pictures no longer say they are interlaced");
    assert!(auto.pictures.len() >= 70, "only {} of 75 pictures shown", auto.pictures.len());
    assert!(auto.pictures.windows(2).all(|w| w[1].0 > w[0].0), "timestamps must keep increasing");
    assert!(last.0 > 2.9, "the last picture, held back by the filter, was lost: it ends at {:.2} s", last.0);
    assert!(auto.pictures[0].0 < 0.1, "starts at the start: {}", auto.pictures[0].0);

    let on = play_measuring(&f, Deinterlace::On, 1.5);
    assert!(on.filter.is_some());
    assert!(share_clean(&on.pictures[..on.pictures.len() - 1]) > 0.92);
}

#[test]
fn progressive_video_never_gets_a_deinterlacer_unless_forced() {
    let _g = serial();
    let f = need!(av_mp4());
    let auto = play_measuring(&f, Deinterlace::Auto, 1.5);
    assert!(auto.pictures.len() > 20);
    assert_eq!(auto.filter, None, "progressive video must not pay for deinterlacing");
    let forced = play_measuring(&f, Deinterlace::On, 2.0);
    assert!(forced.filter.is_some(), "forced on, every picture goes through the filter");
    assert!(forced.pictures.len() > 25, "{} pictures", forced.pictures.len());
    assert!(forced.pictures.windows(2).all(|w| w[1].0 > w[0].0));
}

#[test]
fn seeking_inside_interlaced_video_shows_clean_pictures_from_the_target() {
    let _g = serial();
    let f = need!(interlaced_mpg());
    let mut r = Run::new();
    r.player.open(path(&f));
    assert!(r.wait_state(PlayState::Playing, 5.0));
    r.run_for(0.4);
    r.last_frame = None;
    let mark = r.frames.len();
    r.player.seek(1.52, SeekMode::Accurate);
    let mut after = Vec::new();
    assert!(
        r.until(5.0, |r| {
            if let Some(frame) = r.last_frame.take()
                && frame.pts > 1.0
            {
                after.push((frame.pts, combing(&frame)));
            }
            after.len() >= 6
        }),
        "nothing shown after the seek"
    );
    assert!((after[0].0 - 1.52).abs() < 0.021, "landed on {}", after[0].0);
    assert!(after.iter().all(|p| p.1 < 3.0), "combed pictures after a seek: {after:?}");
    assert!(r.frames[mark..].iter().filter(|f| f.0 > 1.0).all(|f| f.0 >= 1.5), "a picture from before the target was shown");
}

/// Pumps the player for up to `secs` and notes `(pts, combing)` of every picture shown.
fn collect_combing(r: &mut Run, pics: &mut Vec<(f64, f64)>, secs: f64) {
    let end = Instant::now() + Duration::from_secs_f64(secs);
    while Instant::now() < end && r.player.state() != PlayState::Ended {
        r.pump();
        if let Some(frame) = r.last_frame.take() {
            pics.push((frame.pts, combing(&frame)));
        }
        std::thread::sleep(Duration::from_millis(3));
    }
}

#[test]
fn the_policy_can_change_while_playing() {
    let _g = serial();
    let f = need!(interlaced_mpg());
    let mut r = Run::new();
    r.player.set_deinterlace(Deinterlace::Off);
    r.player.open(path(&f));
    let mut pics: Vec<(f64, f64)> = Vec::new();
    collect_combing(&mut r, &mut pics, 0.6);
    assert!(pics.len() >= 8);
    r.player.set_deinterlace(Deinterlace::Auto);
    collect_combing(&mut r, &mut pics, 4.0);
    assert!(pics.windows(2).all(|w| w[1].0 > w[0].0), "pictures lost or repeated across the change: {:?}", pics.iter().map(|p| p.0).collect::<Vec<_>>());
    assert!(pics[..5].iter().all(|p| p.1 > 3.0), "before the change the pictures are combed");
    // The decoder is a little ahead of the screen: the change shows after the pictures already queued.
    let tail = &pics[pics.len() - 20..pics.len() - 1];
    assert!(tail.iter().all(|p| p.1 < 3.0), "after the change they are clean: {tail:?}");
}

#[test]
fn a_paused_picture_is_redone_by_seeking_to_where_it_is() {
    let _g = serial();
    let f = need!(interlaced_mpg());
    let mut r = Run::new();
    r.player.set_deinterlace(Deinterlace::Off);
    r.player.open_with(path(&f), Some(1.0), true, StreamSelection::default());
    assert!(r.wait_state(PlayState::Paused, 5.0), "{:?}", r.states);
    assert!(r.until(3.0, |r| r.last_frame.is_some()));
    assert!(combing(r.last_frame.as_ref().unwrap()) > 3.0, "the picture on screen is combed");

    r.player.set_deinterlace(Deinterlace::Auto);
    r.last_frame = None;
    r.player.seek(1.0, SeekMode::Accurate);
    assert!(r.until(3.0, |r| r.last_frame.is_some()), "no picture after the seek");
    let shown = r.last_frame.take().unwrap();
    assert!((shown.pts - 1.0).abs() < 0.021, "{}", shown.pts);
    assert!(combing(&shown) < 3.0, "the same moment, rebuilt: {}", combing(&shown));
    assert_eq!(r.player.state(), PlayState::Paused);
}

// --- seeking in containers that cannot tell where their keyframes are ------------------------------------

#[test]
fn seeks_in_a_program_stream_land_on_the_target_not_after_it() {
    let _g = serial();
    // MPEG program streams (DVDs, TV captures) let the container put a seek on any picture: the
    // decoder then waits for the next keyframe, and the first picture shown comes late.
    let f = need!(interlaced_mpg());
    let mut r = Run::new();
    r.player.open(path(&f));
    assert!(r.wait_state(PlayState::Playing, 5.0));
    for target in [1.0, 1.52, 2.2, 0.7, 2.6] {
        r.last_frame = None;
        r.player.seek(target, SeekMode::Accurate);
        let mut first = None;
        assert!(
            r.until(5.0, |r| {
                // Pictures of the old position may still be on their way.
                if let Some(frame) = r.last_frame.take()
                    && (frame.pts - target).abs() < 0.6
                {
                    first = Some(frame.pts);
                }
                first.is_some()
            }),
            "nothing shown after a seek to {target}"
        );
        let first = first.unwrap();
        assert!((first - target).abs() < 0.021, "seek to {target}: first picture at {first}");
    }
    // A fast seek may land earlier, never later.
    r.last_frame = None;
    r.player.seek(2.2, SeekMode::Fast);
    let mut first = None;
    assert!(r.until(5.0, |r| {
        if let Some(frame) = r.last_frame.take()
            && (frame.pts - 2.2).abs() < 1.0
        {
            first = Some(frame.pts);
        }
        first.is_some()
    }));
    assert!(first.unwrap() <= 2.2 + 0.001 && first.unwrap() > 1.5, "fast seek landed at {first:?}");
}

// --- multi-format & 64-bit streaming container coverage --------------------------------------------

#[test]
fn probes_and_plays_wav_rf64_aiff_y4m_and_ivf_containers() {
    let _g = serial();
    let dir = media_dir().join(format!("lumen400-fmt-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);

    // 1. 64-bit RF64 WAV master (`ds64` chunk)
    let rf64_path = dir.join("test.rf64");
    let pcm = vec![0u8; 48_000 * 4]; // 1.0 s stereo 16-bit
    let mut rf64 = Vec::new();
    rf64.extend_from_slice(b"RF64\xFF\xFF\xFF\xFFWAVEds64");
    rf64.extend_from_slice(&28u32.to_le_bytes());
    rf64.extend_from_slice(&1000u64.to_le_bytes());
    rf64.extend_from_slice(&(pcm.len() as u64).to_le_bytes());
    rf64.extend_from_slice(&48_000u64.to_le_bytes());
    rf64.extend_from_slice(&0u32.to_le_bytes());
    rf64.extend_from_slice(b"fmt ");
    rf64.extend_from_slice(&16u32.to_le_bytes());
    rf64.extend_from_slice(&1u16.to_le_bytes());
    rf64.extend_from_slice(&2u16.to_le_bytes());
    rf64.extend_from_slice(&48_000u32.to_le_bytes());
    rf64.extend_from_slice(&192_000u32.to_le_bytes());
    rf64.extend_from_slice(&4u16.to_le_bytes());
    rf64.extend_from_slice(&16u16.to_le_bytes());
    rf64.extend_from_slice(b"data\xFF\xFF\xFF\xFF");
    rf64.extend_from_slice(&pcm);
    std::fs::write(&rf64_path, &rf64).unwrap();
    let info = lumen400_engine::probe(path(&rf64_path), &LangPrefs::default()).expect("probe rf64");
    assert_eq!(info.container, "rf64");
    assert_eq!(info.audio.len(), 1);
    assert!((info.duration.unwrap() - 1.0).abs() < 1e-4);

    // 2. Y4M 64-bit streaming video
    let y4m_path = dir.join("test.y4m");
    let mut y4m = b"YUV4MPEG2 W16 H16 F25:1 Ip A1:1 C420\n".to_vec();
    for _ in 0..10 {
        y4m.extend_from_slice(b"FRAME\n");
        y4m.extend_from_slice(&[128u8; 256 + 64 + 64]);
    }
    std::fs::write(&y4m_path, &y4m).unwrap();
    let info_y4m = lumen400_engine::probe(path(&y4m_path), &LangPrefs::default()).expect("probe y4m");
    assert_eq!(info_y4m.container, "yuv4mpegpipe");
    assert_eq!((info_y4m.video[0].width, info_y4m.video[0].height), (16, 16));
    assert!((info_y4m.duration.unwrap() - 0.4).abs() < 1e-4);

    let _ = std::fs::remove_dir_all(&dir);
}

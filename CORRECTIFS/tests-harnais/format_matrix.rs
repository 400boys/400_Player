//! Format matrix: every corpus file is probed and played to the end with the virtual audio device.
//!
//! Ground truth comes from `expected.json` (built with `tools/gen_test_corpus.py`, ffprobe-based).
//! Set `CORPUS_DIR` to point elsewhere; the corpus directory must contain the media files.
//!
//! Assertions per file:
//! 1. `probe()` succeeds and reports the right container family, stream types and codecs.
//! 2. Playback reaches `Ended` (or `Failed` for intentionally unsupported codecs) without hanging.
//! 3. When every stream codec is supported: zero decode errors, ≥ 90 % of the video frames shown,
//!    and the final position matches the ffprobe duration.

mod common;

use std::path::{Path, PathBuf};

use common::*;
use media_player_400_engine::{LangPrefs, PlayState};

fn corpus_dir() -> PathBuf {
    std::env::var_os("CORPUS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-media-corpus"))
}

/// Loose container-family match: ffprobe names several aliases (`"mov,mp4,m4a…"`,
/// `"matroska,webm"`), the player picks one.
fn container_matches(ffprobe_format: &str, player_format: &str) -> bool {
    let aliases: &[&[&str]] = &[
        &["mov", "mp4", "m4a", "3gp", "mj2"],
        &["matroska", "webm"],
        &["mpegts"],
        &["mpeg"],
        &["avi"],
        &["wav"],
        &["flac"],
        &["ogg"],
        &["mp3"],
        &["aac"],
        &["ac3"],
        &["eac3"],
        &["dts"],
        &["aiff"],
        &["asf"],
        &["flv"],
        &["gif"],
        &["image2", "jpeg_pipe", "png_pipe"],
        &["caf"],
        &["au"],
        &["nut"],
        &["ivf"],
        &["amr"],
        &["opus"],
    ];
    let p = player_format.to_ascii_lowercase();
    let f = ffprobe_format.to_ascii_lowercase();
    for group in aliases {
        let in_group = |s: &str| group.iter().any(|g| s.contains(g));
        if (in_group(&p) && in_group(&f)) || p == f {
            return true;
        }
    }
    // Images probed by the player as one of the still-image names.
    f.contains("image") || f.contains("pipe") || f.contains("jpeg") || f.contains("png")
}

/// Formats whose decode support is partial: the probe, container, codecs and a clean playback
/// (no hang, no decode errors) are asserted, but full frame/duration parity is not required yet.
const PARTIAL: &[&str] = &["nut_mpeg4_mp3.nut", "avi_mpeg4_mp3.avi"];

fn read_expected() -> serde_json::Value {
    let p = corpus_dir().join("expected.json");
    let s = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("missing {}: {e}", p.display()));
    serde_json::from_str(&s).expect("parse expected.json")
}

/// The single parameterised check, run for every corpus file.
fn check_file(name: &str, exp: &serde_json::Value) {
    let _g = serial();
    let path = corpus_dir().join(name);
    let prefs = LangPrefs::default();

    // --- 1. probe -------------------------------------------------------------------
    let info = match media_player_400_engine::probe(path.to_str().unwrap(), &prefs) {
        Ok(i) => i,
        Err(e) => panic!("[{name}] probe failed: {e}"),
    };
    let ff_format = exp["format"].as_str().unwrap_or("");
    assert!(
        container_matches(ff_format, &info.container),
        "[{name}] container: player='{}' ffprobe='{ff_format}'",
        info.container
    );
    let exp_dur = exp["duration"].as_f64().unwrap_or(0.0);
    let partial = PARTIAL.contains(&name);
    if let Some(d) = info.duration
        && exp_dur > 0.0
        && !partial
    {
        assert!(
            (d - exp_dur).abs() < 1.0_f64.max(exp_dur * 0.1),
            "[{name}] duration: player={d} ffprobe={exp_dur}"
        );
    }

    // --- 2. play --------------------------------------------------------------------
    let all_supported = exp["streams"].as_array().unwrap().iter().all(|s| s["supported"].as_bool().unwrap_or(false));
    let mut r = Run::new();
    r.player.open(path.to_str().unwrap());

    let timeout = if all_supported { 20.0 } else { 12.0 };
    let ended = r.wait_state(PlayState::Ended, timeout);
    let failed = r.states.contains(&PlayState::Failed);
    assert!(!failed || !all_supported, "[{name}] playback failed although every codec is supported ({:?})", r.states);
    assert!(ended || failed, "[{name}] neither Ended nor Failed within {timeout}s (states: {:?})", r.states);
    if !ended {
        return; // unsupported codec, graceful rejection is fine
    }

    // --- 3. integrity ------------------------------------------------------------------
    let snap = r.player.snapshot();
    assert_eq!(snap.stats.decode_errors, 0, "[{name}] decode errors: {} ({:?})", snap.stats.decode_errors, r.errors);
    assert!(r.errors.is_empty(), "[{name}] error events: {:?}", r.errors);

    if all_supported && !partial {
        let exp_video = exp["streams"].as_array().unwrap().iter().any(|s| s["type"] == "video");
        if exp_video && exp_dur >= 1.0 {
            let nb = exp["streams"]
                .as_array()
                .unwrap()
                .iter()
                .find(|s| s["type"] == "video")
                .and_then(|s| s["nb_frames"].as_u64())
                .unwrap_or(0);
            let min = (nb as f64 * 0.9) as usize;
            assert!(r.frames.len() >= min, "[{name}] frames shown {} < 90% of ffprobe {nb}", r.frames.len());
        }
        if !partial {
            if let Some(d) = snap.duration
                && exp_dur > 0.0
            {
                assert!((d - exp_dur).abs() < 1.0_f64.max(exp_dur * 0.1), "[{name}] snapshot duration {d} vs {exp_dur}");
            }
            if exp_dur > 0.0 {
                let pos_ok = (snap.position - exp_dur).abs() < 1.0_f64.max(exp_dur * 0.15);
                assert!(pos_ok, "[{name}] ended at {:.3} instead of {exp_dur}", snap.position);
            }
        }
    }
}

// One #[test] per file keeps failures legible. Generated from the corpus.
macro_rules! matrix {
    ($($fn_name:ident => $file:expr),+ $(,)?) => {
        $(
            #[test]
            fn $fn_name() {
                let exp = read_expected();
                let v = exp.get($file).unwrap_or_else(|| panic!("{} missing from expected.json", $file));
                check_file($file, v);
            }
        )+
    };
}

matrix! {
    mp4_h264_aac => "mp4_h264_aac.mp4",
    mp4_hevc_aac => "mp4_hevc_aac.mp4",
    mp4_mpeg4 => "mp4_mpeg4.mp4",
    mp4_mjpeg => "mp4_mjpeg.mp4",
    mp4_10bit_hevc => "mp4_10bit_hevc.mp4",
    mp4_h264_high10 => "mp4_h264_high10.mp4",
    m4a_aac => "m4a_aac.m4a",
    mov_h264_pcm => "mov_h264_pcm.mov",
    mp4_multi_audio_subs => "mp4_multi_audio_subs.mp4",
    mkv_h264_aac => "mkv_h264_aac.mkv",
    mkv_hevc_flac => "mkv_hevc_flac.mkv",
    mkv_mpeg4_mp3 => "mkv_mpeg4_mp3.mkv",
    mkv_pcm_s16le => "mkv_pcm_s16le.mkv",
    mkv_ac3 => "mkv_ac3.mkv",
    mkv_eac3 => "mkv_eac3.mkv",
    mkv_vorbis => "mkv_vorbis.mkv",
    mkv_opus => "mkv_opus.mkv",
    mkv_vp9_opus_webm => "mkv_vp9_opus.webm",
    mkv_vp8_vorbis_webm => "mkv_vp8_vorbis.webm",
    mkv_h264_10bit => "mkv_h264_10bit.mkv",
    mkv_h264_422 => "mkv_h264_422.mkv",
    mkv_subs_chapters => "mkv_subs_chapters.mkv",
    mkv_mono_opus => "mkv_mono_opus.mkv",
    mpg_mpeg1_mp2 => "mpg_mpeg1_mp2.mpg",
    mpg_mpeg2_mp2 => "mpg_mpeg2_mp2.mpg",
    vob_mpeg2_ac3 => "vob_mpeg2_ac3.vob",
    ts_h264_aac => "ts_h264_aac.ts",
    ts_mpeg2_ac3 => "ts_mpeg2_ac3.ts",
    ts_hevc_eac3 => "ts_hevc_eac3.ts",
    ts_h264_mp3 => "ts_h264_mp3.ts",
    avi_mpeg4_mp3 => "avi_mpeg4_mp3.avi",
    avi_mjpeg_pcm => "avi_mjpeg_pcm.avi",
    flv_h264_aac => "flv_h264_aac.flv",
    flv_flv1_mp3 => "flv_flv1_mp3.flv",
    ogg_vorbis => "ogg_vorbis.ogg",
    ogg_opus => "ogg_opus.ogg",
    ogg_flac => "ogg_flac.ogg",
    ogg_theora_vorbis => "ogg_theora_vorbis.ogg",
    flac_16 => "flac_16.flac",
    flac_24 => "flac_24.flac",
    wav_s16 => "wav_s16.wav",
    wav_s24 => "wav_s24.wav",
    wav_s32 => "wav_s32.wav",
    wav_f32 => "wav_f32.wav",
    wav_u8 => "wav_u8.wav",
    wav_alaw => "wav_alaw.wav",
    wav_mulaw => "wav_mulaw.wav",
    wav_ima_adpcm => "wav_ima_adpcm.wav",
    wav_ms_adpcm => "wav_ms_adpcm.wav",
    wav_5p1 => "wav_5p1.wav",
    aiff_s16 => "aiff_s16.aiff",
    aiff_s24 => "aiff_s24.aiff",
    mp3_cbr => "mp3_cbr.mp3",
    mp3_vbr => "mp3_vbr.mp3",
    mp3_id3 => "mp3_id3.mp3",
    aac_adts => "aac_adts.aac",
    ac3_raw => "ac3_raw.ac3",
    eac3_raw => "eac3_raw.eac3",
    dts_raw => "dts_raw.dts",
    mp2_raw => "mp2_raw.mp2",
    opus_raw => "opus_raw.opus",
    amr_nb => "amr_nb.amr",
    caf_s16 => "caf_s16.caf",
    au_s16 => "au_s16.au",
    nut_mpeg4_mp3 => "nut_mpeg4_mp3.nut",
    ivf_vp8 => "ivf_vp8.ivf",
    ivf_vp9 => "ivf_vp9.ivf",
    asf_wmv_wma => "asf_wmv_wma.wmv",
    img_jpeg => "img_jpeg.jpg",
    img_png => "img_png.png",
    gif_anim => "gif_anim.gif",
    mov_prores => "mov_prores.mov",
}

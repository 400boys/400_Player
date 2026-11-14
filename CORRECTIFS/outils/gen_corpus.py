#!/usr/bin/env python3
"""Generate a comprehensive multimedia test corpus with ffmpeg for 400_Player testing.

Output: <corpus_dir>/<name> files + expected.json (ffprobe-derived ground truth).
"""
import json
import os
import subprocess
import sys

CORPUS = sys.argv[1] if len(sys.argv) > 1 else "/tmp/400_Player/target/test-media-corpus"
os.makedirs(CORPUS, exist_ok=True)

# 2-second test source: colour bars + moving pattern, 160x90 (mod-2), 25 fps
V = ["-f", "lavfi", "-i", "testsrc2=size=160x90:rate=25:duration=2"]
A = ["-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000:duration=2"]
SHORT = ["-t", "2"]

# (filename, ffmpeg args, expect_play)  expect_play=False → player may reject the codec,
# we still record probe behaviour (must not crash).
CASES = [
    # --- MP4 family -------------------------------------------------------
    ("mp4_h264_aac.mp4", [*V, *A, "-map", "0:v", "-map", "1:a", "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "96k"], True),
    ("mp4_hevc_aac.mp4", [*V, *A, "-map", "0:v", "-map", "1:a", "-c:v", "libx265", "-preset", "ultrafast", "-pix_fmt", "yuv420p", "-tag:v", "hvc1", "-c:a", "aac"], True),
    ("mp4_mpeg4.mp4", [*V, "-an", "-c:v", "mpeg4", "-q:v", "6"], True),
    ("mp4_mjpeg.mp4", [*V, "-an", "-c:v", "mjpeg", "-q:v", "5"], True),
    ("mp4_10bit_hevc.mp4", [*V, "-an", "-c:v", "libx265", "-preset", "ultrafast", "-pix_fmt", "yuv420p10le", "-tag:v", "hvc1"], True),
    ("mp4_h264_high10.mp4", [*V, "-an", "-c:v", "libx264", "-preset", "ultrafast", "-profile:v", "high10", "-pix_fmt", "yuv420p10le"], True),
    ("m4a_aac.m4a", [*A, "-c:a", "aac", "-b:a", "128k"], True),
    ("mov_h264_pcm.mov", [*V, *A, "-map", "0:v", "-map", "1:a", "-c:v", "libx264", "-preset", "ultrafast", "-c:a", "pcm_s16le"], True),
    ("mp4_multi_audio_subs.mp4", [*V, *A, "-f", "lavfi", "-i", "sine=frequency=880:sample_rate=44100:duration=2",
        "-map", "0:v", "-map", "1:a", "-map", "2:a", "-c:v", "libx264", "-preset", "ultrafast", "-c:a", "aac",
        "-metadata:s:a:0", "language=eng", "-metadata:s:a:1", "language=fre",
        "-c:s", "mov_text", "-metadata:s:s:0", "language=fre"], False),
    # --- Matroska / WebM ----------------------------------------------------
    ("mkv_h264_aac.mkv", [*V, *A, "-map", "0:v", "-map", "1:a", "-c:v", "libx264", "-preset", "ultrafast", "-c:a", "aac"], True),
    ("mkv_hevc_flac.mkv", [*V, *A, "-map", "0:v", "-map", "1:a", "-c:v", "libx265", "-preset", "ultrafast", "-c:a", "flac"], True),
    ("mkv_mpeg4_mp3.mkv", [*V, *A, "-map", "0:v", "-map", "1:a", "-c:v", "mpeg4", "-q:v", "6", "-c:a", "libmp3lame"], True),
    ("mkv_pcm_s16le.mkv", [*A, "-c:a", "pcm_s16le"], True),
    ("mkv_ac3.mkv", [*A, "-c:a", "ac3"], True),
    ("mkv_eac3.mkv", [*A, "-c:a", "eac3"], True),
    ("mkv_vorbis.mkv", [*A, "-c:a", "libvorbis"], True),
    ("mkv_opus.mkv", [*A, "-c:a", "libopus"], True),
    ("mkv_vp9_opus.webm", [*V, *A, "-map", "0:v", "-map", "1:a", "-c:v", "libvpx-vp9", "-deadline", "realtime", "-cpu-used", "8", "-c:a", "libopus"], True),
    ("mkv_vp8_vorbis.webm", [*V, *A, "-map", "0:v", "-map", "1:a", "-c:v", "libvpx", "-deadline", "realtime", "-cpu-used", "8", "-c:a", "libvorbis"], True),
    ("mkv_h264_10bit.mkv", [*V, "-an", "-c:v", "libx264", "-preset", "ultrafast", "-profile:v", "high10", "-pix_fmt", "yuv420p10le"], True),
    ("mkv_h264_422.mkv", [*V, "-an", "-c:v", "libx264", "-preset", "ultrafast", "-profile:v", "high422", "-pix_fmt", "yuv422p"], True),
    ("mkv_subs_chapters.mkv", [*V, *A, "-map", "0:v", "-map", "1:a", "-c:v", "libx264", "-preset", "ultrafast", "-c:a", "aac",
        "-c:s", "srt", "-metadata:s:s:0", "language=fre"], False),
    ("mkv_mono_opus.mkv", ["-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000:duration=2", "-af", "pan=mono|c0=c0", "-c:a", "libopus"], True),
    # --- MPEG-PS / VOB / mpg ------------------------------------------------
    ("mpg_mpeg1_mp2.mpg", [*V, *A, "-map", "0:v", "-map", "1:a", "-c:v", "mpeg1video", "-q:v", "6", "-c:a", "mp2"], True),
    ("mpg_mpeg2_mp2.mpg", [*V, *A, "-map", "0:v", "-map", "1:a", "-c:v", "mpeg2video", "-q:v", "6", "-c:a", "mp2"], True),
    ("vob_mpeg2_ac3.vob", [*V, *A, "-map", "0:v", "-map", "1:a", "-c:v", "mpeg2video", "-q:v", "6", "-c:a", "ac3", "-f", "vob"], True),
    # --- MPEG-TS --------------------------------------------------------------
    ("ts_h264_aac.ts", [*V, *A, "-map", "0:v", "-map", "1:a", "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", "-c:a", "aac", "-f", "mpegts"], True),
    ("ts_mpeg2_ac3.ts", [*V, *A, "-map", "0:v", "-map", "1:a", "-c:v", "mpeg2video", "-q:v", "6", "-c:a", "ac3", "-f", "mpegts"], True),
    ("ts_hevc_eac3.ts", [*V, *A, "-map", "0:v", "-map", "1:a", "-c:v", "libx265", "-preset", "ultrafast", "-pix_fmt", "yuv420p", "-c:a", "eac3", "-f", "mpegts"], True),
    ("ts_h264_mp3.ts", [*V, *A, "-map", "0:v", "-map", "1:a", "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", "-c:a", "libmp3lame", "-f", "mpegts"], True),
    # --- AVI --------------------------------------------------------------
    ("avi_mpeg4_mp3.avi", [*V, *A, "-map", "0:v", "-map", "1:a", "-c:v", "mpeg4", "-q:v", "6", "-c:a", "libmp3lame"], True),
    ("avi_mjpeg_pcm.avi", [*V, *A, "-map", "0:v", "-map", "1:a", "-c:v", "mjpeg", "-q:v", "5", "-c:a", "pcm_s16le"], True),
    # --- FLV --------------------------------------------------------------
    ("flv_h264_aac.flv", [*V, *A, "-map", "0:v", "-map", "1:a", "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", "-c:a", "aac", "-f", "flv"], True),
    ("flv_flv1_mp3.flv", [*V, *A, "-map", "0:v", "-map", "1:a", "-c:v", "flv", "-q:v", "6", "-c:a", "libmp3lame", "-f", "flv"], True),
    # --- Ogg ---------------------------------------------------------------
    ("ogg_vorbis.ogg", [*A, "-c:a", "libvorbis"], True),
    ("ogg_opus.ogg", [*A, "-c:a", "libopus"], True),
    ("ogg_flac.ogg", [*A, "-c:a", "flac", "-f", "ogg"], True),
    ("ogg_theora_vorbis.ogg", [*V, *A, "-map", "0:v", "-map", "1:a", "-c:v", "libtheora", "-q:v", "7", "-c:a", "libvorbis"], True),
    # --- FLAC --------------------------------------------------------------
    ("flac_16.flac", [*A, "-c:a", "flac", "-sample_fmt", "s16"], True),
    ("flac_24.flac", [*A, "-c:a", "flac", "-sample_fmt", "s32"], True),
    # --- WAV family ----------------------------------------------------------
    ("wav_s16.wav", [*A, "-c:a", "pcm_s16le"], True),
    ("wav_s24.wav", [*A, "-c:a", "pcm_s24le"], True),
    ("wav_s32.wav", [*A, "-c:a", "pcm_s32le"], True),
    ("wav_f32.wav", [*A, "-c:a", "pcm_f32le"], True),
    ("wav_u8.wav", [*A, "-c:a", "pcm_u8"], True),
    ("wav_alaw.wav", [*A, "-c:a", "pcm_alaw"], True),
    ("wav_mulaw.wav", [*A, "-c:a", "pcm_mulaw"], True),
    ("wav_ima_adpcm.wav", [*A, "-c:a", "adpcm_ima_wav"], True),
    ("wav_ms_adpcm.wav", [*A, "-c:a", "adpcm_ms"], True),
    ("wav_5p1.wav", ["-f", "lavfi", "-i", "anoisesrc=d=2:c=pink:r=48000:a=0.5", "-af", "pan=5.1|c0=c0|c1=c0|c2=c0|c3=c0|c4=c0|c5=c0", "-c:a", "pcm_s16le"], True),
    # --- AIFF ----------------------------------------------------------------
    ("aiff_s16.aiff", [*A, "-c:a", "pcm_s16be", "-f", "aiff"], True),
    ("aiff_s24.aiff", [*A, "-c:a", "pcm_s24be", "-f", "aiff"], True),
    # --- raw streams -----------------------------------------------------------
    ("mp3_cbr.mp3", [*A, "-c:a", "libmp3lame", "-b:a", "128k", "-write_xing", "0"], True),
    ("mp3_vbr.mp3", [*A, "-c:a", "libmp3lame", "-q:a", "4"], True),
    ("mp3_id3.mp3", [*A, "-c:a", "libmp3lame", "-b:a", "128k", "-id3v2_version", "3",
        "-metadata", "title=Test Titre", "-metadata", "artist=Artiste 400"], True),
    ("aac_adts.aac", [*A, "-c:a", "aac", "-f", "adts"], True),
    ("ac3_raw.ac3", [*A, "-c:a", "ac3", "-f", "ac3"], True),
    ("eac3_raw.eac3", [*A, "-c:a", "eac3", "-f", "eac3"], True),
    ("dts_raw.dts", [*A, "-c:a", "dca", "-f", "dts"], True),
    ("mp2_raw.mp2", [*A, "-c:a", "mp2", "-b:a", "192k", "-f", "mp2"], True),
    ("opus_raw.opus", [*A, "-c:a", "libopus", "-f", "opus"], True),
    ("amr_nb.amr", ["-f", "lavfi", "-i", "sine=frequency=440:sample_rate=8000:duration=2", "-c:a", "libopencore_amrnb", "-f", "amr"], True),
    # --- exotic containers (parsed by the player) --------------------------------
    ("caf_s16.caf", [*A, "-c:a", "pcm_s16le", "-f", "caf"], True),
    ("au_s16.au", [*A, "-c:a", "pcm_s16be", "-f", "au"], True),
    ("nut_mpeg4_mp3.nut", [*V, *A, "-map", "0:v", "-map", "1:a", "-c:v", "mpeg4", "-q:v", "6", "-c:a", "libmp3lame", "-f", "nut"], True),
    ("ivf_vp8.ivf", [*V, "-an", "-c:v", "libvpx", "-deadline", "realtime", "-cpu-used", "8", "-f", "ivf"], True),
    ("ivf_vp9.ivf", [*V, "-an", "-c:v", "libvpx-vp9", "-deadline", "realtime", "-cpu-used", "8", "-f", "ivf"], True),
    ("asf_wmv_wma.wmv", [*V, *A, "-map", "0:v", "-map", "1:a", "-c:v", "wmv2", "-q:v", "6", "-c:a", "wmav2", "-f", "asf"], True),
    # --- images ---------------------------------------------------------------
    ("img_jpeg.jpg", ["-f", "lavfi", "-i", "testsrc2=size=160x90:rate=1:duration=1", "-frames:v", "1", "-q:v", "3"], True),
    ("img_png.png", ["-f", "lavfi", "-i", "testsrc2=size=160x90:rate=1:duration=1", "-frames:v", "1"], True),
    ("gif_anim.gif", ["-f", "lavfi", "-i", "testsrc2=size=160x90:rate=10:duration=2"], True),
    # --- prores --------------------------------------------------------------
    ("mov_prores.mov", [*V, "-an", "-c:v", "prores_ks", "-profile:v", "0"], True),
]

def run(args):
    p = subprocess.run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", *args, "-nostdin"],
                       capture_output=True, text=True)
    return p.returncode, p.stderr.strip()

ok, failed = [], []
for name, args, _ in CASES:
    rc, err = run([*args, os.path.join(CORPUS, name)])
    (ok if rc == 0 else failed).append(name)
    if rc != 0:
        print(f"FFMPEG FAIL {name}: {err[-200:]}")

print(f"\nGenerated {len(ok)}/{len(CASES)} files into {CORPUS}")
if failed:
    print("Failed:", failed)

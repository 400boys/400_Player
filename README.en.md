# 400_Player (English Documentation)

**400_Player** is a modern, **100% standalone pure-Rust** GPU-accelerated media player (`#![forbid(unsafe_code)]`, **zero FFmpeg dependency**) featuring a dark glassmorphic **Violet-400 (`#A78BFA`)** aesthetic, built with `egui`/`eframe` 0.37, `wgpu` 30, and `cpal`.

## Key Features

- **100% Standalone Pure-Rust Engine (`#![forbid(unsafe_code)]`, 0 FFmpeg)**:
  - 64-bit streaming demuxing and decoding (`MediaReader` + `PacketPayload::Slice`) supporting files of any size (from 1 KB to 100+ GB 4K/8K remuxes) without loading media payloads into RAM.
  - Native pure-Rust **5-tap `bwdif` motion-adaptive spatial-temporal deinterlacer** (`Auto`, `On`, `Off`, toggled with `D`).
- **GPU Video Pipeline (`wgpu` + WGSL)**:
  - Direct upload of 8-bit (`YUV420P`, `YUV422P`, `YUV444P`, `NV12`, `RGBA`) and 10/12/16-bit (`YUV420P10LE`, `P010LE`, `P016LE`) textures.
  - Single-pass WGSL shader supporting BT.601, BT.709, BT.2020, and SMPTE 240M matrices, HDR10 (SMPTE ST 2084 PQ) and HLG (ARIB STD-B67) tone-mapping to SDR (BT.2408), colour adjustments, rotation/mirror/zoom/pan, and triangular dithering.
- **Real-Time Lock-Free Audio & DSP**:
  - Audio-driven master clock (`PlaybackClock` seqlock) with hardware latency compensation.
  - Pitch-preserving WSOLA time-stretching (`0.25×` to `4.0×`), 10-band parametric equalizer, Night Mode compressor, and 48-band FFT spectrum visualizer.
- **Complete Subtitle Support**:
  - Text subtitles: **SRT**, **WebVTT**, **ASS/SSA**, `mov_text` with automatic character encoding detection (`chardetng`).
  - Bitmap subtitles: **Blu-ray PGS**, **DVD VobSub**, and **DVB** rendered as GPU textures scaled to the video viewport.

## Quick Build

```bash
cargo build --release --locked
./target/release/400_Player path/to/media.mkv
```
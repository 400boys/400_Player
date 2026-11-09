# 400_Player (400 Media Player)

**400_Player** is a modern, **100% standalone pure-Rust** GPU-accelerated media player (`#![forbid(unsafe_code)]`, **zero FFmpeg dependency, zero DLL**) featuring customizable dark glassmorphic themes and skins, built with `egui`/`eframe` 0.37, `wgpu` 30, and `cpal`.

## Key Features

- **100% Standalone Pure-Rust Engine (`#![forbid(unsafe_code)]`, 0 FFmpeg, 0 DLL)**:
  - Multi-threaded H.264/AVC (`ThreadedDecoder`) and HEVC decoders delivering 60+ fps 1080p/4K real-time playback.
  - High-performance FFT-accelerated AAC-LC / HE-AAC IMDCT ($O(N \log N)$), E-AC3 5.1 / AC-3, MP3, and FLAC decoders.
  - 64-bit streaming demuxing (`MediaReader` + `PacketPayload::Slice`) with instant Matroska `Cues` / `SeekHead` index lookups for microsecond seeks.
  - Native pure-Rust **5-tap `bwdif` motion-adaptive spatial-temporal deinterlacer** (`Auto`, `On`, `Off`, toggled with `D`).
- **GPU Video Pipeline (`wgpu` + WGSL)**:
  - DirectX 12 default on Windows, with Vulkan / Metal cross-platform support.
  - Direct upload of 8-bit (`YUV420P`, `YUV422P`, `YUV444P`, `NV12`, `RGBA`) and 10/12/16-bit (`YUV420P10LE`, `P010LE`, `P016LE`) textures.
  - Single-pass WGSL shader supporting BT.601, BT.709, BT.2020 matrices, HDR10 (PQ) and HLG tone-mapping to SDR, rotation, zoom, and ambient glow.
- **Dual Interface Design**:
  - **Simple Mode (Default)**: Minimalist home view centered on the 400Player logo, instant open buttons, and recent media with progress bars.
  - **Advanced Mode (Studio Dashboard)**: Full-featured 212px sidebar navigation (Home, Videos, Music, Favorites), Hero banner, categorized media grid, and live skin selector (Mint `#3de6c9`, Iris `#a095ff`, Amber `#ffbc76`, Rose `#fb7185`).
- **Real-Time Lock-Free Audio & DSP**:
  - Audio-driven master clock with hardware latency compensation.
  - Pitch-preserving WSOLA time-stretching (`0.25×` to `4.0×`), 10-band equalizer, Night Mode compressor, and 48-band spectrum visualizer.

## Quick Build

```bash
cargo build --profile fast --bin 400_Media_Player
./target/fast/400_Media_Player.exe path/to/media.mkv
```

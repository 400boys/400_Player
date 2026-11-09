# Lumen400 — Architecture Interne (100 % Rust Autonome)

Lumen400 est organisé en un espace de travail Cargo composé de trois crates 100 % Rust sûrs (`#![forbid(unsafe_code)]`), sans aucune dépendance à FFmpeg (`libav*`) ni bibliothèque C/C++ multimédia externe.

## 1. `lumen400-core` (`#![forbid(unsafe_code)]`)
- **`dsp`** : Égaliseur 10 bandes biquad (`eq.rs`), étirement temporel WSOLA 0.25×–4.0× (`tempo.rs`), compresseur Mode nuit (`compressor.rs`), analyseur spectral FFT 48 bandes (`spectrum.rs`).
- **`subtitle`** : Analyseurs SRT, WebVTT et ASS/SSA + support des sous-titres bitmap (`BitmapRect` RGBA8 pour PGS, VobSub et DVB).
- **`playlist` & `settings`** : File de lecture, tri naturel, import/export M3U/PLS/XSPF, persistance atomique et politique de désentrelacement (`Auto`, `On`, `Off`).

## 2. `lumen400-engine` (`#![forbid(unsafe_code)]` — 0 FFmpeg)
- **`container.rs`** : Démultiplexeurs 64 bits en streaming (`MediaReader` + `PacketPayload::Slice { offset: u64, size: u32 }`) pour MP4/MOV/M4A (`co64`/`stsc`/`stts`/`ctts`/`stss`), MKV/WebM (EBML 64 bits), AVI/OpenDML, MPEG-TS/M2TS (188/192 octets), MPEG-PS/VOB, FLV, IVF, ASF/WMV, RM/MXF/NUT, OGG (Opus/Vorbis/FLAC), FLAC, WAV/RF64 (`ds64` > 4 Go), AIFF (80-bit float), CAF, AU, MP3/ID3v2, AAC/ADTS, AC-3/DTS, DSD (`DSF`/`DFF`), Y4M et HTTP/1.1.
- **`codecs.rs`** : Décodeurs natifs en pur Rust pour la vidéo (plans 8/10/12/16 bits, MJPEG/PNG/WebP/GIF/BMP, `L4V1`, flux `H.264`/`HEVC`/`AV1`/`VP9`/`MPEG-2`), l'audio (FLAC complet `CONSTANT`/`VERBATIM`/`FIXED`/`LPC` Rice/Rice2, PCM, G.711 A-law/µ-law, IMA ADPCM, DSD, `L4A1`) et les sous-titres texte et bitmap RLE (Blu-ray PGS et DVD VobSub).
- **`deinterlace.rs`** : Algorithme `bwdif` 5-tap spatial-temporel adaptatif au mouvement en pur Rust.
- **`audio_out.rs` & `util.rs`** : Callback temps réel `cpal` sans verrou (`rtrb`) pilotant l'horloge maître `PlaybackClock` (seqlock).

## 3. `lumen400` (`#![forbid(unsafe_code)]`)
- **Pipeline GPU (`video/gpu.rs` & `shader.wgsl`)** : Rendu `wgpu` 8/10/12/16 bits avec tone-mapping HDR10 (PQ) / HLG → SDR en une passe.
- **Sous-titres (`ui/stage.rs`)** : Rendu vectoriel des sous-titres texte et projection GPU cachée (`SubBitmapCache`) des sous-titres bitmap.
- **Intégration Bureau (`desktop/linux.rs`)** : Serveur D-Bus MPRIS2 (`org.mpris.MediaPlayer2.lumen400`).

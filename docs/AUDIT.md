# Lumen400 — Rapport d'Audit Indépendant & Sécurité

1. **Autonomie 100 % Rust (0 FFmpeg, `#![forbid(unsafe_code)]`)** :
   - Suppression intégrale de `ffmpeg-next` et `ffmpeg-sys-next` dans `Cargo.toml` et `Cargo.lock`.
   - Activation de `#![forbid(unsafe_code)]` sur `lumen400-core`, `lumen400-engine` et `lumen400` (**0 bloc `unsafe`** dans les 75 fichiers Rust du dépôt).
2. **Streaming 64 bits pour fichiers de toute taille (> 4 Go / 100+ Go)** :
   - Lecture paresseuse des paquets par offset 64 bits (`MediaReader` + `PacketPayload::Slice`) sans jamais charger le fichier entier en mémoire.
   - Prise en charge des boîtes ISO-BMFF 64 bits (`co64`), du WAV 64 bits (`RF64`/`ds64`), d'EBML Matroska 64 bits et de `YUV4MPEG2` en $O(1)$.
3. **Corrections fonctionnelles vérifiées** :
   - Raccourci `D` (`Action::CycleDeinterlace`), menu, palette, OSD et synchronisation lors de `reset_settings`.
   - Préservation de `--paused` lorsqu'il est combiné avec `--start` (`desktop/glue.rs`).
   - Rendu complet des sous-titres bitmap (Blu-ray PGS, DVD VobSub, DVB) via `BitmapRect` et `SubBitmapCache`.

# 400_Player (400_media_player) — Rapport d'Audit, Correctifs & Modernisation

## 1. Autonomie 100 % Rust (`#![forbid(unsafe_code)]`)
- Aucune dépendance à FFmpeg (`libav*`) ni bibliothèque C/C++ externe.
- `#![forbid(unsafe_code)]` activé sur `400_media_player-core`, `400_media_player-engine` et `400_media_player`.

## 2. Correctifs critiques appliqués (v0.2.0)
1. **Suppression de la fuite/réallocation de textures GPU par frame (`ui/welcome.rs`, `ui/advanced.rs`)** :
   - Les textures `400player-logo` et `400player-emblem-nobg` sont désormais mises en cache dans `egui::Context` (`IdTypeMap`) au lieu d'être réallouées via `ctx.load_texture` à chaque frame (60–120 Hz).
   - Le cache VRAM des miniatures de bibliothèque (`library_thumbnails` dans `app.rs`) est plafonné à 128 entrées.
2. **Écriture atomique réelle et persistance complète (`400_media_player-core/src/settings.rs`, `ui/advanced.rs`, `main.rs`)** :
   - `write_atomic` écrit dans un fichier temporaire (`.<nom>.<pid>.<seq>.tmp`), exécute `sync_all()` puis effectue un `fs::rename` atomique (avec repli sécurisé sous Windows).
   - Les favoris du Mode Avancé (`Settings::favorites`) et le choix `advanced_mode` sont désormais persistés dans `settings.json`.
   - La position de la fenêtre `(w.x, w.y)` est restaurée au démarrage via `ViewportBuilder::with_position`.
   - Internationalisation bilingue FR/EN complète ajoutée dans `ui/advanced.rs`.
3. **Harmonisation du binaire et des scripts d'empaquetage (`packaging/*`, `main.rs`)** :
   - `packaging/install.sh`, `packaging/debian/build-deb.sh` et `packaging/windows/build-windows.ps1` détectent automatiquement `400_Media_Player` et créent le lien symbolique `400_media_player`.
   - Unification du journal d'erreur (`400_Player-crash.log` et `400player-crash.log`).
4. **Moteur multimédia et colorimétrie HDR10 / HLG (`400_media_player-engine`)** :
   - Extraction des métadonnées colorimétriques ITU-T H.273 dans les conteneurs MP4 (`colr` `nclx`/`nclc` + `pasp`) et Matroska/WebM (`0x55B0` : `0x55B1`, `0x55B9`, `0x55BA` PQ/HLG, `0x55BB` BT.2020) pour activer le tone-mapping GPU HDR10 et HLG.
   - Correction du taux d'échantillonnage audio MP4 au-dessus de 65 535 Hz (96 kHz / 192 kHz) via la `timescale` `mdhd` et `AudioSampleEntry v2`.
   - Réassemblage complet des trames PES multi-paquets dans `parse_mpeg_ts_streaming`, suppression des en-têtes AVC/AAC dans `parse_flv_streaming` (avec extraction de `avcC`/`AudioSpecificConfig`), et découpage paquet par paquet selon la table de laçage dans `parse_ogg_streaming`.
   - Mise en cache de `RuntimeContext` via `OnceLock`, prise en compte de `DiscardMode::NonReference` en H.264, remontée explicite des erreurs de décodage et des codecs non supportés, et persistance d'état `PDS`/`ODS` pour les sous-titres Blu-ray PGS.
   - Suppression de la fausse détection matérielle automatique dans `hwaccel.rs` (activable uniquement via `400_MEDIA_PLAYER_HWDEC` pour diagnostic).
   - Réduction de la pile des threads workers de 16 Mio à 4 Mio.
5. **Amélioration du pipeline GPU (`video/shader.wgsl`)** :
   - Ajout d'un filtre de reconstruction bicubique Catmull-Rom 5-tap avec anti-ringing sur le plan Luma pour une mise à l'échelle nette en plein écran.

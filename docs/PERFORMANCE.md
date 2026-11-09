# Conception bas-latence et profil de performance (`PERFORMANCE.md`)

| Composant | Borne / Garantie | Implémentation |
|---|---|---|
| **Streaming disque 64 bits** | **0 chargement complet en RAM** | `MediaReader` + `PacketPayload::Slice { offset: u64, size: u32 }` (`container.rs`) pour fichiers > 4 Go / 100+ Go |
| **Callback audio temps réel** | **0 allocation, 0 mutex bloquant** | Anneaux SPSC `rtrb` (`audio_out.rs`), égaliseur biquad et compresseur pré-alloués |
| **Horloge maître A/V** | **Lectures wait-free, écritures non bloquantes** | Seqlock atomique `PlaybackClock` (`util.rs`) ; dérive A/V < ±25 ms |
| **File de paquets démultiplexés** | **≤ 48 Mio** au total | `MAX_QUEUE_BYTES = 48 << 20` dans `demux.rs` |
| **File d'images décodées (`FrameQueue`)** | **3 à 12 images (≈ 192 Mio max)** | Capacité dynamique `(192 MiB / frame_bytes).clamp(3, 12)` (`video_dec.rs`) |
| **Désentrelacement (`bwdif`)** | **Coût nul sur vidéo progressive** | Instancié uniquement à la détection d'une trame entrelacée en mode `Auto` (`deinterlace.rs`) |

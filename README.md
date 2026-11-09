# 400_Player (400 Media Player)

**Un lecteur multimédia moderne, autonome, écrit en 100% pur Rust (0 FFmpeg, 0 DLL externe).**
Rendu GPU DirectX 12 / Vulkan via `wgpu`, décodage vidéo multithreadé (H.264, HEVC), décodage audio accéléré par FFT (AAC, E-AC3, MP3, FLAC, Opus), pipeline à très faible latence, interface soignée en verre dépoli avec mode épuré et **Mode Avancé (Dashboard Studio)** intégrant un sélecteur de skins temps réel.

![400_Player Interface](docs/screenshots/01-accueil.png)

| Caractéristique | Détails |
|---|---|
| **Nom de l'application** | **400_Player** |
| **Exécutable de distribution** | **`400 Media Player.exe`** (38 Mo autonome, 0 dépendance DLL) |
| **Version** | 0.1.0 |
| **Architecture** | 100% Rust pur (`#![forbid(unsafe_code)]` dans les briques de décodage), multithread |
| **Moteur vidéo** | H.264/AVC (`ThreadedDecoder` multi-cœurs), HEVC/H.265, MPEG-4, ProRes, VP8/VP9, AV1 |
| **Moteur audio** | AAC-LC / HE-AAC (IMDCT rapide FFT $O(N \log N)$), E-AC3 5.1 / AC-3, MP3 (sync trames), Opus, Vorbis, FLAC |
| **Conteneurs** | Matroska (`.mkv`, `.webm` avec indexation Cues et SeekHead), MP4/MOV (tolérance de troncature), AVI, TS |
| **Rendu graphique** | DirectX 12 natif Windows, Vulkan, Metal (`wgpu`), shaders WGSL, tone-mapping HDR→SDR |
| **Interface** | `egui`/`eframe`, mode épuré par défaut + **Mode Avancé Dashboard** (sidebar 212px, hero banner, reprise de lecture, bibliothèque filtrable, skins Menthe/Iris/Ambre/Rose) |
| **Licence** | MIT ou Apache-2.0 |

---

## 1. Fonctionnalités principales

### 🚀 Performance & Décodage pur Rust
* **Zéro DLL, zéro dépendance externe** : Aucune installation de FFmpeg, de codecs tiers ou de runtime C++ n'est requise. L'exécutable est 100% autonome.
* **Décodage vidéo multithreadé** : Décodage parallèle exploitant tous les cœurs du processeur pour une lecture 1080p et 4K fluide à 60+ fps sans saccades.
* **Audio haute fidélité sans coupure** : Algorithme IMDCT rapide par transformée de Fourier (`rustfft`) réduisant le temps de calcul audio de 40 ms à 55 µs par paquet, éliminant les décrochages audio (`underruns = 0`).
* **Navigation instantanée dans les vidéos** : Lecture de l'index Matroska (`Cues`) et `SeekHead` permettant des sauts instantanés en 1 microseconde en mémoire vive sans re-seek aveugle.

### 🎨 Deux interfaces en une : Mode Simple & Mode Avancé
* **Mode Simple (par défaut)** : Écran d'accueil épuré, centré sur le logo 400Player officiel, les boutons d'accès rapide (*Ouvrir un fichier*, *Dossier*, *Lien*, *✨ Mode Avancé*) et la liste des derniers médias visionnés avec barre de progression.
* **Mode Avancé (Dashboard Studio)** :
  - **Barre latérale (212px)** : Navigation rapide (*Accueil*, *Vidéos*, *Musique*, *Favoris*), accès *À l'écran*, *File d’attente*, *Flux réseau*, et carte d'ajout de dossiers.
  - **Bannière Hero** : Présentation *« VOTRE NOUVEL ESPACE MÉDIA — Votre cinéma. Votre rythme. »* avec cercles concentriques et emblème officiel.
  - **Reprendre le fil** : Présentation dynamique des médias interrompus avec pourcentage visionné.
  - **Bibliothèque complète** : Grille réactive de cartes de médias avec badges de durée, boutons favoris et recherche instantanée.
  - **Sélecteur de Skins en direct** : Bascule instantanée de la palette de couleurs entre **Menthe** (`#3de6c9`), **Iris** (`#a095ff`), **Ambre** (`#ffbc76`), **Rose** (`#fb7185`) ainsi qu'un commutateur Thème Sombre / Clair.

---

## 2. Raccourcis clavier essentiels

| Raccourci | Action |
|---|---|
| `Espace` / `K` | Lecture / Pause |
| `←` / `→` | Saut court (5 s) |
| `Maj + ←` / `Maj + →` | Saut moyen (10 s) |
| `Ctrl + ←` / `Ctrl + →` | Saut long (60 s) |
| `↑` / `↓` | Volume haut / bas |
| `M` | Activer / désactiver le son (Muet) |
| `F` / `F11` | Plein écran |
| `Ctrl + O` | Ouvrir un fichier |
| `Ctrl + K` | Palette de commandes rapides |
| `F1` | Aide et liste complète des raccourcis |
| `Ctrl + Q` | Quitter l'application |

---

## 3. Compiler le projet

Le projet est développé avec la chaîne d'outils Rust standard.

### Prérequis
- Rust 1.88 ou supérieur (édition 2024 recommandée)

### Compilation optimisée (profil rapide)
```bash
cargo build --profile fast --bin 400_Media_Player
```
L'exécutable compilé se trouve dans `target/fast/400_Media_Player.exe`.

### Compilation finale release
```bash
cargo build --release --bin 400_Media_Player
```

---

## 4. Architecture des crates

```text
400_Player/
├── Cargo.toml
├── assets/                      # Logos officiels 400Player, icônes (.ico, .svg, .rgba) et polices
├── crates/
│   ├── lumen400-core/           # Types, réglages, historique, DSP (égaliseur, compression, spectre), playlists
│   ├── lumen400-engine/         # Moteur A/V : conteneurs (MKV, MP4...), décodeurs multithread, horloge maître
│   ├── lumen400/                # Application graphique : interface egui, rendu WGPU DX12, Mode Avancé
│   ├── rust_h264/               # Décodeur H.264 pure-Rust avec ThreadedDecoder multi-cœurs
│   └── oxideav-aac/             # Décodeur AAC pure-Rust avec IMDCT FFT ultra-rapide
└── dist-windows/                # Exécutables autonomes prêts à l'emploi
```

---

## 5. Licences

* **400_Player** est distribué sous licence double **MIT ou Apache-2.0** (`LICENSE-MIT`, `LICENSE-APACHE`).
* Polices embarquées : **Inter** et **JetBrains Mono** (SIL OFL 1.1), icônes **Phosphor** (MIT).

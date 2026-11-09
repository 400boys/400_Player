# Développer Lumen400

Ce document décrit la façon de compiler, de tester et de vérifier visuellement le lecteur.

## Prérequis (Linux)

| Quoi | Pourquoi |
|---|---|
| Rust stable ≥ 1.97 (`rust-toolchain.toml`), édition 2024 | compilateur |
| `libclang` (ex. `libclang-18-dev`) et `LIBCLANG_PATH` | `bindgen` pour `ffmpeg-sys-next` |
| Bibliothèques de développement FFmpeg 6.1 ou plus (`libavcodec`, `libavformat`, `libavutil`, `libswscale`, `libswresample`, `libavfilter`, `libavdevice` non requis) | décodage, conversion, désentrelacement |
| `libasound2-dev` | sortie audio (`cpal`) |
| `pkg-config` | détection de FFmpeg |
| Un pilote Vulkan, GL ou Metal/D3D | rendu `wgpu` (en CI : Mesa « lavapipe », rendu logiciel) |
| `ffmpeg` (binaire) | les tests d'intégration du moteur fabriquent leurs médias avec lui ; ils sont **ignorés** s'il manque |
| `dbus-daemon` (optionnel) | 7 tests du bus de session, ignorés s'il manque |
| `xvfb`, `xdotool` (optionnel) | captures d'écran et scripts d'interface sans écran |

## Commandes

```bash
export LIBCLANG_PATH=/usr/lib/llvm-18/lib          # adapter à votre machine

cargo build -p lumen400 --profile fast             # optimisé, sans LTO : rapide à compiler
cargo build -p lumen400 --release                  # LTO « fat » : lent à compiler (≈ plusieurs minutes), binaire final

cargo test   --workspace --profile fast            # tout (≈ 3 min sur 2 cœurs)
cargo clippy --workspace --all-targets --profile fast
cargo fmt --all                                    # rustfmt.toml : largeur 180
```

Les tests d'intégration du moteur (`crates/lumen400-engine/tests/playback.rs`) jouent de vrais
fichiers générés à la demande dans `target/test-media/` avec la sortie audio **virtuelle** (aucune
carte son requise). Ils sont sérialisés (verrou `serial()`) parce que certains mesurent des durées.

## Pilotage automatique de l'interface

`lumen400 --automation script.txt` exécute un script d'interface (captures d'écran, vérifications de bout en bout).
Un script est un fichier texte, une commande par ligne (`#` commence un commentaire) :

```text
wait 1.5                 # secondes
wait_play 3              # jusqu'à ce que la position de lecture atteigne 3 s
key ctrl+k               # une touche (modificateurs : ctrl, shift, alt)
text bonjour             # des caractères tapés
move 400 300             # également : click / rclick / dclick x y
scroll -1                # molette, en crans
action ToggleInfo        # n'importe quelle commande de la table d'actions
open /chemin/film.mp4    # remplace la file de lecture
tab playback             # ouvre les réglages sur un onglet
set theme light          # theme dark|oled|light|system, accent <nom>, lang fr|en
resize 1280 760
shot accueil.png         # capture de la fenêtre (à côté du script si le chemin est relatif)
expect state playing     # idle|opening|buffering|playing|paused|ended|failed
expect pos_ge 2.5
quit
```

Une ligne `expect` qui échoue est signalée sur `stderr` et le processus sort avec un code non nul.

### Recette : captures d'écran sans écran (Linux)

```bash
export WGPU_BACKEND=vulkan LIBGL_ALWAYS_SOFTWARE=1       # rendu logiciel (Mesa lavapipe)
xvfb-run -a -s "-screen 0 1600x1000x24" \
  target/fast/lumen400 --ephemeral --virtual-audio --automation mon_script.txt
```

`--ephemeral` ne lit ni n'écrit les réglages et l'historique de l'utilisateur ; `--virtual-audio`
ne touche jamais à la carte son.

## Bureau (MPRIS, fenêtre unique)

Les tests de `crates/lumen400/src/desktop/linux.rs` lancent un `dbus-daemon` **privé** (jamais le
bus de session de l'utilisateur) et vérifient : les propriétés et méthodes MPRIS, les signaux
`PropertiesChanged` et `Seeked`, le transfert d'un second lancement vers la fenêtre ouverte
(`lumen400.Control.Open`) et la protection du nom de bus contre le vol par un nouvel arrivant.
La liste des types MIME de `desktop::MIME_TYPES` doit être identique à la ligne `MimeType=` de
`packaging/lumen400.desktop` (un test le vérifie dès que ce fichier existe).

## Conventions

* `unsafe` : interdit dans l'intégralité du dépôt (`#![forbid(unsafe_code)]` actif sur `lumen400-core`, `lumen400-engine` et `lumen400`). Le moteur multimédia est 100 % pur Rust autonome, sans aucun appel FFI ni dépendance à FFmpeg (`docs/AUDIT.md`).
* Rien ne bloque le thread d'interface : tout ce qui peut attendre (D-Bus, disque, réseau, veille) vit sur un thread dédié.
* Chaque texte de l'interface existe en français **et** en anglais (macro `translations!` dans `i18n.rs`).
* Un comportement corrigé = un test qui échoue avant la correction.

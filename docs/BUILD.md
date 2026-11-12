# 400_media_player — Guide de Compilation (100 % Autonome, 0 FFmpeg)

## 1. Prérequis
- **Rust** : `1.94.0` ou plus récent (édition 2024).
- **Aucune dépendance à FFmpeg (`libav*`) ni `libclang`** : 400_media_player compile 100 % en pur Rust (`#![forbid(unsafe_code)]`).

### Ubuntu / Debian
```bash
sudo apt-get update
sudo apt-get install -y build-essential pkg-config libasound2-dev \
  libxkbcommon-dev libwayland-dev libx11-dev libxcursor-dev libxi-dev libxrandr-dev \
  libvulkan1 mesa-vulkan-drivers
```

## 2. Compilation et Tests
```bash
cargo build --release --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked -- --test-threads=1
```

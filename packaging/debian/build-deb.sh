#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
VERSION="$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')"
ARCH="$(dpkg --print-architecture 2>/dev/null || echo amd64)"
PKG_DIR="$ROOT/target/debian/lumen400_${VERSION}_${ARCH}"
if [[ ! -x "$ROOT/target/release/lumen400" ]]; then
    cargo build --release --locked
fi
rm -rf "$PKG_DIR"
mkdir -p "$PKG_DIR/DEBIAN"
PREFIX=/usr DESTDIR="$PKG_DIR" "$ROOT/packaging/install.sh"
cat > "$PKG_DIR/DEBIAN/control" <<CTRL
Package: lumen400
Version: ${VERSION}
Section: video
Priority: optional
Architecture: ${ARCH}
Depends: libc6, libasound2t64 | libasound2, libvulkan1 | libgl1
Maintainer: Lumen400 Contributors <contact@lumen400.io>
Description: Lecteur multimédia moderne 100 % Rust autonome accéléré par GPU (Violet-400)
CTRL
dpkg-deb --build --root-owner-group "$PKG_DIR"
tar -C "$PKG_DIR/usr" -czf "$ROOT/target/debian/lumen400-${VERSION}-linux-${ARCH}.tar.gz" .

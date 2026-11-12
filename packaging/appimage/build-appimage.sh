#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VERSION="${VERSION:-0.1.0}"
ARCH="${ARCH:-x86_64}"
APPDIR="${ROOT_DIR}/target/AppDir"
rm -rf "${APPDIR}"
PREFIX=/usr DESTDIR="${APPDIR}" "${ROOT_DIR}/packaging/install.sh"
ln -sf usr/bin/400_media_player "${APPDIR}/AppRun"
ln -sf usr/share/applications/400_media_player.desktop "${APPDIR}/400_media_player.desktop"
ln -sf usr/share/icons/hicolor/256x256/apps/400_media_player.png "${APPDIR}/400_media_player.png"
ln -sf usr/share/icons/hicolor/256x256/apps/400_media_player.png "${APPDIR}/.DirIcon"
if command -v appimagetool >/dev/null 2>&1; then
    ARCH="${ARCH}" appimagetool "${APPDIR}" "${ROOT_DIR}/target/400_media_player-${VERSION}-${ARCH}.AppImage"
fi

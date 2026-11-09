#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VERSION="${VERSION:-0.1.0}"
ARCH="${ARCH:-x86_64}"
APPDIR="${ROOT_DIR}/target/AppDir"
rm -rf "${APPDIR}"
PREFIX=/usr DESTDIR="${APPDIR}" "${ROOT_DIR}/packaging/install.sh"
ln -sf usr/bin/lumen400 "${APPDIR}/AppRun"
ln -sf usr/share/applications/lumen400.desktop "${APPDIR}/lumen400.desktop"
ln -sf usr/share/icons/hicolor/256x256/apps/lumen400.png "${APPDIR}/lumen400.png"
ln -sf usr/share/icons/hicolor/256x256/apps/lumen400.png "${APPDIR}/.DirIcon"
if command -v appimagetool >/dev/null 2>&1; then
    ARCH="${ARCH}" appimagetool "${APPDIR}" "${ROOT_DIR}/target/Lumen400-${VERSION}-${ARCH}.AppImage"
fi

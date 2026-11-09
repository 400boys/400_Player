#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PREFIX="${PREFIX:-/usr/local}"
DESTDIR="${DESTDIR:-}"
TARGET_DIR="${DESTDIR}${PREFIX}"

if [[ "${1:-}" == "--uninstall" ]]; then
    rm -f "${TARGET_DIR}/bin/lumen400"
    rm -f "${TARGET_DIR}/share/applications/lumen400.desktop"
    rm -f "${TARGET_DIR}/share/metainfo/io.lumen400.Lumen400.metainfo.xml"
    rm -f "${TARGET_DIR}/share/icons/hicolor/scalable/apps/lumen400.svg"
    for size in 16 24 32 48 64 128 256 512; do
        rm -f "${TARGET_DIR}/share/icons/hicolor/${size}x${size}/apps/lumen400.png"
    done
    exit 0
fi

BIN="${BIN:-${ROOT_DIR}/target/release/lumen400}"
install -Dm755 "${BIN}" "${TARGET_DIR}/bin/lumen400"
install -Dm644 "${ROOT_DIR}/packaging/lumen400.desktop" "${TARGET_DIR}/share/applications/lumen400.desktop"
install -Dm644 "${ROOT_DIR}/packaging/io.lumen400.Lumen400.metainfo.xml" "${TARGET_DIR}/share/metainfo/io.lumen400.Lumen400.metainfo.xml"
install -Dm644 "${ROOT_DIR}/assets/icon/lumen400.svg" "${TARGET_DIR}/share/icons/hicolor/scalable/apps/lumen400.svg"
for size in 16 24 32 48 64 128 256 512; do
    install -Dm644 "${ROOT_DIR}/assets/icon/lumen400-${size}.png" "${TARGET_DIR}/share/icons/hicolor/${size}x${size}/apps/lumen400.png"
done

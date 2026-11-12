#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PREFIX="${PREFIX:-/usr/local}"
DESTDIR="${DESTDIR:-}"
TARGET_DIR="${DESTDIR}${PREFIX}"

if [[ "${1:-}" == "--uninstall" ]]; then
    rm -f "${TARGET_DIR}/bin/400_media_player"
    rm -f "${TARGET_DIR}/bin/400_Media_Player"
    rm -f "${TARGET_DIR}/share/applications/400_media_player.desktop"
    rm -f "${TARGET_DIR}/share/metainfo/io.400_media_player.400_media_player.metainfo.xml"
    rm -f "${TARGET_DIR}/share/icons/hicolor/scalable/apps/400_media_player.svg"
    for size in 16 24 32 48 64 128 256 512; do
        rm -f "${TARGET_DIR}/share/icons/hicolor/${size}x${size}/apps/400_media_player.png"
    done
    exit 0
fi

if [[ -z "${BIN:-}" ]]; then
    if [[ -x "${ROOT_DIR}/target/release/media_player_400" ]]; then
        BIN="${ROOT_DIR}/target/release/media_player_400"
    else
        BIN="${ROOT_DIR}/target/release/400_media_player"
    fi
fi

install -Dm755 "${BIN}" "${TARGET_DIR}/bin/400_media_player"
ln -sf 400_media_player "${TARGET_DIR}/bin/400_Media_Player"
install -Dm644 "${ROOT_DIR}/packaging/400_media_player.desktop" "${TARGET_DIR}/share/applications/400_media_player.desktop"
install -Dm644 "${ROOT_DIR}/packaging/io.400_media_player.400_media_player.metainfo.xml" "${TARGET_DIR}/share/metainfo/io.400_media_player.400_media_player.metainfo.xml"
install -Dm644 "${ROOT_DIR}/assets/icon/400_media_player.svg" "${TARGET_DIR}/share/icons/hicolor/scalable/apps/400_media_player.svg"
for size in 16 24 32 48 64 128 256 512; do
    install -Dm644 "${ROOT_DIR}/assets/icon/400_media_player-${size}.png" "${TARGET_DIR}/share/icons/hicolor/${size}x${size}/apps/400_media_player.png"
done

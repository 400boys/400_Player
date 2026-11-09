#!/usr/bin/env python3
"""Generate Lumen400's embedded font subsets and the Rust icon table.

Inputs (all MIT / OFL licensed, fetched from npm):
  @phosphor-icons/web          -> icon font + CSS with code points
  @expo-google-fonts/inter     -> Inter static TTFs
  @expo-google-fonts/jetbrains-mono

Usage:
  npm pack @phosphor-icons/web @expo-google-fonts/inter @expo-google-fonts/jetbrains-mono
  # extract the three tarballs to <dir>/{phos,inter,jbm}
  python3 tools/gen_assets.py <dir>

Outputs:
  assets/fonts/*.ttf                      (subsetted, a few hundred KB in total)
  crates/lumen400/src/ui/icons_gen.rs     (pub const NAME: char = '\\u{....}';)
"""
import re
import sys
from pathlib import Path

from fontTools import subset
from fontTools.ttLib import TTFont

ROOT = Path(__file__).resolve().parent.parent

# Rust constant name -> Phosphor icon name.
ICONS = {
    # transport
    "PLAY": "play", "PAUSE": "pause", "STOP": "stop",
    "SKIP_BACK": "skip-back", "SKIP_FORWARD": "skip-forward",
    "REWIND": "rewind", "FAST_FORWARD": "fast-forward",
    "ARROW_CCW": "arrow-counter-clockwise", "ARROW_CW": "arrow-clockwise",
    "REPEAT": "repeat", "REPEAT_ONCE": "repeat-once", "SHUFFLE": "shuffle",
    "PLAY_CIRCLE": "play-circle", "PAUSE_CIRCLE": "pause-circle",
    # volume
    "SPEAKER_HIGH": "speaker-high", "SPEAKER_LOW": "speaker-low",
    "SPEAKER_NONE": "speaker-none", "SPEAKER_X": "speaker-x",
    # window / view
    "ARROWS_OUT": "arrows-out", "ARROWS_IN": "arrows-in",
    "PICTURE_IN_PICTURE": "picture-in-picture", "PUSH_PIN": "push-pin",
    "MONITOR": "monitor", "MONITOR_PLAY": "monitor-play",
    "FRAME_CORNERS": "frame-corners",
    # panels
    "PLAYLIST": "playlist", "LIST_BULLETS": "list-bullets", "QUEUE": "queue",
    "SLIDERS": "sliders-horizontal", "EQUALIZER": "equalizer",
    "SUBTITLES": "subtitles", "CLOSED_CAPTIONING": "closed-captioning",
    "GEAR": "gear-six", "INFO": "info", "CHART_BAR": "chart-bar",
    "PULSE": "pulse", "COMMAND": "command", "KEYBOARD": "keyboard",
    "QUESTION": "question", "TERMINAL": "terminal-window",
    # files / sources
    "FOLDER_OPEN": "folder-open", "FOLDER": "folder-simple",
    "FILE_VIDEO": "file-video", "FILE_AUDIO": "file-audio",
    "MUSIC_NOTES": "music-notes", "MUSIC_NOTE": "music-note",
    "FILM_STRIP": "film-strip", "FILM_SLATE": "film-slate",
    "LINK": "link", "GLOBE": "globe", "TELEVISION": "television",
    # actions
    "CAMERA": "camera", "TRASH": "trash", "X": "x", "PLUS": "plus",
    "MINUS": "minus", "CHECK": "check", "COPY": "copy",
    "SEARCH": "magnifying-glass", "DOTS_THREE": "dots-three",
    "DOTS_THREE_VERTICAL": "dots-three-vertical",
    "DRAG": "dots-six-vertical", "CARET_DOWN": "caret-down",
    "CARET_UP": "caret-up", "CARET_RIGHT": "caret-right",
    "CARET_LEFT": "caret-left", "SQUARE_OUT": "arrow-square-out",
    "DOWNLOAD": "download-simple", "UPLOAD": "upload-simple",
    "FLOPPY": "floppy-disk", "EYE": "eye", "EYE_SLASH": "eye-slash",
    "BOOKMARK": "bookmark-simple", "LIST": "list",
    "CLOCK": "clock", "TIMER": "timer", "GAUGE": "gauge",
    "HOURGLASS": "hourglass-medium", "LIGHTNING": "lightning",
    # display
    "ASPECT_RATIO": "resize", "CROP": "crop",
    "ROTATE": "arrow-clockwise", "FLIP_H": "flip-horizontal",
    "FLIP_V": "flip-vertical", "ZOOM_IN": "magnifying-glass-plus",
    "ZOOM_OUT": "magnifying-glass-minus", "SUN": "sun", "MOON": "moon",
    "PALETTE": "palette", "SPARKLE": "sparkle", "MAGIC_WAND": "magic-wand",
    # audio
    "HEADPHONES": "headphones", "WAVEFORM": "waveform",
    "VINYL": "vinyl-record", "EAR": "ear",
    # feedback
    "WARNING": "warning", "WARNING_CIRCLE": "warning-circle",
    "CHECK_CIRCLE": "check-circle", "X_CIRCLE": "x-circle",
    "TRANSLATE": "translate", "TEXT_AA": "text-aa",
    # window controls
    "WIN_MIN": "minus", "WIN_MAX": "square", "WIN_RESTORE": "copy-simple",
    "WIN_CLOSE": "x",
}

INTER_RANGES = (
    "U+0020-007E,U+00A0-024F,U+0300-036F,U+0370-03FF,U+0400-04FF,U+1E00-1EFF,"
    "U+2000-206F,U+20A0-20CF,U+2100-214F,U+2190-21FF,U+2212,U+2215,U+25A0-25FF,"
    "U+FB00-FB06"
)
MONO_RANGES = "U+0020-007E,U+00A0-00FF,U+2000-206F,U+2190-21FF,U+25A0-25FF"


def parse_unicodes(spec: str):
    out = []
    for part in spec.split(","):
        part = part.strip().replace("U+", "")
        if "-" in part:
            a, b = part.split("-")
            out.extend(range(int(a, 16), int(b, 16) + 1))
        else:
            out.append(int(part, 16))
    return out


def do_subset(src: Path, dst: Path, unicodes):
    opts = subset.Options()
    opts.layout_features = ["kern", "liga", "tnum"]
    opts.hinting = False
    opts.desubroutinize = True
    opts.notdef_outline = True
    opts.name_IDs = [0, 1, 2, 3, 4, 5, 6, 13, 14]
    opts.glyph_names = False
    opts.drop_tables += ["DSIG", "FFTM", "GDEF"]
    font = TTFont(str(src))
    sub = subset.Subsetter(opts)
    sub.populate(unicodes=unicodes)
    sub.subset(font)
    font.save(str(dst))
    print(f"  {dst.name:28s} {dst.stat().st_size / 1024:7.1f} KiB")


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    base = Path(sys.argv[1])
    phos = base / "phos/package/src"
    fonts = ROOT / "assets/fonts"
    fonts.mkdir(parents=True, exist_ok=True)

    # --- Phosphor code points
    css = (phos / "regular/style.css").read_text()
    table = dict(
        (m.group(1), int(m.group(2), 16))
        for m in re.finditer(r'\.ph\.ph-([a-z0-9-]+):before\s*\{\s*content:\s*"\\([0-9a-fA-F]+)"', css)
    )
    missing = [v for v in ICONS.values() if v not in table]
    if missing:
        sys.exit(f"missing Phosphor icons: {missing}")
    cps = sorted({table[v] for v in ICONS.values()})

    print("Phosphor subsets:")
    for weight, folder, fname in (
        ("regular", "regular", "Phosphor.ttf"),
        ("bold", "bold", "Phosphor-Bold.ttf"),
        ("fill", "fill", "Phosphor-Fill.ttf"),
    ):
        do_subset(phos / folder / fname, fonts / f"Phosphor-{weight}.ttf", cps)

    print("Inter subsets:")
    for name, folder in (("Regular", "400Regular"), ("Medium", "500Medium"),
                         ("SemiBold", "600SemiBold"), ("Bold", "700Bold")):
        do_subset(base / f"inter/package/{folder}/Inter_{folder}.ttf",
                  fonts / f"Inter-{name}.ttf", parse_unicodes(INTER_RANGES))

    print("JetBrains Mono subsets:")
    do_subset(base / "jbm/package/500Medium/JetBrainsMono_500Medium.ttf",
              fonts / "JetBrainsMono-Medium.ttf", parse_unicodes(MONO_RANGES))

    # --- Licences
    (fonts / "LICENSE-Inter-OFL.txt").write_text((base / "inter/package/LICENSE_FONT").read_text())
    (fonts / "LICENSE-JetBrainsMono-OFL.txt").write_text((base / "jbm/package/LICENSE_FONT").read_text())
    (fonts / "LICENSE-Phosphor-MIT.txt").write_text((base / "phos/package/LICENSE").read_text())

    # --- Rust constants
    out = ROOT / "crates/lumen400/src/ui/icons_gen.rs"
    out.parent.mkdir(parents=True, exist_ok=True)
    lines = [
        "// @generated by tools/gen_assets.py from Phosphor Icons (MIT). Do not edit.",
        "#![allow(dead_code)]",
        "",
    ]
    for const, name in sorted(ICONS.items()):
        lines.append(f"/// ph-{name}")
        lines.append(f"pub const {const}: char = '\\u{{{table[name]:04x}}}';")
    out.write_text("\n".join(lines) + "\n")
    print(f"wrote {out.relative_to(ROOT)} ({len(ICONS)} icons)")


if __name__ == "__main__":
    main()

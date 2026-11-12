#!/usr/bin/env python3
"""Draws the 400_media_player application icon at every size the packagers need.

    python3 tools/gen_icon.py          # writes assets/icon/*

The artwork is simple on purpose — a violet-400 → indigo squircle with a rounded "play" triangle
and a soft light in the corner — so it stays legible from 16 px (taskbar) to 512 px (store pages).
"""
import math
import pathlib
from PIL import Image, ImageChops, ImageDraw, ImageFilter

OUT = pathlib.Path(__file__).resolve().parent.parent / "assets" / "icon"
SIZES = [16, 24, 32, 48, 64, 128, 256, 512]
TOP = (196, 181, 253)      # violet-300
MID = (167, 139, 250)      # violet-400  (the "400")
BOTTOM = (79, 70, 229)     # indigo-600
SS = 4                     # supersampling


def squircle_mask(n, radius_ratio=0.235):
    big = n * SS
    m = Image.new("L", (big, big), 0)
    d = ImageDraw.Draw(m)
    d.rounded_rectangle((0, 0, big - 1, big - 1), radius=int(big * radius_ratio), fill=255)
    return m


def gradient(n):
    big = n * SS
    g = Image.new("RGB", (big, big))
    px = g.load()
    for y in range(big):
        for x in range(big):
            # diagonal blend, light from the top left
            t = (x * 0.35 + y * 0.65) / big
            if t < 0.45:
                k = t / 0.45
                c = tuple(round(TOP[i] + (MID[i] - TOP[i]) * k) for i in range(3))
            else:
                k = (t - 0.45) / 0.55
                c = tuple(round(MID[i] + (BOTTOM[i] - MID[i]) * k) for i in range(3))
            px[x, y] = c
    return g


def rounded_triangle(n):
    """A play triangle with softly rounded corners, optically centred."""
    big = n * SS
    layer = Image.new("L", (big, big), 0)
    d = ImageDraw.Draw(layer)
    s = big
    left, right = 0.375 * s, 0.70 * s
    top, bottom = 0.27 * s, 0.73 * s
    pts = [(left, top), (right, s * 0.5), (left, bottom)]
    d.polygon(pts, fill=255)
    # round the corners: blur then threshold
    r = max(1, int(s * 0.035))
    layer = layer.filter(ImageFilter.GaussianBlur(r)).point(lambda v: 255 if v > 128 else 0)
    layer = layer.filter(ImageFilter.GaussianBlur(SS * 0.6))
    return layer


def render(n):
    mask = squircle_mask(n)
    base = gradient(n)
    big = n * SS
    # soft light in the upper-left corner
    glow = Image.new("L", (big, big), 0)
    ImageDraw.Draw(glow).ellipse((-big * 0.25, -big * 0.35, big * 0.75, big * 0.55), fill=120)
    glow = glow.filter(ImageFilter.GaussianBlur(big * 0.12))
    white = Image.new("RGB", (big, big), (255, 255, 255))
    base = Image.composite(white, base, glow)
    # drop shadow of the triangle
    tri = rounded_triangle(n)
    shadow = tri.filter(ImageFilter.GaussianBlur(big * 0.02)).point(lambda v: int(v * 0.35))
    shadow = ImageChops.offset(shadow, 0, int(big * 0.018))
    dark = Image.new("RGB", (big, big), (40, 20, 110))
    base = Image.composite(dark, base, shadow)
    base = Image.composite(white, base, tri)
    img = base.convert("RGBA")
    img.putalpha(mask)
    return img.resize((n, n), Image.LANCZOS)


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    images = {}
    for n in SIZES:
        img = render(n)
        img.save(OUT / f"400_media_player-{n}.png", optimize=True)
        images[n] = img
    images[256].save(OUT / "400_media_player.ico", format="ICO", sizes=[(s, s) for s in (16, 24, 32, 48, 64, 128, 256)])
    (OUT / "400_media_player.svg").write_text('<?xml version="1.0" encoding="UTF-8"?>\n<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512" width="512" height="512">\n  <defs>\n    <linearGradient id="bg-grad" x1="0%" y1="0%" x2="75%" y2="100%">\n      <stop offset="0%" stop-color="#c4b5fd" />\n      <stop offset="45%" stop-color="#a78bfa" />\n      <stop offset="100%" stop-color="#4f46e5" />\n    </linearGradient>\n    <radialGradient id="top-glow" cx="25%" cy="15%" r="55%">\n      <stop offset="0%" stop-color="#ffffff" stop-opacity="0.48" />\n      <stop offset="100%" stop-color="#ffffff" stop-opacity="0" />\n    </radialGradient>\n    <filter id="play-shadow" x="-20%" y="-20%" width="140%" height="140%">\n      <feDropShadow dx="0" dy="9" stdDeviation="10" flood-color="#28146e" flood-opacity="0.38" />\n    </filter>\n  </defs>\n  <!-- Squircle background -->\n  <rect x="0" y="0" width="512" height="512" rx="120" ry="120" fill="url(#bg-grad)" />\n  <rect x="0" y="0" width="512" height="512" rx="120" ry="120" fill="url(#top-glow)" />\n  <!-- Optically centred rounded play triangle -->\n  <path d="M 192 156\n           C 192 141, 208 132, 221 140\n           L 350 220\n           C 362 228, 362 244, 350 252\n           L 221 332\n           C 208 340, 192 331, 192 316\n           Z"\n        transform="translate(0, 20)"\n        fill="#ffffff"\n        filter="url(#play-shadow)" />\n</svg>\n', encoding="utf-8")
    print("wrote", ", ".join(sorted(p.name for p in OUT.iterdir())))


if __name__ == "__main__":
    main()

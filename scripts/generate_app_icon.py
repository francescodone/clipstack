#!/usr/bin/env python3
"""Regenerate the ClipStack application icon (1024px master).

Design: flat and 2026-oriented. One squircle tile carrying a single soft
diagonal fade through dark orange, and the menu bar mark promoted to a solid
glyph whose sheets fade from full white in front to barely-there behind. No
sheen, no rim light, no drop shadow - the depth comes from the fades alone, the
way SourceTree's tile achieves it. No badges, no clip, no text lines.

    python3 scripts/generate_app_icon.py

Writes docs/app-icon.png (the master). Feed that to `npx tauri icon` to
produce every bundle format (icns, ico, png set) — see README.
"""

import math
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

SIZE = 1024
SS = 4  # supersample factor
CANVAS = SIZE * SS

# The mark is the tray icon's geometry: a 36-unit canvas with three rounded
# sheets staggered by 4 units. Filled rather than stroked here — at tile size
# outlines read as clutter, and a fade only shows on a solid area.
UNIT = 36.0
OFFSET = 4.0
GAP = 1.1  # clear air between a sheet and the edge behind it
RADIUS = 3.0  # tray units; softer corners, the 2026 look

SHEET_W, SHEET_H = 16.0, 20.0
FRONT = (6.0, 4.0, 6.0 + SHEET_W, 4.0 + SHEET_H)
MID = tuple(c + OFFSET for c in FRONT)
BACK = tuple(c + 2 * OFFSET for c in FRONT)

# Back-to-front paint order. The fade is the whole idea: the rearmost sheet is
# barely present, the front sheet is absolute.
LAYERS = [
    (BACK, (255, 255, 255, 74)),
    (MID, (255, 255, 255, 150)),
    (FRONT, (255, 255, 255, 255)),
]

# Where the mark sits on the 1024 grid: centred on the group, about half the
# tile. The group's centre is the middle sheet's centre.
MARK_SCALE = 20.0  # tray units -> icon units
GROUP_CX = (FRONT[0] + BACK[2]) / 2
GROUP_CY = (FRONT[1] + BACK[3]) / 2
MARK_CX, MARK_CY = 512.0, 512.0

# Tile: one flat diagonal fade through dark, burnt orange. The top-left keeps
# enough light to read as orange rather than brown, and the bottom-right goes
# near-black so the glyph still carries the contrast it has in the menu bar.
TILE_A = (176, 74, 20, 255)  # top-left, lit burnt orange
TILE_B = (58, 22, 6, 255)  # bottom-right, near-black ember


def u(x):
    """Icon-grid units (0..1024) to supersampled pixels."""
    return x * SS


def squircle_path(cx, cy, half, steps=64):
    """Continuous-curve squircle as a polygon (superellipse, n=5)."""
    n = 5.0
    pts = []
    for i in range(steps * 4):
        t = 2 * math.pi * i / (steps * 4)
        c, s = math.cos(t), math.sin(t)
        x = cx + half * math.copysign(abs(c) ** (2 / n), c)
        y = cy + half * math.copysign(abs(s) ** (2 / n), s)
        pts.append((u(x), u(y)))
    return pts


def rounded_points(box, radius, steps=48):
    """Corners of a rounded rectangle as a point list, in icon units."""
    x0, y0, x1, y1 = box
    r = min(radius, (x1 - x0) / 2, (y1 - y0) / 2)
    corners = [
        (x0 + r, y0 + r, 180, 270),
        (x1 - r, y0 + r, 270, 360),
        (x1 - r, y1 - r, 0, 90),
        (x0 + r, y1 - r, 90, 180),
    ]
    pts = []
    for cx, cy, a0, a1 in corners:
        for i in range(steps + 1):
            a = math.radians(a0 + (a1 - a0) * i / steps)
            pts.append((cx + r * math.cos(a), cy + r * math.sin(a)))
    return pts


def to_px(pts):
    return [(u(x), u(y)) for x, y in pts]


def diagonal_fade(size, a, b):
    """Linear gradient running corner to corner, top-left to bottom-right.

    A one-pixel-tall strip holding the ramp is sampled at u = x + y with an
    affine transform, which is exact and avoids a per-pixel Python loop at
    supersampled sizes.
    """
    strip = Image.new("RGB", (2 * size, 1))
    px = strip.load()
    span = 2 * size - 1
    for x in range(2 * size):
        t = x / span
        px[x, 0] = tuple(int(i + (j - i) * t) for i, j in zip(a[:3], b[:3]))
    out = strip.transform(
        (size, size), Image.AFFINE, (1, 1, 0, 0, 0, 0), resample=Image.BILINEAR
    )
    return out.convert("RGBA")


def mark_box(box):
    """Map a tray-unit sheet box onto the icon grid, centred on the group."""
    x0, y0, x1, y1 = box
    cx, cy = (x0 + x1) / 2, (y0 + y1) / 2
    dx, dy = (cx - GROUP_CX) * MARK_SCALE, (cy - GROUP_CY) * MARK_SCALE
    half_w, half_h = (x1 - x0) / 2 * MARK_SCALE, (y1 - y0) / 2 * MARK_SCALE
    return (
        MARK_CX + dx - half_w,
        MARK_CY + dy - half_h,
        MARK_CX + dx + half_w,
        MARK_CY + dy + half_h,
    )


def expand(box, pad):
    x0, y0, x1, y1 = box
    return (x0 - pad, y0 - pad, x1 + pad, y1 + pad)


def draw_tile():
    """Flat squircle carrying one diagonal fade and nothing else."""
    tile = diagonal_fade(CANVAS, TILE_A, TILE_B)

    mask = Image.new("L", (CANVAS, CANVAS), 0)
    ImageDraw.Draw(mask).polygon(squircle_path(512, 512, 448), fill=255)
    # Soften the superellipse corners toward the Apple continuous look.
    mask = mask.filter(ImageFilter.GaussianBlur(2 * SS))

    img = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
    img.paste(tile, (0, 0), mask)
    return img


def draw_mark():
    """Three solid sheets, each punched clear of the sheet in front of it.

    Filled shapes rather than stroked outlines: a fade only reads on a solid
    area. The area the next sheet occupies is knocked out so the layers never
    touch, which is what keeps three translucent sheets legible.
    """
    layer = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
    pad = GAP * MARK_SCALE

    for i, (box, colour) in enumerate(LAYERS):
        mask = Image.new("L", (CANVAS, CANVAS), 0)
        md = ImageDraw.Draw(mask)
        body = to_px(rounded_points(mark_box(box), RADIUS * MARK_SCALE))
        md.polygon(body, fill=255)
        if i + 1 < len(LAYERS):
            nxt = mark_box(LAYERS[i + 1][0])
            gap = to_px(rounded_points(expand(nxt, pad), RADIUS * MARK_SCALE + pad))
            md.polygon(gap, fill=0)
        colour_img = Image.new("RGBA", (CANVAS, CANVAS), colour)
        layer.alpha_composite(Image.composite(colour_img, layer, mask))
    return layer


def main():
    img = draw_tile()
    img.alpha_composite(draw_mark())

    img = img.resize((SIZE, SIZE), Image.LANCZOS)
    out = Path(__file__).resolve().parent.parent / "docs" / "app-icon.png"
    img.save(out)
    print(f"wrote {out} ({SIZE}x{SIZE})")


if __name__ == "__main__":
    main()

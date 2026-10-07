#!/usr/bin/env python3
"""Regenerate the ClipStack application icon (1024px master).

Design: the menu bar mark — three offset sheets, the universal copy glyph —
promoted to an app tile in a restrained, VS Code-ish key: a dark graphite
squircle with a soft top sheen, and the stack drawn as clean outlines. The
sheets step from full white in front to dimmed white behind, which gives the
mark depth without ornament. No badges, no clip, no text lines.

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
# sheets staggered by 4 units, stroked 2 units wide, corners at radius 2.
UNIT = 36.0
STROKE = 2.0
RADIUS = 2.0
OFFSET = 4.0
GAP = 0.6  # clear air between a sheet and the edge behind it

SHEET_W, SHEET_H = 16.0, 20.0
FRONT = (6.0, 4.0, 6.0 + SHEET_W, 4.0 + SHEET_H)
MID = tuple(c + OFFSET for c in FRONT)
BACK = tuple(c + 2 * OFFSET for c in FRONT)

# Back-to-front paint order with the depth step: dimmed white in the back,
# full white in front.
LAYERS = [
    (BACK, (255, 255, 255, 110)),
    (MID, (255, 255, 255, 175)),
    (FRONT, (255, 255, 255, 255)),
]

# Where the mark sits on the 1024 grid: centred on the group, about half the
# tile. The group's centre is the middle sheet's centre.
MARK_SCALE = 19.0  # tray units -> icon units
GROUP_CX = (FRONT[0] + BACK[2]) / 2
GROUP_CY = (FRONT[1] + BACK[3]) / 2
MARK_CX, MARK_CY = 512.0, 512.0

TILE_TOP = (46, 46, 51, 255)
TILE_BOTTOM = (20, 20, 24, 255)


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


def vertical_gradient(size, top, bottom):
    strip = Image.new("RGBA", (1, size))
    for y in range(size):
        t = y / (size - 1)
        strip.putpixel((0, y), tuple(int(a + (b - a) * t) for a, b in zip(top, bottom)))
    return strip.resize((size, size))


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
    """Dark graphite squircle with sheen and a hairline rim."""
    tile = vertical_gradient(CANVAS, TILE_TOP, TILE_BOTTOM)

    mask = Image.new("L", (CANVAS, CANVAS), 0)
    ImageDraw.Draw(mask).polygon(squircle_path(512, 512, 448), fill=255)
    # Soften the superellipse corners toward the Apple continuous look.
    mask = mask.filter(ImageFilter.GaussianBlur(2 * SS))

    img = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
    img.paste(tile, (0, 0), mask)

    # Light falling on the tile from above.
    sheen = Image.new("L", (CANVAS, CANVAS), 0)
    ImageDraw.Draw(sheen).ellipse([u(-300), u(-760), u(1324), u(520)], fill=34)
    sheen = sheen.filter(ImageFilter.GaussianBlur(90 * SS))
    sheen = Image.composite(sheen, Image.new("L", (CANVAS, CANVAS), 0), mask)
    white = Image.new("RGBA", (CANVAS, CANVAS), (255, 255, 255, 255))
    white.putalpha(sheen)
    img.alpha_composite(white)

    # Hairline rim: the edge light on a dark-mode tile.
    from PIL import ImageChops

    outer = Image.new("L", (CANVAS, CANVAS), 0)
    ImageDraw.Draw(outer).polygon(squircle_path(512, 512, 448), fill=255)
    inner = Image.new("L", (CANVAS, CANVAS), 0)
    ImageDraw.Draw(inner).polygon(squircle_path(512, 512, 448 - 3), fill=255)
    rim = ImageChops.subtract(outer, inner).point(lambda v: int(v * 0.16))
    lit = Image.new("RGBA", (CANVAS, CANVAS), (255, 255, 255, 255))
    lit.putalpha(rim)
    img.alpha_composite(lit)
    return img


def draw_mark():
    """The three-sheet stack on a transparent layer, tray-icon technique.

    Each sheet's outline is a filled ring — the rounded rect expanded by
    half the stroke, with the rect shrunk by half the stroke punched out.
    Stroked polylines show notches at the corners; filled rings stay clean.
    The area where the next sheet sits is punched out too, so back sheets
    never touch the front ones — the same air the tray icon has.
    """
    layer = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
    half = STROKE * MARK_SCALE / 2
    pad = (STROKE / 2 + GAP) * MARK_SCALE

    for i, (box, colour) in enumerate(LAYERS):
        mask = Image.new("L", (CANVAS, CANVAS), 0)
        md = ImageDraw.Draw(mask)
        box_px = mark_box(box)
        outer = to_px(rounded_points(expand(box_px, half), RADIUS * MARK_SCALE + half))
        inner = to_px(rounded_points(expand(box_px, -half), max(0.0, RADIUS * MARK_SCALE - half)))
        md.polygon(outer, fill=255)
        md.polygon(inner, fill=0)
        if i + 1 < len(LAYERS):
            nxt = mark_box(LAYERS[i + 1][0])
            gap = to_px(rounded_points(expand(nxt, pad), RADIUS * MARK_SCALE + pad))
            md.polygon(gap, fill=0)
        colour_img = Image.new("RGBA", (CANVAS, CANVAS), colour)
        layer.alpha_composite(Image.composite(colour_img, layer, mask))
    return layer


def main():
    img = draw_tile()

    # Soft shadow so the sheets float above the tile.
    shadow = Image.new("L", (CANVAS, CANVAS), 0)
    sd = ImageDraw.Draw(shadow)
    for box, _ in LAYERS:
        sd.polygon(to_px(rounded_points(mark_box(box), RADIUS * MARK_SCALE)), fill=90)
    shadow = shadow.filter(ImageFilter.GaussianBlur(18 * SS))
    shadow = shadow.point(lambda v: int(v * 0.5))
    black = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 255))
    black.putalpha(shadow)
    img.alpha_composite(black)

    img.alpha_composite(draw_mark())

    img = img.resize((SIZE, SIZE), Image.LANCZOS)
    out = Path(__file__).resolve().parent.parent / "docs" / "app-icon.png"
    img.save(out)
    print(f"wrote {out} ({SIZE}x{SIZE})")


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Regenerate the ClipStack application icon (1024px master).

Design: the same stacked-sheets mark as the menu bar icon, promoted to a
Big Sur-style squircle. Three sheets of paper fan up and to the right on a
deep-indigo gradient; the front sheet carries the clipboard clip, and an
amber "copy" badge (two overlapping squares) sits on its corner.

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


def u(x):
    """Design units (0..1024 grid) to supersampled pixels."""
    return x * SS


def squircle_path(cx, cy, half, radius, steps=64):
    """Approximate a continuous-curve squircle as a polygon.

    A true squircle is a superellipse; |x|^n + |y|^n = 1 with n≈5 is close
    enough at icon scale and trivially polygonisable.
    """
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


def scale_pts(pts):
    return [(u(x), u(y)) for x, y in pts]


def vertical_gradient(size, top, bottom):
    """Linear top-to-bottom gradient RGBA image."""
    strip = Image.new("RGBA", (1, size))
    for y in range(size):
        t = y / (size - 1)
        strip.putpixel((0, y), tuple(int(a + (b - a) * t) for a, b in zip(top, bottom)))
    return strip.resize((size, size))


def main():
    # --- squircle tile with gradient -------------------------------------
    tile = vertical_gradient(CANVAS, (63, 81, 181, 255), (30, 27, 75, 255))

    mask = Image.new("L", (CANVAS, CANVAS), 0)
    ImageDraw.Draw(mask).polygon(squircle_path(512, 512, 448, 0), fill=255)
    # Soften the superellipse corners toward the Apple continuous look.
    mask = mask.filter(ImageFilter.GaussianBlur(2 * SS)).resize((CANVAS, CANVAS))

    img = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
    img.paste(tile, (0, 0), mask)

    # Subtle top sheen, like light falling on the tile.
    sheen = Image.new("L", (CANVAS, CANVAS), 0)
    ImageDraw.Draw(sheen).ellipse(
        [u(-300), u(-720), u(1324), u(560)], fill=46
    )
    sheen = sheen.filter(ImageFilter.GaussianBlur(90 * SS))
    white = Image.new("RGBA", (CANVAS, CANVAS), (255, 255, 255, 255))
    white.putalpha(Image.eval(sheen, lambda v: v))
    # Keep the sheen inside the tile.
    white.putalpha(Image.composite(white.getchannel("A"), Image.new("L", (CANVAS, CANVAS), 0), mask))
    img.alpha_composite(white)

    d = ImageDraw.Draw(img)

    # --- drop shadow under the sheets ------------------------------------
    shadow = Image.new("L", (CANVAS, CANVAS), 0)
    sd = ImageDraw.Draw(shadow)
    sd.polygon(scale_pts(rounded_points((286, 356, 690, 700), 46)), fill=110)
    shadow = shadow.filter(ImageFilter.GaussianBlur(26 * SS))
    shadow = shadow.point(lambda v: int(v * 0.55))
    black = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 255))
    black.putalpha(shadow)
    img.alpha_composite(black)
    d = ImageDraw.Draw(img)

    # --- the three sheets ------------------------------------------------
    # Back to front, fanning down-left to up-right.
    sheets = [
        (366, 226, 660, 560),  # back
        (316, 276, 610, 610),  # middle
        (266, 326, 560, 660),  # front
    ]
    fills = [(196, 205, 235, 255), (222, 228, 246, 255), (252, 252, 255, 255)]
    for (box, fill) in zip(sheets, fills):
        d.polygon(scale_pts(rounded_points(box, 44)), fill=fill)

    # Front sheet outline to separate it from the middle one.
    d.line(
        scale_pts(rounded_points(sheets[2], 44)) + [scale_pts(rounded_points(sheets[2], 44))[0]],
        fill=(120, 130, 170, 140),
        width=max(1, int(2 * SS)),
        joint="curve",
    )

    # --- clipboard clip on the front sheet ------------------------------
    clip_box = (366, 296, 460, 350)
    d.rounded_rectangle([u(c) for c in clip_box], radius=u(20), fill=(78, 90, 160, 255))
    d.rounded_rectangle(
        [u(390), u(282), u(436), u(316)], radius=u(14), fill=(150, 162, 220, 255)
    )

    # Text lines on the front sheet.
    for i, (y0, y1, x1) in enumerate([(400, 424, 520), (452, 476, 500), (504, 528, 520)]):
        d.rounded_rectangle(
            [u(300), u(y0), u(x1), u(y1)], radius=u(12), fill=(150, 158, 195, 255)
        )

    # --- amber copy badge ------------------------------------------------
    badge_cx, badge_cy, badge_r = 610, 640, 118
    d.ellipse([u(badge_cx - badge_r - 8), u(badge_cy - badge_r - 8),
               u(badge_cx + badge_r + 8), u(badge_cy + badge_r + 8)],
              fill=(24, 22, 60, 210))  # rim
    d.ellipse([u(badge_cx - badge_r), u(badge_cy - badge_r),
               u(badge_cx + badge_r), u(badge_cy + badge_r)],
              fill=(250, 190, 60, 255))

    # Two overlapping rounded squares = copy.
    s = 58
    o = 16
    bx, by = badge_cx - s / 2 - o / 2, badge_cy - s / 2 - o / 2
    back = (bx + o, by - o + 6, bx + o + s, by - o + 6 + s)
    front = (bx - o + 4, by + o - 2, bx - o + 4 + s, by + o - 2 + s)
    d.polygon(scale_pts(rounded_points(back, 14)), outline=(70, 48, 6, 255),
              fill=(250, 190, 60, 255), width=int(9 * SS))
    d.polygon(scale_pts(rounded_points(front, 14)), fill=(250, 190, 60, 255))
    d.polygon(scale_pts(rounded_points(front, 14)), outline=(70, 48, 6, 255),
              width=int(9 * SS))

    # --- downsample ------------------------------------------------------
    out = img.resize((SIZE, SIZE), Image.LANCZOS)
    target = Path(__file__).resolve().parent.parent / "docs" / "app-icon.png"
    target.parent.mkdir(parents=True, exist_ok=True)
    out.save(target)
    print(f"wrote {target} ({SIZE}x{SIZE})")


if __name__ == "__main__":
    main()

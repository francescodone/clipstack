#!/usr/bin/env python3
"""Regenerate the ClipStack menu bar (template) icon.

The mark is three offset sheets of paper. The two edges peeking out behind the
front sheet read as a stack, and the front sheet overlapping the one directly
behind it is the universal copy / duplicate glyph.

Rendered as alpha-only black on transparent so macOS can tint it for light and
dark menu bars (NSImage isTemplate).

    python3 scripts/generate_tray_icon.py

Outputs src-tauri/icons/tray.png (36px) and tray@2x.png (72px). muda pins the
status item image to 18x18 *points*, so the rasters are 2x and 4x: on Retina the
36px PNG lands 1:1 on the backing store instead of being upscaled from 18px.
"""

import math
from pathlib import Path

from PIL import Image, ImageDraw

# All coordinates are "units" on a nominal 36-unit canvas: 36 units = 18pt, so
# 1 unit = 0.5pt and the @2x raster is exactly 2 px per unit.
UNIT = 36.0
STROKE = 2.0  # 1pt line
RADIUS = 2.0  # sheet corner radius
OFFSET = 4.0  # 2pt stagger between stacked sheets
GAP = 0.6  # clear air between a sheet and the edge behind it

# Back to front. The front sheet sits top-left so the sheets behind it peek out
# along the bottom and right edges. Every coordinate is an even number of units,
# i.e. a whole pixel at 1x, so the 18pt raster stays sharp on non-Retina too.
SHEET_W, SHEET_H = 16.0, 20.0
FRONT = (6.0, 4.0, 6.0 + SHEET_W, 4.0 + SHEET_H)
MID = tuple(c + OFFSET for c in FRONT)
BACK = tuple(c + 2 * OFFSET for c in FRONT)


def rounded_rect_points(box, radius, steps=48):
    """Polygon approximating a rounded rectangle outline, in unit space."""
    x0, y0, x1, y1 = box
    r = min(radius, (x1 - x0) / 2, (y1 - y0) / 2)
    corners = [
        (x0 + r, y0 + r, 180, 270),
        (x1 - r, y0 + r, 270, 360),
        (x1 - r, y1 - r, 0, 90),
        (x0 + r, y1 - r, 90, 180),
    ]
    points = []
    for cx, cy, a0, a1 in corners:
        for i in range(steps + 1):
            angle = math.radians(a0 + (a1 - a0) * i / steps)
            points.append((cx + r * math.cos(angle), cy + r * math.sin(angle)))
    return points


def expand(box, pad):
    x0, y0, x1, y1 = box
    return (x0 - pad, y0 - pad, x1 + pad, y1 + pad)


def scale_box(box, s):
    return [c * s for c in box]


def scale_points(points, s):
    return [(x * s, y * s) for x, y in points]


def draw_stack(alpha, s):
    """Paint the icon onto an 8-bit alpha canvas; `s` converts units to px."""
    draw = ImageDraw.Draw(alpha)
    width = max(1, round(STROKE * s))
    pad = STROKE / 2 + GAP

    def sheet(box):
        draw.rounded_rectangle(
            scale_box(box, s), radius=RADIUS * s, outline=255, width=width
        )

    def knock_out(box):
        points = rounded_rect_points(expand(box, pad), RADIUS + pad)
        draw.polygon(scale_points(points, s), fill=0)

    sheet(BACK)
    knock_out(MID)
    sheet(MID)
    knock_out(FRONT)
    sheet(FRONT)


def render(pixels, supersample=8):
    ss = pixels * supersample
    alpha = Image.new("L", (ss, ss), 0)
    draw_stack(alpha, ss / UNIT)
    alpha = alpha.resize((pixels, pixels), Image.LANCZOS)

    out = Image.new("RGBA", (pixels, pixels), (0, 0, 0, 0))
    out.putalpha(alpha)
    return out


def main():
    icons = Path(__file__).resolve().parent.parent / "src-tauri" / "icons"
    for name, pixels in (("tray.png", 36), ("tray@2x.png", 72)):
        target = icons / name
        render(pixels).save(target)
        print(f"wrote {target} ({pixels}x{pixels})")


if __name__ == "__main__":
    main()

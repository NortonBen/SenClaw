"""Regenerate the Windows tray ICOs (assets/tray_icon.ico + tray_icon_white.ico).

The tray glyph is three claw slashes — each a closed pair of cubic beziers,
identical to assets/tray_icon.svg (viewBox 192 192 640 640). Windows renders
tray icons as raw pixels at 16-24 px where the slashes are ~1.5 px thin, so
small sizes get a dilation pass (BOLD) to thicken strokes before downsampling.
macOS/Linux keep using tray_icon.png and are not touched by this script.

Usage:  python3 gen_tray_ico.py   (needs Pillow)
"""
import io
import struct
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

ASSETS = Path(__file__).resolve().parent.parent / "assets"

VB_X, VB_Y, VB_S = 192.0, 192.0, 640.0  # viewBox origin + size

# (start, c1, c2, end) per bezier; the second one returns toward the top.
def slash(dx):
    return [
        ((341 + dx, 232), (293 + dx, 430), (305 + dx, 612), (403 + dx, 792)),
        ((403 + dx, 792), (359 + dx, 612), (363 + dx, 430), (403 + dx, 250)),
    ]

SLASHES = [slash(0), slash(140), slash(280)]

# extra half-stroke in FINAL px per icon size — tapers off as the icon gets room
BOLD = {16: 0.55, 20: 0.50, 24: 0.45, 32: 0.30, 48: 0.0, 64: 0.0}
SS = 16  # supersample factor

def cubic(p0, p1, p2, p3, t):
    mt = 1 - t
    x = mt**3 * p0[0] + 3 * mt**2 * t * p1[0] + 3 * mt * t**2 * p2[0] + t**3 * p3[0]
    y = mt**3 * p0[1] + 3 * mt**2 * t * p1[1] + 3 * mt * t**2 * p2[1] + t**3 * p3[1]
    return x, y

def polygon_for(path, canvas):
    pts = []
    for (p0, p1, p2, p3) in path:
        for i in range(241):
            x, y = cubic(p0, p1, p2, p3, i / 240.0)
            pts.append(((x - VB_X) / VB_S * canvas, (y - VB_Y) / VB_S * canvas))
    return pts

def render(size, color):
    canvas = size * SS
    mask = Image.new("L", (canvas, canvas), 0)
    d = ImageDraw.Draw(mask)
    for path in SLASHES:
        d.polygon(polygon_for(path, canvas), fill=255)
    if BOLD[size] > 0:
        r = max(1, round(BOLD[size] * SS))
        mask = mask.filter(ImageFilter.MaxFilter(2 * r + 1))
    out = Image.new("RGBA", (size, size), color + (0,))
    out.putalpha(mask.resize((size, size), Image.LANCZOS))
    return out

def write_ico(path, color):
    """ICO with PNG-compressed entries (supported since Vista)."""
    blobs = []
    for s in sorted(BOLD):
        buf = io.BytesIO()
        render(s, color).save(buf, format="PNG")
        blobs.append((s, buf.getvalue()))
    header = struct.pack("<HHH", 0, 1, len(blobs))
    entries = b""
    offset = 6 + 16 * len(blobs)
    for s, blob in blobs:
        entries += struct.pack("<BBBBHHII", s, s, 0, 0, 1, 32, len(blob), offset)
        offset += len(blob)
    path.write_bytes(header + entries + b"".join(b for _, b in blobs))
    print(f"wrote {path} ({offset} bytes, sizes {sorted(BOLD)})")

if __name__ == "__main__":
    write_ico(ASSETS / "tray_icon_white.ico", (255, 255, 255))
    write_ico(ASSETS / "tray_icon.ico", (0, 0, 0))

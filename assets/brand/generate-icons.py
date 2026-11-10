#!/usr/bin/env python3
"""Derives the platform app icons from assets/brand/icon.svg.

The mark's path is copied byte for byte (spec section 7): only its colour is
set and it is placed with a transform on a white rounded tile with a faint edge,
so taskbars and docks of any colour show it. Needs rsvg-convert and ImageMagick (magick).

    python3 assets/brand/generate-icons.py

Writes, next to this script: icon-tile.svg (Linux scalable icon),
hicolor/<size>.png (Linux), icon.ico (Windows) and icon.icns (macOS).
"""
import re
import struct
import subprocess
import tempfile
from pathlib import Path

BRAND = Path(__file__).resolve().parent
TILE = "#ffffff"  # the app's white
MARK = "#161616"  # Oxocarbon dark background
# A faint edge (Oxocarbon #dde1e6, 4 of 512 units) so the white tile still
# reads on a white or light-grey taskbar; it fades out at the smallest sizes.
EDGE = "#dde1e6"

source = (BRAND / "icon.svg").read_text()
path = re.search(r"<path [^>]*/>", source).group(0)


def tile(margin, radius, scale):
    """A 512x512 rounded tile with the unchanged mark scaled about its centre."""
    side = 512 - 2 * margin - 4  # the edge is drawn inside the tile
    mark = path.replace("currentColor", MARK)
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" width="512" height="512" viewBox="0 0 512 512">\n'
        f'  <rect x="{margin + 2}" y="{margin + 2}" width="{side}" height="{side}" rx="{radius - 2}" fill="{TILE}" stroke="{EDGE}" stroke-width="4"/>\n'
        f'  <g transform="translate(256 256) scale({scale}) translate(-256 -256)">\n    {mark}\n  </g>\n'
        "</svg>\n"
    )


def render(svg, size, out):
    subprocess.run(["rsvg-convert", "-w", str(size), "-h", str(size), "-o", str(out), str(svg)], check=True)


# Windows and Linux: nearly full-bleed tile so small sizes stay legible.
standard = BRAND / "icon-tile.svg"
standard.write_text(tile(margin=16, radius=96, scale=0.72))
assert re.search(r' d="[^"]*"', standard.read_text()).group(0) == re.search(r' d="[^"]*"', source).group(0)

with tempfile.TemporaryDirectory() as tmp:
    tmp = Path(tmp)

    (BRAND / "hicolor").mkdir(exist_ok=True)
    for size in (16, 24, 32, 48, 64, 128, 256, 512):
        render(standard, size, BRAND / "hicolor" / f"{size}.png")

    # Up to 128 px as 32-bit BMP entries (ImageMagick), 256 px as PNG, the
    # usual layout of Windows icons; the ICO directory is rebuilt here.
    ico_sizes = (16, 20, 24, 32, 40, 48, 64, 128)
    for size in ico_sizes:
        render(standard, size, tmp / f"ico-{size}.png")
    subprocess.run(
        ["magick", *(str(tmp / f"ico-{s}.png") for s in ico_sizes), "-strip", str(tmp / "small.ico")],
        check=True,
    )
    small = (tmp / "small.ico").read_bytes()
    images = []
    for i in range(struct.unpack("<H", small[4:6])[0]):
        entry = small[6 + 16 * i : 22 + 16 * i]
        length, offset = struct.unpack("<II", entry[8:16])
        images.append((entry[:8], small[offset : offset + length]))
    images.append((struct.pack("<BBBBHH", 0, 0, 0, 0, 1, 32), (BRAND / "hicolor" / "256.png").read_bytes()))
    ico = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    for head, data in images:
        ico += head + struct.pack("<II", len(data), offset)
        offset += len(data)
    (BRAND / "icon.ico").write_bytes(ico + b"".join(data for _, data in images))

    # macOS: Apple's grid leaves ~10% around the tile (824 of 1024 px).
    macos = tmp / "icon-macos.svg"
    macos.write_text(tile(margin=50, radius=92, scale=0.62))
    chunks = b""
    for kind, size in (
        (b"icp4", 16), (b"icp5", 32), (b"icp6", 64), (b"ic07", 128), (b"ic08", 256),
        (b"ic09", 512), (b"ic10", 1024), (b"ic11", 32), (b"ic12", 64), (b"ic13", 256), (b"ic14", 512),
    ):
        png = tmp / f"icns-{size}.png"
        if not png.exists():
            render(macos, size, png)
        data = png.read_bytes()
        chunks += kind + struct.pack(">I", 8 + len(data)) + data
    (BRAND / "icon.icns").write_bytes(b"icns" + struct.pack(">I", 8 + len(chunks)) + chunks)

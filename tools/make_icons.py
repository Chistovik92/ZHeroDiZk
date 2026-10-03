#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only
"""Generate the ZHeroDiZk application icon (ICO for Windows, SVG for the in-app logo).

Pure standard library and deterministic: the same call always writes the same bytes, so the
committed files in client/branding/ can be checked against a fresh render.
"""
import struct
import sys
import zlib
from pathlib import Path

BACKGROUND = (0x0F, 0x17, 0x2A, 0xFF)  # deep navy
LETTER = (0xFA, 0xCC, 0x15, 0xFF)      # gold
# The letter Z as a polygon in unit coordinates (even-odd fill).
Z_POLYGON = [(0.25, 0.24), (0.75, 0.24), (0.75, 0.33), (0.40, 0.67), (0.75, 0.67),
             (0.75, 0.76), (0.25, 0.76), (0.25, 0.67), (0.60, 0.33), (0.25, 0.33)]
CORNER = 0.22          # corner radius of the square, as a share of its side
SAMPLES = 4            # supersampling per axis
ICON_SIZES = (16, 32, 48, 64, 128, 256)


def _inside_polygon(x, y, polygon):
    inside = False
    count = len(polygon)
    for index in range(count):
        x1, y1 = polygon[index]
        x2, y2 = polygon[(index + 1) % count]
        if (y1 > y) != (y2 > y) and x < (x2 - x1) * (y - y1) / (y2 - y1) + x1:
            inside = not inside
    return inside


def _inside_rounded_square(x, y):
    """Point (unit coordinates) inside the square with rounded corners."""
    radius = CORNER
    dx = max(radius - x, 0.0, x - (1.0 - radius))
    dy = max(radius - y, 0.0, y - (1.0 - radius))
    return dx * dx + dy * dy <= radius * radius


def render(size):
    """RGBA pixels, top row first, `size` x `size`."""
    rows = []
    step = 1.0 / (size * SAMPLES)
    for row in range(size):
        line = bytearray()
        for column in range(size):
            red = green = blue = alpha = 0
            for sy in range(SAMPLES):
                for sx in range(SAMPLES):
                    x = column / size + (sx + 0.5) * step
                    y = row / size + (sy + 0.5) * step
                    if not _inside_rounded_square(x, y):
                        continue
                    colour = LETTER if _inside_polygon(x, y, Z_POLYGON) else BACKGROUND
                    red += colour[0]
                    green += colour[1]
                    blue += colour[2]
                    alpha += 255
            covered = SAMPLES * SAMPLES
            hits = alpha // 255
            if hits:
                line += bytes((red // hits, green // hits, blue // hits, alpha // covered))
            else:
                line += bytes((0, 0, 0, 0))
        rows.append(bytes(line))
    return rows


def png_bytes(size):
    rows = render(size)
    raw = b"".join(b"\x00" + row for row in rows)

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    header = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header)
            + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b""))


def dib_bytes(size):
    """Icon image as a 32-bit DIB with an empty AND mask (bottom-up BGRA)."""
    rows = render(size)
    header = struct.pack("<IiiHHIIiiII", 40, size, size * 2, 1, 32, 0, 0, 0, 0, 0, 0)
    pixels = bytearray()
    for row in reversed(rows):
        for index in range(0, len(row), 4):
            r, g, b, a = row[index:index + 4]
            pixels += bytes((b, g, r, a))
    mask_row = b"\x00" * (((size + 31) // 32) * 4)
    return header + bytes(pixels) + mask_row * size


def ico_bytes():
    """ICO with PNG data for 256 px and classic DIB data for the smaller sizes."""
    images = []
    for size in ICON_SIZES:
        images.append((size, png_bytes(size) if size == 256 else dib_bytes(size)))
    directory = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    entries = b""
    data = b""
    for size, blob in images:
        entries += struct.pack("<BBBBHHII", size % 256, size % 256, 0, 0, 1, 32, len(blob), offset)
        data += blob
        offset += len(blob)
    return directory + entries + data


def svg_text():
    points = " ".join(f"{x * 256:g},{y * 256:g}" for x, y in Z_POLYGON)
    colour = lambda c: "#%02x%02x%02x" % c[:3]  # noqa: E731
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256">'
        f'<rect width="256" height="256" rx="{CORNER * 256:g}" fill="{colour(BACKGROUND)}"/>'
        f'<polygon points="{points}" fill="{colour(LETTER)}"/></svg>\n'
    )


def write_all(directory):
    directory = Path(directory)
    directory.mkdir(parents=True, exist_ok=True)
    (directory / "app_icon.ico").write_bytes(ico_bytes())
    (directory / "icon.svg").write_text(svg_text(), encoding="utf-8", newline="\n")


if __name__ == "__main__":
    target = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).resolve().parents[1] / "client" / "branding"
    write_all(target)
    print(f"written to {target}")

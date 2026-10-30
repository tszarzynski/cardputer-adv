#!/usr/bin/env python3
"""Turns the screenshots in a Cardputer's log into PNGs, for a GIF.

The app logs `Frame::screenshot()` (cardputer-adv's), one line each; this
finds those lines in a saved log (espflash's monitor output, say) and writes
them out as frames at a steady rate, each shown for as long as it was on
the screen, by the times they were taken. Long pauses are cut short. Then
`gifski --fps <fps> -o out.gif <dir>/*.png`, or `just gif <log>`.

    tools/screenshots.py LOG DIR [--scale 3] [--fps 20] [--max-hold 1.5]

Python's standard library only.
"""

import argparse
import base64
import re
import struct
import sys
import zlib
from pathlib import Path

MARKER = "cardputer-screenshot"
LINE = re.compile(MARKER + r" (\d+)x(\d+) (\d+) ([A-Za-z0-9+/=]+)")
ANSI = re.compile(r"\x1b\[[0-9;]*m")


def screenshots(log):
    """Each screenshot in the log: (ms, width, height, RGB565 pixels)."""
    for number, line in enumerate(log.read_text(errors="replace").splitlines(), 1):
        match = LINE.search(ANSI.sub("", line))
        if not match:
            continue
        width, height, millis, text = match.groups()
        width, height = int(width), int(height)
        try:
            data = base64.b64decode(text, validate=True)
        except ValueError:
            print(f"line {number}: not base64, skipped", file=sys.stderr)
            continue
        pixels = []
        for length, color in struct.iter_unpack(">HH", data[: len(data) // 4 * 4]):
            pixels.extend([color] * length)
        if len(pixels) != width * height:
            # Cut short, or another line printed into it.
            print(f"line {number}: {len(pixels)} pixels, skipped", file=sys.stderr)
            continue
        yield int(millis), width, height, pixels


def rgb(color):
    """An RGB565 colour as 8-bit red, green and blue."""
    r, g, b = color >> 11, color >> 5 & 63, color & 31
    return bytes((r * 255 // 31, g * 255 // 63, b * 255 // 31))


def png(width, height, pixels, scale):
    """The pixels as a PNG, each `scale` × `scale`, kept sharp."""
    colors = {}
    rows = []
    for y in range(height):
        row = b"".join(
            colors.setdefault(c, rgb(c)) * scale for c in pixels[y * width : (y + 1) * width]
        )
        rows.extend([b"\0" + row] * scale)

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    header = struct.pack(">IIBBBBB", width * scale, height * scale, 8, 2, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(b"".join(rows), 9))
        + chunk(b"IEND", b"")
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("log", type=Path)
    parser.add_argument("dir", type=Path)
    parser.add_argument("--scale", type=int, default=3, help="pixels a screen pixel (3)")
    parser.add_argument("--fps", type=int, default=20, help="frames a second (20)")
    parser.add_argument(
        "--max-hold", type=float, default=1.5, help="longest a screen shows, in s (1.5)"
    )
    args = parser.parse_args()

    shots = list(screenshots(args.log))
    if not shots:
        sys.exit(f"no screenshots in {args.log}")
    args.dir.mkdir(parents=True, exist_ok=True)
    for old in args.dir.glob("frame-*.png"):
        old.unlink()

    step = 1000 / args.fps
    frames = 0
    for i, (millis, width, height, pixels) in enumerate(shots):
        until = shots[i + 1][0] if i + 1 < len(shots) else millis + args.max_hold * 1000
        held = min(until - millis, args.max_hold * 1000)
        image = png(width, height, pixels, args.scale)
        for _ in range(max(1, round(held / step))):
            (args.dir / f"frame-{frames:05}.png").write_bytes(image)
            frames += 1
    print(f"{len(shots)} screenshots, {frames} frames at {args.fps} fps in {args.dir}")


if __name__ == "__main__":
    main()

# Worldkiln
# Copyright (c) 2026 Zachary Joubert / AEOWUN
# SPDX-License-Identifier: Apache-2.0
"""Worldkiln procedural image/texture generator. JSON stdin -> PNG stdout.

Standard library only; no network, files, subprocesses, eval or model-written code.
The editor embeds this vetted script and publishes its output after approval.
"""
import json
import math
import random
import struct
import sys
import zlib


def generate(spec):
    width, height = spec.get("width", 256), spec.get("height", 256)
    if type(width) is not int or type(height) is not int or not 1 <= width <= 1024 or not 1 <= height <= 1024:
        raise ValueError("Image dimensions must be integers from 1 to 1024")
    palette = spec.get("palette", [[80, 100, 120, 255], [180, 190, 200, 255]])
    if not isinstance(palette, list) or not 1 <= len(palette) <= 8:
        raise ValueError("Use one to eight RGBA colors")
    for color in palette:
        if not isinstance(color, list) or len(color) != 4 or any(type(c) is not int or not 0 <= c <= 255 for c in color):
            raise ValueError("Each color must contain four bytes: R, G, B, A")
    pattern = spec.get("pattern", "noise")
    if pattern not in ("solid", "checker", "noise", "brick", "gradient"):
        raise ValueError("Pattern must be solid, checker, noise, brick or gradient")
    seed, scale = spec.get("seed", 0), spec.get("scale", 16)
    if type(seed) is not int or type(scale) is not int or not 1 <= scale <= 256:
        raise ValueError("Seed must be an integer; scale must be 1–256")
    shapes = spec.get("shapes", [])
    if not isinstance(shapes, list) or len(shapes) > 64:
        raise ValueError("Use at most 64 shapes")
    for shape in shapes:
        if not isinstance(shape, dict) or shape.get("kind") not in ("rect", "circle"):
            raise ValueError("Shape kind must be rect or circle")
        bounds = shape.get("bounds")
        if not isinstance(bounds, list) or len(bounds) != 4 or any(type(n) not in (int, float) or not math.isfinite(n) or abs(n) > 4096 for n in bounds):
            raise ValueError("Shape bounds must be finite [x, y, width, height]")
        color = shape.get("color")
        if not isinstance(color, list) or len(color) != 4 or any(type(c) is not int or not 0 <= c <= 255 for c in color):
            raise ValueError("Shape color must be RGBA bytes")
    rng = random.Random(seed)
    pixels = bytearray(width * height * 4)
    for y in range(height):
        for x in range(width):
            if pattern == "solid":
                color = palette[0]
            elif pattern == "checker":
                color = palette[((x // scale) + (y // scale)) % len(palette)]
            elif pattern == "brick":
                row = y // scale
                mortar = y % scale < 2 or (x + (row % 2) * scale) % (scale * 2) < 2
                color = palette[-1] if mortar else palette[0]
            elif pattern == "gradient":
                t = x / max(1, width - 1)
                color = [round(a * (1 - t) + b * t) for a, b in zip(palette[0], palette[-1])]
            else:
                t = rng.random()
                color = [round(a * (1 - t) + b * t) for a, b in zip(palette[0], palette[-1])]
            for shape in shapes:
                sx, sy, sw, sh = shape["bounds"]
                inside = sx <= x < sx + sw and sy <= y < sy + sh
                if shape["kind"] == "circle":
                    inside = sw > 0 and sh > 0 and ((x - sx - sw / 2) / (sw / 2)) ** 2 + ((y - sy - sh / 2) / (sh / 2)) ** 2 <= 1
                if inside:
                    color = shape["color"]
            offset = (y * width + x) * 4
            pixels[offset:offset + 4] = bytes(color)
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xffffffff)
    rows = b"".join(b"\0" + pixels[y * width * 4:(y + 1) * width * 4] for y in range(height))
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b""))


if __name__ == "__main__":
    try:
        payload = sys.stdin.buffer.read(1_048_577)
        if len(payload) > 1_048_576:
            raise ValueError("Generation request exceeds 1 MiB")
        sys.stdout.buffer.write(generate(json.loads(payload)))
    except (ValueError, TypeError, KeyError, OverflowError) as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)

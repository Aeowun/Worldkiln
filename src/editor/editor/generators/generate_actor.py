# Worldkiln
# Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
# Licensed under the AeoEngine Source-Available License.
# See LICENSE in the repository root for the complete terms.
"""Worldkiln static Actor source generator. JSON stdin -> embedded glTF 2 GLB.

Standard library only. Structured triangle meshes; no arbitrary Python execution.
No skins/animations, external buffers, extensions, morphs or embedded behaviors.
"""
import json
import math
import struct
import sys


def generate(spec):
    positions, triangles = spec.get("vertices"), spec.get("triangles")
    if not isinstance(positions, list) or not 3 <= len(positions) <= 30000:
        raise ValueError("Use 3–30000 vertices")
    if not isinstance(triangles, list) or not 1 <= len(triangles) <= 30000:
        raise ValueError("Use 1–30000 indexed triangles")
    for vertex in positions:
        if not isinstance(vertex, list) or len(vertex) != 3 or any(type(n) not in (int, float) or not math.isfinite(n) or abs(n) > 100000 for n in vertex):
            raise ValueError("Vertices must be finite XYZ coordinates within ±100000")
    color = spec.get("color", [0.6, 0.6, 0.6, 1.0])
    if not isinstance(color, list) or len(color) != 4 or any(type(n) not in (int, float) or not math.isfinite(n) or not 0 <= n <= 1 for n in color):
        raise ValueError("Color must contain four finite components from 0 to 1")
    points, normals = [], []
    for triangle in triangles:
        if not isinstance(triangle, list) or len(triangle) != 3 or any(type(i) is not int or not 0 <= i < len(positions) for i in triangle):
            raise ValueError("Each triangle must contain three valid vertex indices")
        a, b, c = [positions[i] for i in triangle]
        u, v = [b[i] - a[i] for i in range(3)], [c[i] - a[i] for i in range(3)]
        normal = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]
        length = math.sqrt(sum(n * n for n in normal))
        if length < 1e-12:
            raise ValueError("Degenerate triangles are unsupported")
        points.extend((a, b, c))
        normals.extend([[n / length for n in normal]] * 3)
    count = len(points)
    packed = b"".join(struct.pack("<fff", *point) for point in points)
    packed_normals = b"".join(struct.pack("<fff", *normal) for normal in normals)
    indices = struct.pack("<" + "I" * count, *range(count))
    blob = packed + packed_normals + indices
    name = spec.get("name", "GeneratedActor")
    if not isinstance(name, str) or not 1 <= len(name) <= 64 or any(not (c.isascii() and (c.isalnum() or c in "_-")) for c in name):
        raise ValueError("Name must contain 1–64 ASCII letters, digits, underscores or hyphens")
    document = {
        "asset": {"version": "2.0", "generator": "Worldkiln constrained Python generator"},
        "scene": 0, "scenes": [{"nodes": [0]}], "nodes": [{"mesh": 0, "name": name}],
        "meshes": [{"name": name, "primitives": [{"attributes": {"POSITION": 0, "NORMAL": 1}, "indices": 2, "material": 0, "mode": 4}]}],
        "materials": [{"name": "Surface", "pbrMetallicRoughness": {"baseColorFactor": color, "metallicFactor": 0, "roughnessFactor": 1}}],
        "buffers": [{"byteLength": len(blob)}],
        "bufferViews": [{"buffer": 0, "byteOffset": 0, "byteLength": len(packed), "target": 34962},
                        {"buffer": 0, "byteOffset": len(packed), "byteLength": len(packed_normals), "target": 34962},
                        {"buffer": 0, "byteOffset": len(packed) + len(packed_normals), "byteLength": len(indices), "target": 34963}],
        "accessors": [{"bufferView": 0, "componentType": 5126, "count": count, "type": "VEC3", "min": [min(p[i] for p in points) for i in range(3)], "max": [max(p[i] for p in points) for i in range(3)]},
                      {"bufferView": 1, "componentType": 5126, "count": count, "type": "VEC3"},
                      {"bufferView": 2, "componentType": 5125, "count": count, "type": "SCALAR"}],
    }
    text = json.dumps(document, separators=(",", ":")).encode()
    text += b" " * (-len(text) % 4)
    blob += b"\0" * (-len(blob) % 4)
    return struct.pack("<III", 0x46546c67, 2, 12 + 8 + len(text) + 8 + len(blob)) + struct.pack("<I4s", len(text), b"JSON") + text + struct.pack("<I4s", len(blob), b"BIN\0") + blob


if __name__ == "__main__":
    try:
        payload = sys.stdin.buffer.read(1_048_577)
        if len(payload) > 1_048_576:
            raise ValueError("Generation request exceeds 1 MiB")
        sys.stdout.buffer.write(generate(json.loads(payload)))
    except (ValueError, TypeError, KeyError, OverflowError) as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)

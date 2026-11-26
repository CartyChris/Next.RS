#!/usr/bin/env python3
"""Build one indexed, Y-up mesh for either Model Lab renderer. Pawn and OBJ use the standard library."""

import argparse
import json
import math
import os
from pathlib import Path
import struct
import sys
import tempfile

MAX_BYTES = 8 * 1024 * 1024
MAX_RECORDS = 100_000


def empty_mesh():
    return {"version": 1, "positions": [], "normals": [], "uvs": [], "indices": []}


def add_vertex(mesh, position, normal, uv):
    index = len(mesh["positions"]) // 3
    if index >= MAX_RECORDS:
        raise ValueError("mesh exceeds 100,000 output vertices")
    mesh["positions"].extend(position)
    mesh["normals"].extend(normal)
    mesh["uvs"].extend(uv)
    return index


def unit(vector):
    length = math.hypot(*vector)
    if length == 0:
        raise ValueError("zero-length normal or degenerate triangle")
    return tuple(value / length for value in vector)


def pawn(segments):
    if not 8 <= segments <= 128:
        raise ValueError("--segments must be between 8 and 128")
    # Radius, height: foot, plinth, tapered stem, collar and rounded head.
    profile = [
        (0, 0), (.45, 0), (.50, .05), (.50, .12), (.46, .17), (.39, .20),
        (.38, .24), (.41, .27), (.37, .32), (.28, .36), (.19, .50),
        (.14, .72), (.16, .83), (.24, .86), (.25, .90), (.21, .95),
        (.14, .97), (.20, 1.03), (.24, 1.11), (.25, 1.18),
        (.23, 1.27), (.17, 1.35), (.09, 1.40), (0, 1.42),
    ]
    mesh = empty_mesh()
    bottom = add_vertex(mesh, (0, 0, 0), (0, -1, 0), (.5, 0))
    rings = []
    for row, (radius, height) in enumerate(profile[1:-1], 1):
        dr = profile[row + 1][0] - profile[row - 1][0]
        dy = profile[row + 1][1] - profile[row - 1][1]
        radial, vertical = unit((dy, -dr))
        ring = []
        for col in range(segments + 1):
            angle = math.tau * (col % segments) / segments
            x, z = math.cos(angle), math.sin(angle)
            ring.append(add_vertex(mesh, (radius*x, height, radius*z),
                                   (radial*x, vertical, radial*z), (col/segments, height/profile[-1][1])))
        rings.append(ring)
    top = add_vertex(mesh, (0, profile[-1][1], 0), (0, 1, 0), (.5, 1))
    indices = mesh["indices"]
    for col in range(segments):
        indices.extend((bottom, rings[0][col], rings[0][col + 1]))
        for lower, upper in zip(rings, rings[1:]):
            a, b = lower[col:col + 2]
            c, d = upper[col:col + 2]
            indices.extend((a, c, b, b, c, d))
        indices.extend((rings[-1][col], top, rings[-1][col + 1]))
    return mesh


def f32(token):
    # Validate the positions actually seen by the browser, including f32 rounding.
    try:
        value = struct.unpack("f", struct.pack("f", float(token)))[0]
    except OverflowError:
        raise ValueError("coordinate exceeds finite float32 range") from None
    if not math.isfinite(value):
        raise ValueError("coordinate must be a finite float32")
    return value


def index(token, count):
    value = int(token)
    result = value - 1 if value > 0 else count + value
    if not 0 <= result < count:
        raise ValueError(f"index {value} outside existing OBJ records")
    return result


def corner(token, streams):
    fields = token.split("/")
    if len(fields) > 3 or not fields[0] or not fields[-1]:
        raise ValueError(f"invalid OBJ face corner {token!r}")
    position = index(fields[0], len(streams["v"]))
    texcoord = index(fields[1], len(streams["vt"])) if len(fields) > 1 and fields[1] else None
    normal = index(fields[2], len(streams["vn"])) if len(fields) == 3 else None
    return position, texcoord, normal


def face_normal(a, b, c):
    ux, uy, uz = (end - start for start, end in zip(a, b))
    vx, vy, vz = (end - start for start, end in zip(a, c))
    return unit((uy*vz - uz*vy, uz*vx - ux*vz, ux*vy - uy*vx))


def obj(path):
    with path.open("rb") as source:
        data = source.read(MAX_BYTES + 1)
    if len(data) > MAX_BYTES:
        raise ValueError("OBJ input exceeds 8 MiB")
    mesh = empty_mesh()
    streams = {"v": [], "vt": [], "vn": []}
    cache = {}
    for line_no, line in enumerate(data.decode("utf-8-sig").splitlines(), 1):
        fields = line.partition("#")[0].split()
        if not fields:
            continue
        kind, values = fields[0], fields[1:]
        try:
            if kind in streams:
                if len(values) != (2 if kind == "vt" else 3):
                    raise ValueError(f"unsupported number of coordinates for {kind}")
                if len(streams[kind]) >= MAX_RECORDS:
                    raise ValueError(f"more than 100,000 {kind} records")
                coords = tuple(f32(value) for value in values)
                streams[kind].append(unit(coords) if kind == "vn" else coords)
            elif kind == "f":
                if len(values) != 3:
                    raise ValueError("triangulate all faces before importing")
                if len(mesh["indices"]) >= 3 * MAX_RECORDS:
                    raise ValueError("more than 100,000 triangles")
                corners = [corner(value, streams) for value in values]
                flat = face_normal(*(streams["v"][p] for p, _, _ in corners))
                for p, t, n in corners:
                    # A shared position can have independent UV and normal indices.
                    normal = flat if n is None else streams["vn"][n]
                    key = (p, t, normal)
                    if key not in cache:
                        uv = (0, 0) if t is None else streams["vt"][t]
                        cache[key] = add_vertex(mesh, streams["v"][p], normal, uv)
                    mesh["indices"].append(cache[key])
            elif kind not in {"o", "g", "s", "mtllib", "usemtl"}:
                raise ValueError(f"unsupported OBJ record {kind!r}")
        except ValueError as error:
            raise ValueError(f"line {line_no}: {error}") from error
    if not mesh["indices"]:
        raise ValueError("OBJ contains no triangle faces")
    return mesh


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    make = commands.add_parser("pawn", help="generate a lathed chess pawn")
    make.add_argument("--segments", type=int, default=32)
    convert = commands.add_parser(
        "obj", help="convert a triangulated OBJ",
        description="One Y-up mesh. Export triangles with optional UVs and normals. "
                    "Missing attributes use zero UVs and flat face normals. Object groups merge. "
                    "Materials, textures and smoothing groups are ignored. Coordinates and winding are preserved.",
    )
    convert.add_argument("input", type=Path)
    binary = commands.add_parser(
        "glb", help="extract one static triangle mesh from a self-contained GLB 2 file",
        description="Requires pygltflib==1.16.5. Export one mesh with identity transforms, "
                    "float32 positions and normals, and optional float32 UVs. Materials and images are discarded.",
    )
    binary.add_argument("input", type=Path)
    for command in (make, convert, binary):
        command.add_argument("-o", "--output", type=Path, help="write .mesh.json (default: stdout)")
    args = parser.parse_args()
    try:
        if args.command != "pawn" and args.output and args.output.resolve() == args.input.resolve():
            raise ValueError("choose an output path different from the input model")
        if args.command == "glb":
            from mesh_glb import read_glb
            mesh = read_glb(args.input, MAX_BYTES, MAX_RECORDS)
        elif args.command == "obj":
            mesh = obj(args.input)
        else:
            mesh = pawn(args.segments)
        payload = json.dumps(mesh, separators=(",", ":"), allow_nan=False) + "\n"
        if len(payload) > MAX_BYTES:  # numeric fields and keys are ASCII
            raise ValueError("mesh JSON exceeds the Model Lab 8 MiB import limit")
        if args.output is None:
            sys.stdout.write(payload)
        else:
            with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=args.output.parent, delete=False) as tmp:
                try:
                    tmp.write(payload)
                    tmp.close()
                    os.replace(tmp.name, args.output)
                finally:
                    if os.path.exists(tmp.name):
                        os.unlink(tmp.name)
    except (OSError, ValueError) as error:
        parser.exit(2, f"{parser.prog}: {error}\n")


if __name__ == "__main__":
    main()

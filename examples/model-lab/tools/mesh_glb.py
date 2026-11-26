"""Geometry-only GLB 2 reader for the optional mesh.py glb command."""

import math
import struct


def one(values, name):
    if len(values) != 1:
        raise ValueError(f"expected one {name}")
    return values[0]


def entry(values, index, name):
    if type(index) is not int or not 0 <= index < len(values):
        raise ValueError(f"invalid {name} index")
    return values[index]


def read_glb(path, max_bytes, max_records):
    try:
        from pygltflib import GLTF2
    except ImportError as error:
        raise ValueError("GLB import requires: python3 -m pip install pygltflib==1.16.5") from error

    with path.open("rb") as stream:
        data = stream.read(max_bytes + 1)
    if len(data) > max_bytes or len(data) < 28:
        raise ValueError("invalid GLB length (8 MiB limit)")
    magic, version, size, json_size, chunk = struct.unpack_from("<4sIII4s", data)
    if (magic, version, size, chunk) != (b"glTF", 2, len(data), b"JSON"):
        raise ValueError("invalid GLB header or length")
    bin_header = 20 + json_size
    if json_size > 256 * 1024 or json_size % 4 or bin_header + 8 > size:
        raise ValueError("invalid GLB JSON chunk (256 KiB metadata limit)")
    bin_size, chunk = struct.unpack_from("<I4s", data, bin_header)
    if chunk != b"BIN\x00" or bin_size % 4 or bin_header + 8 + bin_size != size:
        raise ValueError("expected exactly one embedded BIN chunk")
    try:
        # Byte-only parsing. Never ask the library to resolve a URI, a path or an image.
        doc = GLTF2.load_from_bytes(data)
        return convert(doc, bin_size, max_records)
    except (AttributeError, IndexError, KeyError, TypeError, RecursionError) as error:
        raise ValueError("invalid GLB metadata") from error


def convert(doc, bin_size, max_records):
    if doc.asset.version != "2.0" or doc.extensionsUsed or doc.extensionsRequired:
        raise ValueError("expected core glTF 2.0 without extensions")
    if doc.animations or doc.skins:
        raise ValueError("only static geometry is supported; bake animation and skinning first")
    buffer = one(doc.buffers, "embedded buffer")
    if buffer.uri is not None or any(image.uri is not None for image in doc.images):
        raise ValueError("only self-contained GLB files without buffer or image URIs are supported")
    if type(buffer.byteLength) is not int or not 0 <= bin_size - buffer.byteLength <= 3:
        raise ValueError("declared buffer length disagrees with BIN chunk")
    if one(doc.scenes, "scene").nodes != [0] or doc.scene not in (None, 0):
        raise ValueError("expected one scene containing the mesh node")
    node = one(doc.nodes, "root node")
    if node.mesh != 0 or node.skin is not None or node.children or node.extensions:
        raise ValueError("expected one static root mesh node without children or extensions")
    identity = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]
    if (node.matrix not in (None, identity) or node.translation not in (None, [0, 0, 0])
            or node.scale not in (None, [1, 1, 1]) or node.rotation not in (None, [0, 0, 0, 1])):
        raise ValueError("apply node transforms in the authoring tool before export")
    mesh = one(doc.meshes, "mesh")
    primitive = one(mesh.primitives, "triangle primitive")
    if primitive.mode != 4 or primitive.extensions or mesh.extensions:
        raise ValueError("expected uncompressed triangle primitives")
    if primitive.targets or mesh.weights:
        raise ValueError("bake morph targets before exporting")
    attrs = primitive.attributes
    if any(value is not None for key, value in vars(attrs).items() if key not in {"POSITION", "NORMAL", "TEXCOORD_0"}):
        raise ValueError("export only POSITION, NORMAL and optional TEXCOORD_0 attributes")
    blob = doc.binary_blob()

    def read(index, shape, formats, count_limit):
        acc = entry(doc.accessors, index, "accessor")
        if acc.type != shape or acc.componentType not in formats or acc.normalized or acc.sparse or acc.extensions:
            raise ValueError(f"unsupported {shape} accessor format")
        view = entry(doc.bufferViews, acc.bufferView, "buffer view")
        unpack = struct.Struct("<" + formats[acc.componentType])
        stride = unpack.size if view.byteStride is None else view.byteStride
        offset = acc.byteOffset
        start = view.byteOffset
        if (view.buffer != 0 or view.extensions or type(acc.count) is not int or not 1 <= acc.count <= count_limit
                or any(type(x) is not int or x < 0 for x in [stride, offset, start, view.byteLength])
                or stride < unpack.size or stride > 252 or start + view.byteLength > buffer.byteLength
                or offset + (acc.count - 1) * stride + unpack.size > view.byteLength):
            raise ValueError("accessor count or span outside buffer bounds")
        return [component for row in range(acc.count)
                for component in unpack.unpack_from(blob, start + offset + row * stride)]

    positions = read(attrs.POSITION, "VEC3", {5126: "fff"}, max_records)
    normals = read(attrs.NORMAL, "VEC3", {5126: "fff"}, max_records)
    count = len(positions) // 3
    uvs = ([0, 0] * count if attrs.TEXCOORD_0 is None
           else read(attrs.TEXCOORD_0, "VEC2", {5126: "ff"}, max_records))
    indices = (list(range(count)) if primitive.indices is None
               else read(primitive.indices, "SCALAR", {5121: "B", 5123: "H", 5125: "I"}, 3 * max_records))
    if len(normals) != 3 * count or len(uvs) != 2 * count or len(indices) % 3 or max(indices) >= count:
        raise ValueError("inconsistent triangle attributes or indices")
    if any(not math.isfinite(x) for stream in (positions, normals, uvs) for x in stream):
        raise ValueError("non-finite mesh attribute")
    for start in range(0, len(normals), 3):
        if abs(sum(x*x for x in normals[start:start + 3]) - 1) > .02:
            raise ValueError("GLB normals must be unit length")
    for i in range(0, len(indices), 3):
        a, b, c = [index * 3 for index in indices[i:i + 3]]
        x, y, z = (positions[b+k] - positions[a+k] for k in range(3))
        u, v, w = (positions[c+k] - positions[a+k] for k in range(3))
        if (y*w - z*v, z*u - x*w, x*v - y*u) == (0, 0, 0):
            raise ValueError("degenerate GLB triangle")
    return {"version": 1, "positions": positions, "normals": normals, "uvs": uvs, "indices": indices}

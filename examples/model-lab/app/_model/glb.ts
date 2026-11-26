import type { MeshAttributes } from "forge:mesh";

export interface GlbOptions {
  name: string;
  extras: Record<string, string | number>;
  /** Linear RGB, matching Three's material.color storage. */
  color: [number, number, number];
  roughness: number;
  metalness: number;
}

/** A single indexed, opaque PBR model. No scene traversal, textures or file reads. */
export function encodeGlb(mesh: MeshAttributes, options: GlbOptions): ArrayBuffer {
  const { positions, normals, uvs, indices } = mesh;
  const vertexCount = positions.length / 3;
  if (!(positions instanceof Float32Array) || !(normals instanceof Float32Array)
      || !(uvs instanceof Float32Array) || !(indices instanceof Uint32Array)
      || !Number.isInteger(vertexCount) || vertexCount < 3 || vertexCount > 1_000_000
      || normals.length !== positions.length || uvs.length !== vertexCount * 2
      || indices.length < 3 || indices.length % 3 !== 0 || indices.length > 6_000_000) {
    throw new Error("GLB requires bounded, indexed positions, normals and UVs.");
  }
  const minimum = [Infinity, Infinity, Infinity];
  const maximum = [-Infinity, -Infinity, -Infinity];
  for (let i = 0; i < positions.length; i++) {
    if (!Number.isFinite(positions[i]) || !Number.isFinite(normals[i])) {
      throw new Error("GLB attributes must be finite.");
    }
    const axis = i % 3;
    minimum[axis] = Math.min(minimum[axis], positions[i]);
    maximum[axis] = Math.max(maximum[axis], positions[i]);
  }
  for (let i = 0; i < normals.length; i += 3) {
    if (Math.abs(Math.hypot(normals[i], normals[i + 1], normals[i + 2]) - 1) > 0.002) {
      throw new Error("GLB normals must have unit length.");
    }
  }
  if (!uvs.every(Number.isFinite) || !indices.every((index) => index < vertexCount)) {
    throw new Error("GLB requires finite UVs and indices within the vertex buffer.");
  }
  const unitValue = (value: number) => Number.isFinite(value) && value >= 0 && value <= 1;
  if (!Array.isArray(options.color) || options.color.length !== 3 || !options.color.every(unitValue)
      || !unitValue(options.roughness) || !unitValue(options.metalness)) {
    throw new Error("GLB material components must be between zero and one.");
  }
  if (typeof options.name !== "string" || !options.extras || typeof options.extras !== "object"
      || !Object.values(options.extras).every((value) => typeof value === "string" || (typeof value === "number" && Number.isFinite(value)))) {
    throw new Error("GLB metadata must contain a name and finite numbers or strings.");
  }

  const arrays = [positions, normals, uvs, indices];
  let binaryLength = 0;
  const bufferViews = arrays.map((array, index) => {
    const view = { buffer: 0, byteOffset: binaryLength, byteLength: array.byteLength, target: index === 3 ? 34963 : 34962 };
    binaryLength += array.byteLength;
    return view;
  });
  const document = {
    asset: { version: "2.0", generator: "Next.RS Rust model lab" },
    scene: 0,
    scenes: [{ nodes: [0] }],
    nodes: [{ mesh: 0, name: options.name, extras: options.extras }],
    meshes: [{ primitives: [{ attributes: { POSITION: 0, NORMAL: 1, TEXCOORD_0: 2 }, indices: 3, material: 0, mode: 4 }] }],
    materials: [{ pbrMetallicRoughness: { baseColorFactor: [...options.color, 1], roughnessFactor: options.roughness, metallicFactor: options.metalness } }],
    buffers: [{ byteLength: binaryLength }],
    bufferViews,
    accessors: [
      { bufferView: 0, componentType: 5126, count: vertexCount, type: "VEC3", min: minimum, max: maximum },
      { bufferView: 1, componentType: 5126, count: vertexCount, type: "VEC3" },
      { bufferView: 2, componentType: 5126, count: vertexCount, type: "VEC2" },
      { bufferView: 3, componentType: 5125, count: indices.length, type: "SCALAR" },
    ],
  };
  const json = new TextEncoder().encode(JSON.stringify(document));
  const jsonLength = Math.ceil(json.length / 4) * 4;
  const binaryStart = 12 + 8 + jsonLength + 8;
  const length = binaryStart + binaryLength;
  if (length > 0xffffffff) throw new Error("GLB exceeds its 32-bit file size limit.");
  const result = new ArrayBuffer(length);
  const view = new DataView(result);
  const bytes = new Uint8Array(result);
  view.setUint32(0, 0x46546c67, true);
  view.setUint32(4, 2, true);
  view.setUint32(8, length, true);
  view.setUint32(12, jsonLength, true);
  view.setUint32(16, 0x4e4f534a, true);
  bytes.fill(0x20, 20, 20 + jsonLength);
  bytes.set(json, 20);
  view.setUint32(binaryStart - 8, binaryLength, true);
  view.setUint32(binaryStart - 4, 0x004e4942, true);
  // All attributes use 4-byte components, so every buffer view is aligned.
  // DataView makes the glTF little-endian requirement independent of the host.
  for (let index = 0; index < arrays.length; index++) {
    const array = arrays[index];
    const start = binaryStart + bufferViews[index].byteOffset;
    for (let i = 0; i < array.length; i++) {
      if (index === 3) view.setUint32(start + i * 4, array[i], true);
      else view.setFloat32(start + i * 4, array[i], true);
    }
  }
  return result;
}

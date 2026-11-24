// forge:mesh — an engine-independent boundary for the versioned Rust mesh ABI.
// Every returned array is JS-owned. Rust may grow its memory on the next call.

const MAX_WASM_BYTES = 16 * 1024 * 1024;
const MAX_VERTICES = 1_000_000;
const MAX_INDICES = 6_000_000;
const KIND_CODES = Object.freeze({ sphere: 0, torus: 1, rock: 2, terrain: 3 });

function checkSize(size) {
  if (!Number.isSafeInteger(size) || size < 1 || size > MAX_WASM_BYTES) {
    throw new RangeError("WASM module size must be between 1 byte and 16 MiB.");
  }
}

function abortIfNeeded(signal) {
  if (signal?.aborted) throw signal.reason ?? new DOMException("Mesh loading was cancelled.", "AbortError");
}

async function readWasm(source, signal) {
  abortIfNeeded(signal);
  if (source instanceof Uint8Array || source instanceof ArrayBuffer) {
    checkSize(source.byteLength);
    // Snapshot caller-owned bytes before asynchronous compilation.
    return source instanceof Uint8Array ? new Uint8Array(source) : source.slice(0);
  }
  if (!(typeof source === "string" || source instanceof URL)) {
    throw new TypeError("WASM source must be a URL, ArrayBuffer, or Uint8Array.");
  }
  const url = String(source);
  if (url.startsWith("data:")) {
    const match = /^data:application\/wasm;base64,([A-Za-z0-9+/]*={0,2})$/i.exec(url);
    if (!match || match[1].length > Math.ceil(MAX_WASM_BYTES / 3) * 4) {
      throw new TypeError("Invalid or oversized embedded WASM data URL.");
    }
    let decoded;
    try { decoded = atob(match[1]); }
    catch { throw new TypeError("Invalid embedded WASM data URL encoding."); }
    checkSize(decoded.length);
    return Uint8Array.from(decoded, (character) => character.charCodeAt(0));
  }
  const response = await fetch(url, { signal });
  if (!response.ok) throw new Error(`WASM request failed (${response.status}).`);
  const declaredSize = response.headers.get("content-length");
  if (declaredSize !== null) checkSize(Number(declaredSize));
  if (!response.body) throw new Error("WASM response has no body.");
  const reader = response.body.getReader();
  const chunks = [];
  let size = 0;
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      size += value.byteLength;
      checkSize(size);
      chunks.push(value);
    }
  } catch (error) {
    await reader.cancel().catch(() => {});
    throw error;
  } finally { reader.releaseLock(); }
  checkSize(size);
  const bytes = new Uint8Array(size);
  let offset = 0;
  for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.byteLength; }
  return bytes;
}

function parseMeshAbi(exports) {
  if (!(exports.memory instanceof WebAssembly.Memory)) throw new TypeError("Invalid mesh ABI: missing exported memory.");
  for (const name of ["mesh_abi_version", "mesh_generate", "mesh_vertex_count", "mesh_index_count", "mesh_positions_ptr", "mesh_normals_ptr", "mesh_uvs_ptr", "mesh_indices_ptr", "mesh_clear"]) {
    if (typeof exports[name] !== "function") throw new TypeError(`Invalid mesh ABI: missing ${name}().`);
  }
  if (exports.mesh_abi_version() !== 1) throw new TypeError("Unsupported mesh ABI version; expected version 1.");
  return exports;
}

function parseOptions(options) {
  if (options === null || typeof options !== "object") throw new TypeError("Mesh options must be an object.");
  const { kind = "torus", detail = 48, seed = 17, scale = 1, variation = 0.5 } = options;
  if (typeof kind !== "string" || !Object.hasOwn(KIND_CODES, kind)) throw new RangeError("Unknown mesh kind; use sphere, torus, rock, or terrain.");
  if (!Number.isInteger(detail) || detail < 8 || detail > 128) throw new RangeError("Mesh detail must be an integer between 8 and 128.");
  if (!Number.isInteger(seed) || seed < 0 || seed > 0xffffffff) throw new RangeError("Mesh seed must be an unsigned 32-bit integer.");
  if (!Number.isFinite(scale) || scale < 0.01 || scale > 100) throw new RangeError("Mesh scale must be between 0.01 and 100 metres.");
  if (!Number.isFinite(variation) || variation < 0 || variation > 1) throw new RangeError("Mesh variation must be between 0 and 1.");
  return { kind: KIND_CODES[kind], detail, seed, scale, variation };
}

function copySpan(ArrayType, buffer, pointer, length, label) {
  const bytes = length * ArrayType.BYTES_PER_ELEMENT;
  if (!Number.isSafeInteger(pointer) || pointer < 0 || pointer % 4 !== 0 || pointer + bytes > buffer.byteLength) {
    throw new RangeError(`Invalid mesh ABI: ${label} points outside WASM memory or is misaligned.`);
  }
  return new ArrayType(buffer, pointer, length).slice();
}

/** Validate typed mesh attributes and calculate axis-aligned bounds. Arrays are not copied. */
export function validateMesh(mesh) {
  if (mesh === null || typeof mesh !== "object") throw new TypeError("Mesh must contain typed attribute arrays.");
  const { positions, normals, uvs, indices } = mesh;
  if (!(positions instanceof Float32Array) || !(normals instanceof Float32Array) || !(uvs instanceof Float32Array) || !(indices instanceof Uint32Array)) {
    throw new TypeError("Mesh requires Float32Array positions/normals/uvs and Uint32Array indices.");
  }
  const vertexCount = positions.length / 3;
  if (!Number.isInteger(vertexCount) || vertexCount < 3 || vertexCount > MAX_VERTICES || normals.length !== positions.length || uvs.length !== vertexCount * 2) {
    throw new RangeError("Mesh attribute lengths are inconsistent or exceed the vertex limit.");
  }
  if (indices.length < 3 || indices.length % 3 !== 0 || indices.length > MAX_INDICES) throw new RangeError("Mesh index count must describe bounded complete triangles.");
  const min = [Infinity, Infinity, Infinity];
  const max = [-Infinity, -Infinity, -Infinity];
  for (let vertex = 0; vertex < vertexCount; vertex++) {
    let squaredNormal = 0;
    for (let axis = 0; axis < 3; axis++) {
      const position = positions[vertex * 3 + axis];
      const normal = normals[vertex * 3 + axis];
      if (!Number.isFinite(position) || !Number.isFinite(normal)) throw new RangeError("Mesh contains a non-finite position or normal.");
      min[axis] = Math.min(min[axis], position);
      max[axis] = Math.max(max[axis], position);
      squaredNormal += normal * normal;
    }
    if (Math.abs(squaredNormal - 1) > 0.02) throw new RangeError("Mesh normals must have unit length.");
    if (!Number.isFinite(uvs[vertex * 2]) || !Number.isFinite(uvs[vertex * 2 + 1])) throw new RangeError("Mesh contains non-finite UV coordinates.");
  }
  for (let triangle = 0; triangle < indices.length; triangle += 3) {
    const a = indices[triangle], b = indices[triangle + 1], c = indices[triangle + 2];
    if (a >= vertexCount || b >= vertexCount || c >= vertexCount) throw new RangeError("Mesh index points outside the vertex array.");
    if (a === b || a === c || b === c) throw new RangeError("Mesh triangle repeats a vertex index.");
    const abx = positions[b * 3] - positions[a * 3];
    const aby = positions[b * 3 + 1] - positions[a * 3 + 1];
    const abz = positions[b * 3 + 2] - positions[a * 3 + 2];
    const acx = positions[c * 3] - positions[a * 3];
    const acy = positions[c * 3 + 1] - positions[a * 3 + 1];
    const acz = positions[c * 3 + 2] - positions[a * 3 + 2];
    if (Math.hypot(aby * acz - abz * acy, abz * acx - abx * acz, abx * acy - aby * acx) === 0) {
      throw new RangeError("Mesh contains a degenerate triangle.");
    }
  }
  return { positions, normals, uvs, indices, vertexCount, triangleCount: indices.length / 3, bounds: { min, max } };
}

/** Load a Rust ABI-v1 generator. Embedded data URLs are decoded without fetch(). */
export async function loadMeshGenerator(source, { signal } = {}) {
  const bytes = await readWasm(source, signal);
  abortIfNeeded(signal);
  const result = await WebAssembly.instantiate(bytes);
  let abi = parseMeshAbi(result.instance.exports);
  if (signal?.aborted) { abi.mesh_clear(); abortIfNeeded(signal); }
  return {
    generate(options = {}) {
      if (!abi) throw new Error("Mesh generator has been disposed.");
      const { kind, detail, seed, scale, variation } = parseOptions(options);
      const status = abi.mesh_generate(kind, detail, seed, scale, variation);
      if (status !== 0) {
        const reasons = ["", "unknown kind", "detail outside 8–128", "invalid scale", "invalid variation", "allocation failed"];
        throw new Error(`Rust mesh generation failed: ${reasons[status] ?? `status ${status}`}.`);
      }
      const vertices = abi.mesh_vertex_count();
      const count = abi.mesh_index_count();
      if (!Number.isSafeInteger(vertices) || vertices < 3 || vertices > MAX_VERTICES || !Number.isSafeInteger(count) || count < 3 || count > MAX_INDICES || count % 3 !== 0) {
        throw new RangeError("Invalid mesh ABI: unsafe vertex or index count.");
      }
      // Acquire after generation: Rust allocations may have grown/detached memory.
      const buffer = abi.memory.buffer;
      return validateMesh({
        positions: copySpan(Float32Array, buffer, abi.mesh_positions_ptr(), vertices * 3, "positions"),
        normals: copySpan(Float32Array, buffer, abi.mesh_normals_ptr(), vertices * 3, "normals"),
        uvs: copySpan(Float32Array, buffer, abi.mesh_uvs_ptr(), vertices * 2, "uvs"),
        indices: copySpan(Uint32Array, buffer, abi.mesh_indices_ptr(), count, "indices"),
      });
    },
    get memoryBytes() { return abi?.memory.buffer.byteLength ?? 0; },
    dispose() {
      if (abi) { abi.mesh_clear(); abi = null; }
    },
  };
}

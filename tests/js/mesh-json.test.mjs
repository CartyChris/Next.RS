import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";

const source = await readFile(new URL("../../src/runtime/js/mesh.js", import.meta.url));
const { parseMeshJson, readWasm } = await import(`data:text/javascript;base64,${source.toString("base64")}`);
const mesh = () => ({
  version: 1, positions: [0, 0, 0, 1, 0, 0, 0, 1, 0],
  normals: [0, 0, 1.004, 0, 0, 1, 0, 0, 1], uvs: [0, 0, 1, 0, 0, 1], indices: [0, 1, 2],
});

test("mesh JSON converts portable arrays to owned typed data with exportable unit normals", () => {
  const input = mesh();
  const result = parseMeshJson(input);
  input.positions[0] = 12;
  assert.ok(result.indices instanceof Uint32Array);
  assert.ok(result.positions instanceof Float32Array);
  assert.equal(result.positions[0], 0);
  assert.equal(result.normals[2], 1);
  assert.deepEqual(result.bounds, {min: [0, 0, 0], max: [1, 1, 0]});
});

test("mesh JSON rejects lossy index or attribute coercion, topology, oversized spans and version errors", () => {
  for (const change of [
    m => { m.version = 2; }, m => { m.positions[0] = "0"; }, m => { m.normals[2] = 0; },
    m => { m.indices[1] = 1.1; }, m => { m.indices[0] = 2 ** 32; }, m => { m.indices[0] = -1; },
    m => { m.indices[0] = 9; }, m => { m.positions[5] = 1e100; }, m => { m.uvs[3] = null; },
    m => { m.indices = [0, 2, 2]; }, m => { m.uvs = new Array(2_000_001); },
    m => { delete m.indices[1]; },
  ]) {
    const invalid = mesh(); change(invalid);
    assert.throws(() => parseMeshJson(invalid), /mesh|indices|normals/i);
  }
});

test("custom Wasm adapters get bounded, cancellable, owned bytes without fetching embedded URLs", async () => {
  const bytes = Uint8Array.of(0, 97, 115, 109, 1, 0, 0, 0);
  const result = await readWasm(bytes);
  assert.notEqual(result.buffer, bytes.buffer);
  assert.deepEqual(await readWasm("data:application/wasm;base64,AGFzbQEAAAA="), bytes);
  await assert.rejects(readWasm(bytes, AbortSignal.abort()), { name: "AbortError" });
});

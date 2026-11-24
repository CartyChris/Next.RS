import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after, test } from "node:test";
import { fileURLToPath } from "node:url";

const source = await readFile(new URL("../../src/runtime/js/mesh.js", import.meta.url), "utf8");
const { loadMeshGenerator, validateMesh } = await import(`data:text/javascript;base64,${Buffer.from(source).toString("base64")}`);

// Test the real dependency-free Rust binary. A missing toolchain is a test failure,
// not a silently skipped WASM integration check.
const target = await mkdtemp(join(tmpdir(), "forge-mesh-test-"));
after(() => rm(target, { recursive: true, force: true }));
const manifest = fileURLToPath(new URL("../../examples/model-lab/rust/mesh/Cargo.toml", import.meta.url));
execFileSync("cargo", ["build", "--manifest-path", manifest, "--target", "wasm32-unknown-unknown", "--release", "--locked", "--target-dir", target], { stdio: "pipe", timeout: 120_000 });
const wasm = await readFile(join(target, "wasm32-unknown-unknown/release/forge_procedural_mesh.wasm"));

function triangle() {
  return {
    positions: new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]),
    normals: new Float32Array([0, 0, 1, 0, 0, 1, 0, 0, 1]),
    uvs: new Float32Array([0, 0, 1, 0, 0, 1]),
    indices: new Uint32Array([0, 1, 2]),
  };
}

// Tiny real WASM fixtures exercise malformed ABI output without replacing any
// module or pretending JavaScript functions are Rust exports.
function abiFixture(overrides = {}) {
  const values = { version: 1, vertices: 3, indices: 3, positions: 16, normals: 52, uvs: 88, triangles: 112, ...overrides };
  const unsigned = (value) => {
    const bytes = [];
    do { const next = value & 127; value >>>= 7; bytes.push(next | (value ? 128 : 0)); } while (value);
    return bytes;
  };
  const signed = (value) => {
    const bytes = [];
    while (true) {
      const next = value & 127;
      value >>= 7;
      const done = (value === 0 && !(next & 64)) || (value === -1 && (next & 64));
      bytes.push(next | (done ? 0 : 128));
      if (done) return bytes;
    }
  };
  const text = (value) => [...unsigned(value.length), ...Buffer.from(value)];
  const section = (id, contents) => [id, ...unsigned(contents.length), ...contents];
  const functions = [
    ["mesh_abi_version", 0, values.version], ["mesh_generate", 1, 0],
    ["mesh_vertex_count", 0, values.vertices], ["mesh_index_count", 0, values.indices],
    ["mesh_positions_ptr", 0, values.positions], ["mesh_normals_ptr", 0, values.normals],
    ["mesh_uvs_ptr", 0, values.uvs], ["mesh_indices_ptr", 0, values.triangles],
    ["mesh_clear", 2, null],
  ];
  const exports = functions.flatMap(([name], index) => [...text(name), 0, index]);
  const bodies = functions.flatMap(([, , value]) => {
    const body = value === null ? [0, 11] : [0, 65, ...signed(value), 11];
    return [...unsigned(body.length), ...body];
  });
  const data = new Uint8Array(124);
  const mesh = triangle();
  for (const [array, offset] of [[mesh.positions, 16], [mesh.normals, 52], [mesh.uvs, 88], [mesh.indices, 112]]) {
    data.set(new Uint8Array(array.buffer), offset);
  }
  return new Uint8Array([
    0, 97, 115, 109, 1, 0, 0, 0,
    ...section(1, [3, 96, 0, 1, 127, 96, 5, 127, 127, 127, 125, 125, 1, 127, 96, 0, 0]),
    ...section(3, [functions.length, ...functions.map(([, type]) => type)]),
    ...section(5, [1, 0, 1]),
    ...section(7, [functions.length + 1, ...exports, ...text("memory"), 2, 0]),
    ...section(10, [functions.length, ...bodies]),
    ...section(11, [1, 0, 65, 0, 11, ...unsigned(data.length), ...data]),
  ]);
}

test("mesh validator calculates bounds and rejects malformed attributes/topology", () => {
  const mesh = validateMesh(triangle());
  assert.deepEqual(mesh.bounds, { min: [0, 0, 0], max: [1, 1, 0] });
  assert.equal(mesh.vertexCount, 3);
  assert.equal(mesh.triangleCount, 1);
  for (const corrupt of [
    (m) => { m.positions[0] = NaN; },
    (m) => { m.normals[0] = Infinity; },
    (m) => { m.normals.fill(0); },
    (m) => { m.uvs[1] = NaN; },
    (m) => { m.indices[0] = 3; },
    (m) => { m.indices[0] = 1; },
    (m) => { m.indices = new Uint32Array([0, 1]); },
    (m) => { m.normals = new Float32Array(6); },
    (m) => { m.positions = [0, 0, 0]; },
  ]) {
    const invalid = triangle();
    corrupt(invalid);
    assert.throws(() => validateMesh(invalid), /mesh/i);
  }
});

test("real Rust module has the versioned mesh ABI and requires no host imports", async () => {
  const module = await WebAssembly.compile(wasm);
  assert.deepEqual(WebAssembly.Module.imports(module), []);
  const instance = await WebAssembly.instantiate(module);
  assert.equal(instance.exports.mesh_abi_version(), 1);
  assert.ok(instance.exports.memory instanceof WebAssembly.Memory);
  assert.equal(instance.exports.mesh_generate(0, 0xffffffff, 0, 1, 0.5), 2);
  assert.equal(instance.exports.mesh_generate(0, 32, 0, NaN, 0.5), 3);
  assert.equal(instance.exports.mesh_generate(0, 32, 0, 1, Infinity), 4);
  assert.equal(instance.exports.mesh_vertex_count(), 0);
});

test("loading snapshots caller-owned bytes before asynchronous compilation", async () => {
  for (const bytes of [new Uint8Array(wasm), Buffer.from(wasm)]) {
    const pending = loadMeshGenerator(bytes);
    bytes.fill(0);
    const generator = await pending;
    try { assert.ok(generator.generate({ detail: 8 }).vertexCount > 0); }
    finally { generator.dispose(); }
  }
});

test("every Rust shape generates indexed geometry with finite unit normals", async () => {
  const generator = await loadMeshGenerator(wasm);
  try {
    for (const kind of ["sphere", "torus", "rock", "terrain"]) {
      for (const detail of [8, 32, 128]) {
        const mesh = generator.generate({ kind, detail, seed: 17 });
        assert.ok(mesh.vertexCount > 0 && mesh.vertexCount <= 16_641);
        assert.ok(mesh.triangleCount > 0);
        assert.equal(mesh.positions.length, mesh.vertexCount * 3);
        assert.equal(mesh.normals.length, mesh.positions.length);
        assert.equal(mesh.uvs.length, mesh.vertexCount * 2);
        assert.equal(mesh.indices.length, mesh.triangleCount * 3);
        assert.deepEqual(validateMesh(mesh).bounds, mesh.bounds);
      }
    }
  } finally { generator.dispose(); }
});

test("seeded output is reproducible and copies survive subsequent memory growth", async () => {
  const generator = await loadMeshGenerator(wasm);
  try {
    for (const kind of ["rock", "terrain"]) {
      const first = generator.generate({ kind, detail: 8, seed: 17 });
      const saved = first.positions.slice();
      generator.generate({ kind: "terrain", detail: 128, seed: 99 });
      assert.deepEqual(first.positions, saved, "WASM growth must not detach or overwrite returned arrays");
      assert.deepEqual(generator.generate({ kind, detail: 8, seed: 17 }).positions, saved);
      assert.notDeepEqual(generator.generate({ kind, detail: 8, seed: 18 }).positions, saved);
    }
    for (let i = 0; i < 4; i++) generator.generate({ kind: "terrain", detail: 128, seed: i });
    const warmMemory = generator.memoryBytes;
    for (let i = 0; i < 50; i++) generator.generate({ kind: i % 2 ? "rock" : "terrain", detail: 128, seed: i });
    assert.equal(generator.memoryBytes, warmMemory, "repeated generation must reuse the WASM heap after warmup");
    assert.ok(warmMemory < 16 * 1024 * 1024);
  } finally { generator.dispose(); }
  assert.equal(generator.memoryBytes, 0);
  assert.doesNotThrow(() => generator.dispose());
  assert.throws(() => generator.generate(), /disposed/);
});

test("JavaScript boundary rejects unbounded, fractional and non-finite options", async () => {
  const generator = await loadMeshGenerator(wasm);
  try {
    for (const options of [
      { kind: "__proto__" }, { kind: 0 }, { detail: 7 }, { detail: 129 },
      { detail: 8.5 }, { seed: -1 }, { seed: 0x100000000 }, { seed: NaN },
      { scale: 0 }, { scale: Infinity }, { variation: -0.1 }, { variation: 1.1 }, null,
    ]) assert.throws(() => generator.generate(options), /mesh/i);
    assert.ok(generator.generate({ seed: 0xffffffff, detail: 8, scale: 0.01, variation: 0 }).vertexCount > 0);
  } finally { generator.dispose(); }
});

test("embedded WASM data URLs instantiate without fetching and malformed sources fail clearly", async () => {
  const generator = await loadMeshGenerator(`data:application/wasm;base64,${wasm.toString("base64")}`);
  assert.ok(generator.generate().vertexCount > 0);
  generator.dispose();
  await assert.rejects(loadMeshGenerator(new Uint8Array([0, 1, 2, 3])), /WebAssembly|magic|module/i);
  await assert.rejects(loadMeshGenerator(new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0])), /mesh ABI/i);
  await assert.rejects(loadMeshGenerator("data:application/wasm;base64,%%%"), /WASM data URL/i);
  await assert.rejects(loadMeshGenerator("data:text/plain;base64,AAAA"), /WASM data URL/i);
  await assert.rejects(loadMeshGenerator(new Uint8Array(0)), /WASM.*size/i);
  const controller = new AbortController();
  controller.abort();
  await assert.rejects(loadMeshGenerator(wasm, { signal: controller.signal }), { name: "AbortError" });
});

test("ABI validation rejects unsupported versions, oversized counts and unsafe pointers before reading", async () => {
  const valid = await loadMeshGenerator(abiFixture());
  assert.deepEqual(valid.generate().positions, triangle().positions);
  valid.dispose();
  await assert.rejects(loadMeshGenerator(abiFixture({ version: 2 })), /ABI version/);
  for (const invalid of [
    { vertices: 1_000_001 }, { vertices: -1 }, { indices: 4 }, { indices: 6_000_003 },
    { positions: 18 }, { normals: -4 }, { uvs: 65_532 }, { triangles: 65_536 },
  ]) {
    const generator = await loadMeshGenerator(abiFixture(invalid));
    try { assert.throws(() => generator.generate(), /Invalid mesh ABI/); }
    finally { generator.dispose(); }
  }
});

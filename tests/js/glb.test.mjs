import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { test } from "node:test";
import { pathToFileURL } from "node:url";

const source = await readFile(new URL("../../examples/model-lab/app/_model/glb.ts", import.meta.url), "utf8");
const requireExample = createRequire(new URL("../../examples/model-lab/package.json", import.meta.url));
const ts = requireExample("typescript");
const javascript = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } }).outputText;
const { encodeGlb } = await import(`data:text/javascript;base64,${Buffer.from(javascript).toString("base64")}`);
const { GLTFLoader } = await import(pathToFileURL(requireExample.resolve("three/addons/loaders/GLTFLoader.js")).href);

function triangle() {
  // Non-zero byte offsets expose accidental serialization of the whole backing buffer.
  return {
    positions: new Float32Array([99, -2.5, 0, 1, 2, 0, 1, 0, 3, 1, 99]).subarray(1, 10),
    normals: new Float32Array([99, 0, 0, 1, 0, 0, 1, 0, 0, 1, 99]).subarray(1, 10),
    uvs: new Float32Array([99, 0, 0, 1, 0, 0.5, 1, 99]).subarray(1, 7),
    indices: new Uint32Array([99, 0, 1, 2, 99]).subarray(1, 4),
  };
}

function options() {
  return {
    name: "Rust terrain · 石",
    extras: { generator: "Next.RS Rust mesh ABI v1", kind: "terrain", seed: 17, units: "metres", upAxis: "Y" },
    color: [0.2, 0.4, 0.6],
    roughness: 0.82,
    metalness: 0.05,
  };
}

function decode(buffer) {
  assert.ok(buffer instanceof ArrayBuffer, "encoding finishes synchronously with owned bytes");
  const bytes = Buffer.from(buffer);
  assert.equal(bytes.toString("ascii", 0, 4), "glTF");
  assert.equal(bytes.readUInt32LE(4), 2);
  assert.equal(bytes.readUInt32LE(8), bytes.length);
  const jsonLength = bytes.readUInt32LE(12);
  assert.equal(jsonLength % 4, 0);
  assert.equal(bytes.readUInt32LE(16), 0x4e4f534a);
  const jsonText = bytes.toString("utf8", 20, 20 + jsonLength);
  assert.match(jsonText.slice(jsonText.lastIndexOf("}") + 1), /^ {0,3}$/);
  const json = JSON.parse(jsonText);
  const binaryHeader = 20 + jsonLength;
  const binaryLength = bytes.readUInt32LE(binaryHeader);
  assert.equal(binaryLength % 4, 0);
  assert.equal(bytes.readUInt32LE(binaryHeader + 4), 0x004e4942);
  assert.equal(binaryHeader + 8 + binaryLength, bytes.length);
  const binary = bytes.subarray(binaryHeader + 8);
  assert.deepEqual(json.buffers, [{ byteLength: binary.length }]);
  return { json, binary };
}

function values({ json, binary }, accessorIndex) {
  const accessor = json.accessors[accessorIndex];
  const view = json.bufferViews[accessor.bufferView];
  const width = { VEC3: 3, VEC2: 2, SCALAR: 1 }[accessor.type];
  assert.equal(view.byteOffset % 4, 0);
  assert.equal(view.byteLength, accessor.count * width * 4);
  assert.ok(view.byteOffset + view.byteLength <= binary.length);
  const read = accessor.componentType === 5126 ? Buffer.prototype.readFloatLE : Buffer.prototype.readUInt32LE;
  return Array.from({ length: accessor.count * width }, (_, i) => read.call(binary, view.byteOffset + i * 4));
}

test("GLB writes aligned little-endian attributes, bounds, metadata and a canonical PBR model", () => {
  const mesh = triangle();
  const metadata = options();
  const decoded = decode(encodeGlb(mesh, metadata));
  const { json } = decoded;
  assert.equal(json.asset.version, "2.0");
  assert.equal(json.scene, 0);
  assert.deepEqual(json.scenes, [{ nodes: [0] }]);
  assert.deepEqual(json.nodes, [{ mesh: 0, name: metadata.name, extras: metadata.extras }]);
  assert.deepEqual(json.meshes[0].primitives, [{ attributes: { POSITION: 0, NORMAL: 1, TEXCOORD_0: 2 }, indices: 3, material: 0, mode: 4 }]);
  assert.deepEqual(json.materials[0].pbrMetallicRoughness, {
    baseColorFactor: [...metadata.color, 1], roughnessFactor: metadata.roughness, metallicFactor: metadata.metalness,
  });
  assert.deepEqual(json.accessors.map((a) => [a.componentType, a.count, a.type]), [
    [5126, 3, "VEC3"], [5126, 3, "VEC3"], [5126, 3, "VEC2"], [5125, 3, "SCALAR"],
  ]);
  assert.deepEqual(json.accessors[0].min, [-2.5, 0, 1]);
  assert.deepEqual(json.accessors[0].max, [2, 3, 1]);
  assert.deepEqual(json.bufferViews.map((view) => view.target), [34962, 34962, 34962, 34963]);
  for (const [i, key] of ["positions", "normals", "uvs", "indices"].entries()) {
    assert.deepEqual(values(decoded, i), [...mesh[key]]);
  }
  assert.equal(json.images, undefined);
  assert.equal(json.extensionsRequired, undefined);
});

test("GLB owns its complete snapshot without an asynchronous file-read dependency", () => {
  const mesh = triangle();
  const metadata = options();
  const bytes = encodeGlb(mesh, metadata);
  const snapshot = Buffer.from(bytes).toString("base64");
  for (const array of Object.values(mesh)) array.fill(0);
  metadata.name = "changed";
  metadata.extras.seed = 99;
  metadata.color[0] = 1;
  assert.equal(Buffer.from(bytes).toString("base64"), snapshot);
  assert.deepEqual(decode(bytes).json.nodes[0].extras.seed, 17);
});

test("GLB rejects malformed geometry and non-finite material or metadata inputs", () => {
  for (const corrupt of [
    (mesh) => { mesh.positions = [0, 0, 0]; },
    (mesh) => { mesh.positions = new Float32Array(0); },
    (mesh) => { mesh.positions[0] = NaN; },
    (mesh) => { mesh.normals[1] = Infinity; },
    (mesh) => { mesh.normals.fill(0); },
    (mesh) => { mesh.normals = new Float32Array(3); },
    (mesh) => { mesh.uvs = new Float32Array(4); },
    (mesh) => { mesh.uvs[1] = Infinity; },
    (mesh) => { mesh.indices = new Uint16Array([0, 1, 2]); },
    (mesh) => { mesh.indices = new Uint32Array([0, 1]); },
    (mesh) => { mesh.indices[2] = 3; },
  ]) {
    const mesh = triangle();
    corrupt(mesh);
    assert.throws(() => encodeGlb(mesh, options()), /GLB/);
  }
  for (const corrupt of [
    (metadata) => { metadata.color[1] = NaN; },
    (metadata) => { metadata.color[0] = -1; },
    (metadata) => { metadata.color.pop(); },
    (metadata) => { metadata.roughness = Infinity; },
    (metadata) => { metadata.metalness = 1.1; },
    (metadata) => { metadata.extras.seed = NaN; },
  ]) {
    const metadata = options();
    corrupt(metadata);
    assert.throws(() => encodeGlb(triangle(), metadata), /GLB/);
  }
});

test("GLB preserves indices larger than 65535 without truncation", () => {
  const mesh = triangle();
  mesh.positions = new Float32Array(65537 * 3);
  mesh.normals = new Float32Array(65537 * 3);
  mesh.uvs = new Float32Array(65537 * 2);
  for (let i = 0; i < 65537; i++) mesh.normals[i * 3 + 2] = 1;
  mesh.positions[3] = 1;
  mesh.positions[65536 * 3 + 1] = 1;
  mesh.indices = new Uint32Array([0, 1, 65536]);
  const decoded = decode(encodeGlb(mesh, options()));
  assert.equal(decoded.json.accessors[3].componentType, 5125);
  assert.deepEqual(values(decoded, 3), [0, 1, 65536]);
});

test("the real Three GLTFLoader reopens exported geometry, material, extras and identity transforms", async () => {
  const mesh = triangle();
  const metadata = options();
  const imported = await new GLTFLoader().parseAsync(encodeGlb(mesh, metadata), "");
  const meshes = [];
  imported.scene.traverse((object) => { if (object.isMesh) meshes.push(object); });
  assert.equal(meshes.length, 1);
  const loaded = meshes[0];
  try {
    assert.deepEqual([...loaded.geometry.getAttribute("position").array], [...mesh.positions]);
    assert.deepEqual([...loaded.geometry.getAttribute("normal").array], [...mesh.normals]);
    assert.deepEqual([...loaded.geometry.getAttribute("uv").array], [...mesh.uvs]);
    assert.deepEqual([...loaded.geometry.index.array], [...mesh.indices]);
    assert.deepEqual(loaded.position.toArray(), [0, 0, 0]);
    assert.deepEqual(loaded.quaternion.toArray(), [0, 0, 0, 1]);
    assert.deepEqual(loaded.scale.toArray(), [1, 1, 1]);
    assert.equal(loaded.material.isMeshStandardMaterial, true);
    assert.equal(loaded.material.wireframe, false);
    assert.deepEqual(loaded.material.color.toArray(), metadata.color);
    assert.equal(loaded.material.roughness, metadata.roughness);
    assert.equal(loaded.material.metalness, metadata.metalness);
    assert.equal(loaded.userData.seed, 17);
    assert.equal(loaded.userData.units, "metres");
  } finally { loaded.geometry.dispose(); loaded.material.dispose(); }
});

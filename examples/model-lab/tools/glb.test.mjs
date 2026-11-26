import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after, test } from "node:test";
import { fileURLToPath } from "node:url";

const ts = createRequire(new URL("../package.json", import.meta.url))("typescript");
const source = await readFile(new URL("../app/_model/glb.ts", import.meta.url), "utf8");
const compiled = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } });
const moduleUrl = text => `data:text/javascript;base64,${Buffer.from(text).toString("base64")}`;
const { encodeGlb } = await import(moduleUrl(compiled.outputText));
const { parseMeshJson } = await import(moduleUrl(await readFile(new URL("../../../src/runtime/js/mesh.js", import.meta.url), "utf8")));
const dir = await mkdtemp(join(tmpdir(), "next-rs-glb-import-"));
after(() => rm(dir, { recursive: true, force: true }));
const cli = fileURLToPath(new URL("./mesh.py", import.meta.url));
const python = (...args) => spawnSync("python3", [cli, ...args], { encoding: "utf8", maxBuffer: 2 * 1024 * 1024 });
const pawn = parseMeshJson(JSON.parse(python("pawn", "--segments", "8").stdout));
const glb = Buffer.from(encodeGlb(pawn, { name: "pawn", extras: {}, color: [1, 1, 1], roughness: 1, metalness: 0 }));
const binHeader = 20 + glb.readUInt32LE(12);

function editHeader(change) {
  const doc = JSON.parse(glb.toString("utf8", 20, binHeader));
  change(doc);
  const json = Buffer.from(JSON.stringify(doc));
  const padded = Math.ceil(json.length / 4) * 4;
  const bytes = Buffer.alloc(20 + padded + glb.length - binHeader, 32);
  glb.copy(bytes, 0, 0, 20);
  bytes.writeUInt32LE(bytes.length, 8);
  bytes.writeUInt32LE(padded, 12);
  json.copy(bytes, 20);
  glb.copy(bytes, 20 + padded, binHeader);
  return bytes;
}

test("actual Next.RS GLB export round-trips through Python and the production JSON importer", async () => {
  const path = join(dir, "pawn.glb");
  await writeFile(path, glb);
  const result = python("glb", path);
  assert.equal(result.status, 0, result.stderr);
  const json = JSON.parse(result.stdout);
  for (const key of ["positions", "normals", "uvs", "indices"]) assert.deepEqual(json[key], [...pawn[key]], key);
  const loaded = parseMeshJson(json);
  for (const key of ["positions", "uvs", "indices"]) assert.deepEqual(loaded[key], pawn[key], key);
  // The browser renormalizes imported normals once; float32 rounding can change a final bit.
  assert.ok(loaded.normals.every((component, i) => Math.abs(component - pawn.normals[i]) < 1e-7));
});

test("unsupported GLB scenes, malformed accessors and external resources fail without overwriting output", async () => {
  const path = join(dir, "unsupported.glb"), out = join(dir, "keep.json");
  await writeFile(out, "keep");
  const fixtures = [
    [glb.subarray(0, glb.length - 1), /length|chunk/i],
    [editHeader(d => { d.nodes[0].scale = [1, 2, 1]; }), /transform/i],
    [editHeader(d => { d.nodes.push({ mesh: 0 }); }), /single|one/i],
    [editHeader(d => { d.animations = [{ channels: [], samplers: [] }]; }), /static|animation/i],
    [editHeader(d => { d.meshes[0].primitives[0].targets = [{ POSITION: 0 }]; }), /morph/i],
    [editHeader(d => { d.skins = [{ joints: [0] }]; }), /static|skin/i],
    [editHeader(d => { d.meshes[0].primitives[0].mode = 5; }), /triangle/i],
    [editHeader(d => { d.buffers[0].uri = "./external.bin"; }), /self-contained|URI|external/i],
    [editHeader(d => { d.images = [{ uri: "./external.png" }]; }), /self-contained|URI|external/i],
    [editHeader(d => { d.accessors[0].count = 1000000000; }), /bounds|count/i],
    [editHeader(d => { d.bufferViews[0].byteLength = 4; }), /bounds|span/i],
  ];
  for (const [bytes, expected] of fixtures) {
    await writeFile(path, bytes);
    const result = python("glb", path, "-o", out);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, expected);
    assert.equal(await readFile(out, "utf8"), "keep");
  }
});

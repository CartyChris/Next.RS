import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after, test } from "node:test";
import { fileURLToPath } from "node:url";

const cli = fileURLToPath(new URL("./mesh.py", import.meta.url));
const runtime = await readFile(new URL("../../../src/runtime/js/mesh.js", import.meta.url), "utf8");
const { parseMeshJson } = await import(`data:text/javascript;base64,${Buffer.from(runtime).toString("base64")}`);
const dir = await mkdtemp(join(tmpdir(), "next-rs-python-mesh-"));
after(() => rm(dir, { recursive: true, force: true }));
const invoke = (...args) => spawnSync("python3", [cli, ...args], { encoding: "utf8", maxBuffer: 8 * 1024 * 1024 });

test("Python pawn passes the production mesh importer with outward triangle winding", () => {
  for (const segments of [8, 32, 128]) {
    const result = invoke("pawn", "--segments", String(segments));
    assert.equal(result.status, 0, result.stderr);
    const mesh = parseMeshJson(JSON.parse(result.stdout));
    assert.ok(mesh.triangleCount > segments * 10);
    assert.equal(mesh.bounds.min[1], 0);
    assert.ok(mesh.bounds.max[1] > 1);
    const { positions, normals, indices } = mesh;
    for (let i = 0; i < indices.length; i += 3) {
      const [a, b, c] = indices.slice(i, i + 3).map(index => 3 * index);
      const ab = [0, 1, 2].map(k => positions[b + k] - positions[a + k]);
      const ac = [0, 1, 2].map(k => positions[c + k] - positions[a + k]);
      const face = [
        ab[1] * ac[2] - ab[2] * ac[1],
        ab[2] * ac[0] - ab[0] * ac[2],
        ab[0] * ac[1] - ab[1] * ac[0],
      ];
      const facing = face.reduce((dot, x, k) => dot + x * (normals[a + k] + normals[b + k] + normals[c + k]), 0);
      assert.ok(facing > 0, `triangle ${i / 3} faces inward`);
    }
  }
  assert.notEqual(invoke("pawn", "--segments", "100000").status, 0);
});

const vertices = "v 0 0 0\nv 1 0 0\nv 0 1 0\n";
test("OBJ preserves indexed UV seams and handles normals and relative references", async () => {
  const path = join(dir, "seams.obj"), out = join(dir, "seams.mesh.json");
  await writeFile(path, vertices + "v 1 1 0\nvt 0 0\nvt 1 0\nvt 0 1\nvt 1 1\nvt 0.5 0.5\nvn 0 0 2\n" +
    "f 1/1/1 2/2/1 3/3/1\nf -3/5/-1 -1/4/-1 -2/3/-1\n");
  const result = invoke("obj", path, "-o", out);
  assert.equal(result.status, 0, result.stderr);
  const mesh = parseMeshJson(JSON.parse(await readFile(out, "utf8")));
  assert.equal(mesh.vertexCount, 5);
  assert.equal(mesh.triangleCount, 2);
  assert.deepEqual([...mesh.uvs.slice(6, 8)], [0.5, 0.5]);
  assert.ok(mesh.normals.every((value, i) => value === (i % 3 === 2 ? 1 : 0)));
});

test("position-only OBJ receives flat normals and zero UVs", async () => {
  const path = join(dir, "flat.obj");
  await writeFile(path, vertices + "f -3 -2 -1\n");
  const result = invoke("obj", path);
  assert.equal(result.status, 0, result.stderr);
  const mesh = parseMeshJson(JSON.parse(result.stdout));
  assert.deepEqual([...mesh.normals], [0, 0, 1, 0, 0, 1, 0, 0, 1]);
  assert.ok(mesh.uvs.every(value => value === 0));
});

test("malformed or unsupported OBJ leaves existing output intact", async () => {
  const path = join(dir, "bad.obj"), out = join(dir, "keep.json");
  await writeFile(out, "keep");
  for (const face of ["f 0 2 3", "f 1 2 9", "f 1/3 2/3 3/3", "f 1 1 2", "f 1 2 3 1", "f -4 -2 -1",
    "f 1/ 2/ 3/", "v NaN 0 1", "l 1 2", "v 1e45 0 0",
    "v 1.00000001 0 0\nf 2 4 3"]) {
    await writeFile(path, vertices + face + "\n");
    const result = invoke("obj", path, "-o", out);
    assert.notEqual(result.status, 0, face);
    assert.match(result.stderr, /line 4|line 5/);
    assert.equal(await readFile(out, "utf8"), "keep");
  }
});

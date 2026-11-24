// Real browser checks for the Rust/WASM model example and forge:game DOM APIs.
// Requires npm ci in examples/model-lab, Rust's wasm32-unknown-unknown target,
// and the installed Playwright on NODE_PATH. Do not install test tools locally.
// FORGE_MODELS_BROWSER=webkit selects WebKit. Linux WebKit is not an iPhone.
import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { once } from "node:events";
import { mkdir, mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { pathToFileURL } from "node:url";
import { inflateRawSync } from "node:zlib";

const playwright = createRequire(import.meta.url)("playwright");
const engine = process.env.FORGE_MODELS_BROWSER ?? "chromium";
assert.ok(["chromium", "webkit"].includes(engine), `Unsupported browser: ${engine}`);
const root = resolve(import.meta.dirname, "../..");
const forge = resolve(process.argv[2] ?? join(root, "target/debug/forge"));
const example = join(root, "examples/model-lab");
const dist = join(example, "dist");
const artifacts = resolve(process.env.FORGE_BROWSER_ARTIFACTS ?? join(root, ".forge/browser-models", engine));
await mkdir(artifacts, { recursive: true });
const scratch = await mkdtemp(join(tmpdir(), "forge-model-browser-"));
const results = [];
let browser;

async function check(name, run) {
  const started = performance.now();
  try {
    const details = await run();
    results.push({ name, passed: true, milliseconds: Math.round(performance.now() - started), ...details });
    console.log(`${engine}: ${name} passed.`);
  } catch (error) {
    results.push({ name, passed: false, error: error.stack ?? String(error), browser: error.browser });
    console.error(`${engine}: ${name} failed.`, error);
  }
}

async function withPage(options, run) {
  const { allowOrigin, ...contextOptions } = options;
  const context = await browser.newContext({
    viewport: { width: 1100, height: 850 }, deviceScaleFactor: 1, acceptDownloads: true,
    serviceWorkers: "block", ...contextOptions,
  });
  const page = await context.newPage();
  page.setDefaultTimeout(15_000);
  const errors = [], requests = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("console", (message) => { if (message.type() === "error") errors.push(message.text()); });
  page.on("request", (request) => {
    const url = request.url();
    if (/^(blob|data):/i.test(url)) return;
    if (url.startsWith("file:") && request.isNavigationRequest() && request.frame() === page.mainFrame()) return;
    if (allowOrigin && new URL(url).origin === allowOrigin) return;
    requests.push(url);
  });
  // A request is a failure even when interception blocks it. Playwright WebKit's
  // offline emulation also rejects local file/blob reads (including FileReader),
  // so use interception plus the zero-request assertion there. Chromium also
  // uses offline emulation. Both engines are isolated from the network at startup.
  await context.route(/^https?:/i, (route) => allowOrigin && new URL(route.request().url()).origin === allowOrigin ? route.continue() : route.abort());
  if (engine !== "webkit" && !allowOrigin) await context.setOffline(true);
  try {
    return await run({ page, context, errors, requests });
  } catch (error) {
    error.browser = { errors, requests };
    await page.screenshot({ path: join(artifacts, `failure-${results.length + 1}.png`), fullPage: true }).catch(() => {});
    throw error;
  } finally {
    await context.close();
  }
}

async function ready(page, url) {
  await page.goto(url);
  await page.locator('#model-status[data-state="ready"]').waitFor({ timeout: 30_000 });
  await page.waitForFunction(() => {
    const canvas = document.querySelector("#model-canvas");
    return canvas?.dataset.wasm === "rust" && canvas.dataset.meshValid === "true" && Number(canvas.dataset.frames) >= 2;
  });
  const dimensions = await page.locator("#model-canvas").evaluate((canvas) => ({
    width: canvas.width, height: canvas.height, cssWidth: canvas.clientWidth, cssHeight: canvas.clientHeight,
    renderer: canvas.dataset.renderer,
  }));
  assert.ok(dimensions.width > 0 && dimensions.height > 0, "the renderer has a nonempty backing buffer");
  assert.ok(dimensions.cssWidth > 0 && dimensions.cssHeight > 0, "the canvas is visible");
  assert.ok(["webgl", "webgpu"].includes(dimensions.renderer), `unknown renderer ${dimensions.renderer}`);
  return dimensions;
}

async function pause(page) {
  const button = page.locator("#model-pause");
  if (await button.getAttribute("aria-pressed") !== "true") await button.click();
  assert.equal(await button.getAttribute("aria-pressed"), "true");
  await delay(150); // Let camera damping settle before comparing geometry.
}

async function canvasImage(page, filename) {
  const bytes = await page.locator("#model-canvas").screenshot({ path: join(artifacts, filename) });
  // Decode the browser's actual screenshot. Dimensions alone do not prove that
  // the Rust geometry was displayed rather than an empty canvas being created.
  const pixels = await page.evaluate(async (url) => {
    const image = new Image(); image.src = url; await image.decode();
    const canvas = document.createElement("canvas");
    canvas.width = image.width; canvas.height = image.height;
    const context = canvas.getContext("2d", { willReadFrequently: true });
    context.drawImage(image, 0, 0);
    const data = context.getImageData(0, 0, canvas.width, canvas.height).data;
    const colors = new Set();
    let min = 255, max = 0, colored = 0;
    for (let i = 0; i < data.length; i += 16) {
      const r = data[i], g = data[i + 1], b = data[i + 2];
      min = Math.min(min, r, g, b); max = Math.max(max, r, g, b);
      colors.add(`${r >> 3},${g >> 3},${b >> 3}`);
      if (Math.max(r, g, b) - Math.min(r, g, b) > 20) colored++;
    }
    return { colors: colors.size, range: max - min, colored, samples: data.length / 16 };
  }, `data:image/png;base64,${bytes.toString("base64")}`);
  assert.ok(pixels.colors > 20 && pixels.range > 40, `canvas appears blank or flat: ${JSON.stringify(pixels)}`);
  assert.ok(pixels.colored > pixels.samples * 0.005, "the model produces a visible colored surface");
  return { hash: createHash("sha256").update(bytes).digest("hex"), ...pixels };
}

async function generate(page, kind, seed, detail = 32) {
  const revision = Number(await page.locator("#model-canvas").getAttribute("data-model-revision"));
  await page.locator("#model-kind").selectOption(kind);
  if (kind === "rock" || kind === "terrain") await page.locator("#model-seed").fill(String(seed));
  await page.locator("#model-detail").selectOption(String(detail));
  await page.locator("#model-regenerate").click();
  await page.waitForFunction((previous) => {
    const canvas = document.querySelector("#model-canvas");
    return Number(canvas?.dataset.modelRevision) > previous && canvas?.dataset.meshValid === "true";
  }, revision);
  await page.locator('#model-status[data-state="ready"]').waitFor();
}

function inspectGlb(bytes) {
  assert.equal(bytes.toString("ascii", 0, 4), "glTF", "export is a real GLB");
  assert.equal(bytes.readUInt32LE(4), 2, "GLB version");
  assert.equal(bytes.readUInt32LE(8), bytes.length, "GLB declared byte length");
  let json, binary;
  for (let cursor = 12; cursor < bytes.length;) {
    const length = bytes.readUInt32LE(cursor), type = bytes.readUInt32LE(cursor + 4);
    assert.equal(length % 4, 0, "GLB chunk alignment");
    assert.ok(cursor + 8 + length <= bytes.length, "GLB chunk stays in the file");
    const chunk = bytes.subarray(cursor + 8, cursor + 8 + length);
    if (type === 0x4e4f534a) json = JSON.parse(chunk.toString("utf8"));
    if (type === 0x004e4942) binary = chunk;
    cursor += 8 + length;
  }
  assert.ok(json && binary, "GLB has JSON and binary chunks");
  assert.ok(json.buffers.every((buffer) => buffer.uri === undefined), "GLB buffers are embedded rather than external dependencies");
  const primitive = json.meshes[0].primitives[0];
  const positions = json.accessors[primitive.attributes.POSITION];
  const normals = json.accessors[primitive.attributes.NORMAL];
  const uvs = json.accessors[primitive.attributes.TEXCOORD_0];
  const indices = json.accessors[primitive.indices];
  assert.equal(positions.type, "VEC3"); assert.equal(positions.componentType, 5126);
  assert.equal(normals.type, "VEC3"); assert.equal(normals.componentType, 5126);
  assert.equal(uvs.type, "VEC2"); assert.equal(uvs.componentType, 5126);
  assert.equal(normals.count, positions.count);
  assert.equal(uvs.count, positions.count);
  assert.ok(positions.count > 3 && indices.count > 3 && indices.count % 3 === 0);
  const values = (accessor, components, componentBytes, read) => {
    const view = json.bufferViews[accessor.bufferView];
    const start = (view.byteOffset ?? 0) + (accessor.byteOffset ?? 0);
    const stride = view.byteStride ?? components * componentBytes;
    assert.equal(start % componentBytes, 0, "accessor offset respects component alignment");
    assert.ok(start + (accessor.count - 1) * stride + components * componentBytes <= binary.length);
    return Array.from({ length: accessor.count }, (_, row) =>
      Array.from({ length: components }, (_, column) => read.call(binary, start + row * stride + column * componentBytes)));
  };
  const points = values(positions, 3, 4, Buffer.prototype.readFloatLE);
  const directions = values(normals, 3, 4, Buffer.prototype.readFloatLE);
  const coordinates = values(uvs, 2, 4, Buffer.prototype.readFloatLE);
  assert.ok(points.every((point) => point.every(Number.isFinite)), "all exported positions are finite");
  assert.ok(directions.every((normal) => Math.abs(Math.hypot(...normal) - 1) < 0.002), "all exported normals have unit length");
  assert.ok(coordinates.every((uv) => uv.every((value) => Number.isFinite(value) && value >= 0 && value <= 1)), "UVs are finite normalized coordinates");
  assert.ok([5123, 5125].includes(indices.componentType), "indices use unsigned 16- or 32-bit integers");
  const indexBytes = indices.componentType === 5123 ? 2 : 4;
  const readIndex = indexBytes === 2 ? Buffer.prototype.readUInt16LE : Buffer.prototype.readUInt32LE;
  assert.ok(values(indices, 1, indexBytes, readIndex).every(([index]) => index < positions.count), "indices reference existing vertices");
  return { vertices: positions.count, triangles: indices.count / 3, geometryHash: createHash("sha256").update(JSON.stringify(points)).digest("hex") };
}

async function exportedModel(page, name) {
  const downloading = page.waitForEvent("download");
  await page.locator("#model-export").click();
  const download = await downloading;
  assert.match(download.suggestedFilename(), /\.glb$/i);
  assert.equal(await download.failure(), null);
  const bytes = await readFile(await download.path());
  const geometry = inspectGlb(bytes);
  const requireExample = createRequire(join(example, "package.json"));
  const { GLTFLoader } = await import(pathToFileURL(requireExample.resolve("three/addons/loaders/GLTFLoader.js")).href);
  const imported = await new GLTFLoader().parseAsync(Uint8Array.from(bytes).buffer, "");
  const meshes = [];
  imported.scene.traverse((object) => { if (object.isMesh) meshes.push(object); });
  assert.equal(meshes.length, 1, "the GLB reopens as one model with the actual Three GLTFLoader");
  assert.equal(meshes[0].geometry.getAttribute("position").count, geometry.vertices);
  assert.equal(meshes[0].geometry.index.count, geometry.triangles * 3);
  assert.equal(meshes[0].material.wireframe, false, "inspection wireframe is excluded from export");
  meshes[0].geometry.dispose(); meshes[0].material.dispose();
  await writeFile(join(artifacts, name), bytes);
  const count = async (selector) => Number((await page.locator(selector).textContent()).replace(/[^0-9]/g, ""));
  assert.equal(geometry.vertices, await count("#model-vertices"));
  assert.equal(geometry.triangles, await count("#model-triangles"));
  return geometry;
}

async function lifecycleCheck(page, selector) {
  // There is no portable browser command for OS tab visibility. Drive the real
  // listener with a synthetic visibility state; RAF and cancellation stay real.
  return page.evaluate(async (selector) => {
    const read = () => selector ? Number(document.querySelector(selector).dataset.frames) : window.__gameTest.frames;
    const original = Object.getOwnPropertyDescriptor(document, "hidden");
    let hidden = false;
    Object.defineProperty(document, "hidden", { configurable: true, get: () => hidden });
    try {
      hidden = true; document.dispatchEvent(new Event("visibilitychange"));
      const before = read();
      await new Promise((resolve) => setTimeout(resolve, 120));
      const whileHidden = read();
      hidden = false; document.dispatchEvent(new Event("visibilitychange"));
      await new Promise((resolve) => setTimeout(resolve, 120));
      return { before, whileHidden, resumed: read() };
    } finally {
      if (original) Object.defineProperty(document, "hidden", original);
      else delete document.hidden;
      document.dispatchEvent(new Event("visibilitychange"));
    }
  }, selector);
}

async function runtimeControls() {
  const source = await readFile(join(root, "src/runtime/js/game.js"), "utf8");
  return withPage({ hasTouch: true, deviceScaleFactor: 2 }, async ({ page, errors, requests }) => {
    await page.setContent('<button id="stick" style="width:120px;height:120px;touch-action:pan-x">Steer</button><button id="action">Boost</button><canvas id="input" tabindex="0" style="width:200px;height:100px"></canvas>');
    await page.evaluate(async (module) => {
      const game = await import(module);
      const stickElement = document.querySelector("#stick"), actionElement = document.querySelector("#action"), canvas = document.querySelector("#input");
      const state = window.__gameTest = {
        game, stick: game.joystick(stickElement, { radius: 40, deadZone: 0 }),
        action: game.actionButton(actionElement, { keys: ["Space"] }), keys: game.input(canvas), frames: 0,
      };
      state.fit = game.fit(canvas, undefined, { maxDpr: 1.5 });
      state.stop = game.loop({ render() { state.frames++; } });
    }, `data:text/javascript;base64,${Buffer.from(source).toString("base64")}`);
    await page.waitForFunction(() => window.__gameTest.frames >= 2);
    const pointers = await page.evaluate(() => {
      const { stick, action, keys } = window.__gameTest;
      const pad = document.querySelector("#stick"), boost = document.querySelector("#action"), canvas = document.querySelector("#input");
      const rect = pad.getBoundingClientRect();
      const fire = (target, type, pointerId, values = {}) => target.dispatchEvent(new PointerEvent(type, {
        bubbles: true, cancelable: true, pointerType: "touch", button: 0, pointerId,
        clientX: rect.left + rect.width / 2 + 40, clientY: rect.top + rect.height / 2, ...values,
      }));
      fire(pad, "pointerdown", 41); fire(boost, "pointerdown", 42);
      const simultaneous = { x: stick.x, active: stick.active, down: action.down };
      fire(pad, "pointermove", 42, { clientX: 0 }); fire(window, "pointerup", 99);
      const unrelated = { x: stick.x, active: stick.active, down: action.down };
      fire(boost, "pointerdown", 43); fire(boost, "pointerup", 42);
      const secondFingerHeld = action.down;
      fire(boost, "pointercancel", 43);
      const cancel = { active: stick.active, down: action.down, pressed: action.pressed };
      fire(pad, "lostpointercapture", 41);
      fire(canvas, "pointerdown", 51); fire(canvas, "pointerdown", 52);
      const inputCount = keys.pointers.size;
      fire(canvas, "pointercancel", 51);
      const remaining = [...keys.pointers.keys()];
      fire(window, "pointerup", 52);
      return { simultaneous, unrelated, secondFingerHeld, cancel, released: [stick.x, stick.y, stick.active], inputCount, remaining, inputReleased: keys.pointers.size };
    });
    assert.deepEqual(pointers.simultaneous, { x: 1, active: true, down: true });
    assert.deepEqual(pointers.unrelated, pointers.simultaneous, "unrelated fingers cannot steal/release another control");
    assert.equal(pointers.secondFingerHeld, true);
    assert.deepEqual(pointers.cancel, { active: true, down: false, pressed: false });
    assert.deepEqual(pointers.released, [0, 0, false]);
    assert.equal(pointers.inputCount, 2); assert.deepEqual(pointers.remaining, [52]); assert.equal(pointers.inputReleased, 0);

    // Native mouse input checks actual pointer capture and release outside the
    // element, independently of the synthetic multi-finger state-machine checks.
    const box = await page.locator("#stick").boundingBox();
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.down();
    await page.mouse.move(box.x + box.width + 50, box.y + box.height / 2);
    assert.equal(await page.evaluate(() => window.__gameTest.stick.active), true);
    await page.mouse.up();
    assert.equal(await page.evaluate(() => window.__gameTest.stick.active), false);
    await page.locator("#action").focus(); await page.keyboard.down("Space");
    assert.equal(await page.evaluate(() => window.__gameTest.action.down), true);
    await page.keyboard.up("Space");
    assert.equal(await page.evaluate(() => window.__gameTest.action.down), false);

    const visibility = await lifecycleCheck(page, null);
    assert.equal(visibility.whileHidden, visibility.before, "hidden pages stop the loop");
    assert.ok(visibility.resumed > visibility.whileHidden, "visible pages resume rendering");
    await page.locator("#input").evaluate((canvas) => { canvas.style.width = "250px"; });
    await page.waitForFunction(() => document.querySelector("#input").width === 375);
    assert.equal(await page.locator("#input").evaluate((canvas) => canvas.height), 150, "DPR cap applies to both dimensions");

    const fallback = await page.evaluate(async () => {
      const events = [], game = window.__gameTest.game;
      let gl;
      const result = await game.createRenderer({
        prefer: "webgpu",
        webgpu: () => ({ init() { throw new Error("deliberate adapter initialization failure"); }, dispose() { events.push("disposed"); } }),
        webgl: () => ({ init() {
          events.push("webgl"); gl = document.createElement("canvas").getContext("webgl2");
          if (!gl) throw new Error("A real WebGL2 context is required for this test");
          gl.clearColor(0.2, 0.4, 0.6, 1); gl.clear(gl.COLOR_BUFFER_BIT);
        } }),
      });
      const pixel = new Uint8Array(4);
      gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, pixel);
      gl.getExtension("WEBGL_lose_context")?.loseContext();
      return { events, backend: result.backend, reason: result.fallbackReason, pixel: [...pixel] };
    });
    assert.deepEqual(fallback.events, ["disposed", "webgl"]);
    assert.equal(fallback.backend, "webgl"); assert.match(fallback.reason, /adapter initialization failure/);
    assert.ok(Math.abs(fallback.pixel[0] - 51) <= 1 && Math.abs(fallback.pixel[1] - 102) <= 1 && Math.abs(fallback.pixel[2] - 153) <= 1,
      `fallback must render actual WebGL pixels: ${fallback.pixel}`);
    const disposed = await page.evaluate(() => {
      const state = window.__gameTest;
      state.stop(); state.fit(); state.keys.dispose(); state.stick.dispose(); state.action.dispose();
      document.querySelector("#stick").dispatchEvent(new PointerEvent("pointerdown", { pointerId: 71, button: 0 }));
      return { active: state.stick.active, touchAction: document.querySelector("#stick").style.touchAction };
    });
    assert.deepEqual(disposed, { active: false, touchAction: "pan-x" });
    assert.deepEqual(errors, []); assert.deepEqual(requests, []);
    return { pointers, visibility, fallback };
  });
}

async function hostedModel() {
  execFileSync(forge, ["build", example, "--target", "server"], { stdio: "inherit" });
  const reservation = createServer();
  reservation.listen(0, "127.0.0.1");
  await once(reservation, "listening");
  const port = reservation.address().port;
  await new Promise((resolve, reject) => reservation.close((error) => error ? reject(error) : resolve()));
  const origin = `http://127.0.0.1:${port}`;
  const child = spawn(forge, ["start", example, "--host", "127.0.0.1", "--port", String(port)], { stdio: ["ignore", "pipe", "pipe"] });
  let output = "", spawnedError;
  child.stdout.on("data", (chunk) => { output += chunk; });
  child.stderr.on("data", (chunk) => { output += chunk; });
  const exited = once(child, "exit").catch((error) => { spawnedError = error; });
  try {
    let serving = false;
    for (let attempt = 0; attempt < 100 && child.exitCode === null && !spawnedError; attempt++) {
      const health = await fetch(`${origin}/health/live`, { signal: AbortSignal.timeout(250) }).catch(() => null);
      if (health?.ok) { await health.body?.cancel(); serving = true; break; }
      await delay(100);
    }
    assert.ok(serving, `production server must start: ${spawnedError ?? output}`);
    return await withPage({ allowOrigin: origin }, async ({ page, errors, requests }) => {
      const wasmResponses = [];
      let csp;
      page.on("response", (response) => {
        if (response.url() === `${origin}/`) csp = response.headers()["content-security-policy"];
        if (/\.wasm(?:[?#]|$)/i.test(response.url())) wasmResponses.push({
          url: response.url(), status: response.status(), contentType: response.headers()["content-type"],
        });
      });
      const dimensions = await ready(page, `${origin}/`);
      assert.ok(csp?.includes("'wasm-unsafe-eval'"), "default CSP explicitly permits WASM compilation");
      assert.ok(wasmResponses.length >= 1, "hosted loader fetched a separate compiled WASM asset");
      assert.ok(wasmResponses.every((response) => response.status === 200 && response.contentType?.startsWith("application/wasm") && new URL(response.url).origin === origin),
        `compiled WASM is served locally with the correct MIME type: ${JSON.stringify(wasmResponses)}`);
      await pause(page);
      const pixels = await canvasImage(page, "hosted-model.png");
      assert.deepEqual(errors, [], "hosted startup has no runtime or CSP errors");
      assert.deepEqual(requests, [], "hosted model has no external requests");
      return { dimensions, pixels, wasmResponses, csp, externalRequests: requests.length };
    });
  } finally {
    child.kill();
    await exited;
  }
}

async function desktopModel(url) {
  return withPage({}, async ({ page, errors, requests }) => {
    const dimensions = await ready(page, url);
    await pause(page);
    const initial = await canvasImage(page, "desktop-torus.png");
    await page.screenshot({ path: join(artifacts, "desktop-page.png"), fullPage: true });
    const geometries = {};
    for (const kind of ["sphere", "torus", "rock", "terrain"]) {
      await generate(page, kind, 17);
      geometries[kind] = await exportedModel(page, `${kind}.glb`);
    }
    const terrain = await canvasImage(page, "desktop-terrain.png");
    assert.notEqual(initial.hash, terrain.hash, "changing model kind changes rendered pixels");
    await generate(page, "rock", 100);
    const first = await exportedModel(page, "rock-seed-100.glb");
    await generate(page, "rock", 101);
    const second = await exportedModel(page, "rock-seed-101.glb");
    assert.notEqual(first.geometryHash, second.geometryHash, "seed changes real exported geometry");
    await generate(page, "rock", 100);
    assert.equal((await exportedModel(page, "rock-repeat.glb")).geometryHash, first.geometryHash, "same parameters reproduce the same Rust geometry");
    await generate(page, "sphere", 17, 48);
    assert.ok((await exportedModel(page, "sphere-detail-48.glb")).vertices > geometries.sphere.vertices, "higher detail increases geometry density");
    // An empty seed is invalid for rock, but must not remain hidden in a disabled
    // input and prevent a later unseeded sphere from being generated.
    await page.locator("#model-kind").selectOption("rock");
    await page.locator("#model-seed").fill("");
    const revision = Number(await page.locator("#model-canvas").getAttribute("data-model-revision"));
    await page.locator("#model-kind").selectOption("sphere");
    await page.locator("#model-regenerate").click();
    await page.waitForFunction((previous) => Number(document.querySelector("#model-canvas").dataset.modelRevision) > previous, revision);
    assert.ok((await exportedModel(page, "sphere-after-empty-seed.glb")).vertices > 3, "unseeded generation recovers from an empty seed");
    const solid = await canvasImage(page, "desktop-solid.png");
    await page.locator("#model-wireframe").check();
    const wireframe = await canvasImage(page, "desktop-wireframe.png");
    assert.notEqual(solid.hash, wireframe.hash, "wireframe changes the rendered surface");
    const visibility = await lifecycleCheck(page, "#model-canvas");
    assert.equal(visibility.whileHidden, visibility.before); assert.ok(visibility.resumed > visibility.whileHidden);
    await page.setViewportSize({ width: 760, height: 650 });
    await page.waitForFunction((old) => document.querySelector("#model-canvas").clientWidth !== old, dimensions.cssWidth);
    assert.ok(await page.locator("#model-canvas").evaluate((canvas) => canvas.width >= canvas.clientWidth * 0.7 && canvas.height > 0));
    assert.deepEqual(errors, [], "normal model editing must not emit runtime errors");
    // Navigate in the same event turn as GLB export. The old view must dispose
    // safely while the replacement view starts normally.
    await Promise.all([
      page.waitForEvent("load"),
      page.evaluate(() => { document.querySelector("#model-export").click(); location.reload(); }),
    ]);
    await page.locator('#model-status[data-state="ready"]').waitFor();
    assert.deepEqual(errors, [], "navigation during export must not emit uncaught errors");
    assert.deepEqual(requests, [], "offline HTML must make zero HTTP requests");
    return { dimensions, initial, terrain, geometries, visibility, networkRequests: requests.length };
  });
}

async function mobileModel(url) {
  return withPage({ viewport: { width: 320, height: 820 }, deviceScaleFactor: 3, hasTouch: true, isMobile: true }, async ({ page, errors, requests }) => {
    const forced = new URL(url); forced.searchParams.set("renderer", "webgl");
    const dimensions = await ready(page, forced.href);
    assert.equal(dimensions.renderer, "webgl", "explicit WebGL selection is honored");
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), "the UI fits a 320px phone viewport");
    assert.ok(dimensions.width <= Math.ceil(dimensions.cssWidth * 2) + 1, "mobile DPR is bounded");
    await pause(page);
    const before = await canvasImage(page, "mobile-before.png");
    await page.evaluate(() => scrollTo(0, 0));
    await page.screenshot({ path: join(artifacts, "mobile-viewport.png") });
    await page.screenshot({ path: join(artifacts, "mobile-page.png"), fullPage: true });
    await page.locator("#model-zoom-in").tap();
    await delay(350);
    const after = await canvasImage(page, "mobile-after-zoom.png");
    assert.notEqual(before.hash, after.hash, "touch zoom changes the rendered view");
    await page.locator("#model-reset").tap();
    await page.setViewportSize({ width: 820, height: 390 });
    await page.waitForFunction(() => document.querySelector("#model-canvas").clientWidth > 320);
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), "landscape layout does not overflow");
    await canvasImage(page, "mobile-landscape.png");
    assert.deepEqual(errors, []); assert.deepEqual(requests, []);
    return { dimensions, before, after, networkRequests: requests.length };
  });
}

function corruptWasm(html) {
  const script = /(<script\b[^>]*\bid="__forge_app"[^>]*>)([\s\S]*?)(<\/script>)/.exec(html);
  assert.ok(script, "single HTML exposes its application script");
  let source = script[2];
  if (source.includes('DecompressionStream("deflate-raw")')) {
    const encoded = /^const s="([^"]*)",I=\[0,10,13,34,92,60\],b=new Uint8Array\((\d+)\)/.exec(source);
    assert.ok(encoded, "recognize the base122 application payload");
    const bytes = new Uint8Array(Number(encoded[2])), illegal = [0, 10, 13, 34, 92, 60];
    let accumulator = 0, bits = 0, cursor = 0;
    const put = (group) => { accumulator = (accumulator << 7 | group) & 16383; if ((bits += 7) > 7) bytes[cursor++] = accumulator >> (bits -= 8); };
    for (const character of encoded[1]) {
      const code = character.charCodeAt(0);
      if (code > 127) { put(illegal[(code >> 7) - 1]); put(code & 127); } else put(code);
    }
    source = inflateRawSync(bytes).toString("utf8");
  }
  let changed = 0;
  source = source.replace(/\bAGFzbQE[A-Za-z0-9+/]{100,}={0,2}/g, (encoded) => {
    const bytes = Buffer.from(encoded, "base64");
    assert.equal(WebAssembly.validate(bytes), true, "the original payload is genuine valid WASM");
    bytes[0] = 1;
    assert.equal(WebAssembly.validate(bytes), false, "the negative fixture has invalid WASM magic");
    changed++;
    return bytes.toString("base64");
  });
  assert.equal(changed, 1, "corrupt exactly the embedded mesh module, with no module mocks");
  source = source.replace(/<\/script/gi, "<\\/script").replace(/<!--/g, "<\\!--");
  return html.slice(0, script.index) + script[1] + source + script[3] + html.slice(script.index + script[0].length);
}

async function failedInitialization(html) {
  const path = join(scratch, "invalid-mesh.html");
  await writeFile(path, corruptWasm(html));
  return withPage({}, async ({ page, errors, requests }) => {
    await page.goto(pathToFileURL(path).href);
    const status = page.locator('#model-status[data-state="error"]');
    await status.waitFor();
    const position = await status.boundingBox();
    assert.ok(position && position.y >= 0 && position.y < page.viewportSize().height, "startup error is visible without scrolling");
    const message = await status.textContent();
    assert.ok(message.trim().length > 10, "failed initialization is explained in the interface");
    assert.equal(await page.locator("#model-regenerate").isDisabled(), true);
    assert.equal(await page.locator("#model-export").isDisabled(), true);
    assert.deepEqual(errors, [], "invalid WASM is handled instead of becoming an uncaught rejection");
    assert.deepEqual(requests, []);
    await page.screenshot({ path: join(artifacts, "initialization-error.png"), fullPage: true });
    return { message, networkRequests: requests.length };
  });
}

try {
  const options = engine === "chromium" ? {
    args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader"],
    ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}),
  } : {};
  browser = await playwright[engine].launch(options);
  await check("browser input, visibility, resize, and actual WebGL fallback", runtimeControls);
  await check("hosted Rust WASM asset, default CSP, and rendered pixels", hostedModel);
  execFileSync(forge, ["build", example, "--target", "single-html"], { stdio: "inherit" });
  assert.deepEqual(await readdir(dist), ["index.html"], "the artifact consists of one HTML file");
  const html = await readFile(join(dist, "index.html"), "utf8");
  const url = pathToFileURL(join(dist, "index.html")).href;
  await check("offline Rust models, parameter changes, GLB export, and rendered pixels", () => desktopModel(url));
  await check("mobile WebGL, touch controls, DPR bounds, and orientation changes", () => mobileModel(url));
  await check("corrupt WASM produces a visible initialization error", () => failedInitialization(html));
} catch (error) {
  results.push({ name: "build and browser startup", passed: false, error: error.stack ?? String(error) });
  console.error(error);
} finally {
  await browser?.close();
  await rm(scratch, { recursive: true, force: true });
  await writeFile(join(artifacts, "results.json"), JSON.stringify({ engine, results }, null, 2) + "\n");
}
if (!results.length || results.some((result) => !result.passed)) process.exitCode = 1;

// Browser smoke tests for every example and target.
// Usage: node smoke.mjs <path to forge binary>   (examples need `npm install` first)
import { spawn, execFileSync } from "node:child_process";
import { createServer } from "node:http";
import { readFile, mkdtemp } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, extname, resolve } from "node:path";
import { createRequire } from "node:module";

// require() honours NODE_PATH, so a globally installed Playwright works too.
const { chromium } = createRequire(import.meta.url)("playwright");

const forge = resolve(process.argv[2] ?? "../../target/release/forge");
const examples = resolve(import.meta.dirname, "../../examples");
const failures = [];
const children = [];
let port = 4300 + Math.floor(Math.random() * 500);

function check(name, ok, detail = "") {
  console.log(`${ok ? "✓" : "✗"} ${name}${ok ? "" : `  ${detail}`}`);
  if (!ok) failures.push(name);
}

function build(example, target) {
  execFileSync(forge, ["build", join(examples, example), "--target", target], { stdio: "pipe" });
}

async function start(example) {
  const p = ++port;
  const child = spawn(forge, ["start", join(examples, example), "--port", String(p)], { stdio: "ignore" });
  children.push(child);
  for (let i = 0; i < 100; i++) {
    try {
      if ((await fetch(`http://127.0.0.1:${p}/health/live`)).ok) return `http://127.0.0.1:${p}`;
    } catch {}
    await new Promise((r) => setTimeout(r, 100));
  }
  throw new Error(`${example} did not start`);
}

// A plain static file server, to prove the static target needs nothing else.
function serveDir(dir) {
  const p = ++port;
  const types = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".json": "application/json" };
  const server = createServer(async (req, res) => {
    let path = decodeURIComponent(new URL(req.url, "http://x").pathname);
    if (path.endsWith("/")) path += "index.html";
    try {
      const body = await readFile(join(dir, path));
      res.writeHead(200, { "content-type": types[extname(path)] ?? "application/octet-stream" }).end(body);
    } catch {
      res.writeHead(404).end(await readFile(join(dir, "404.html")).catch(() => "not found"));
    }
  }).listen(p);
  children.push({ kill: () => server.close() });
  return `http://127.0.0.1:${p}`;
}

async function page(browser, options) {
  const p = await browser.newPage(options);
  p.errors = [];
  p.on("pageerror", (e) => p.errors.push(e.message));
  p.requests = [];
  p.on("request", (r) => p.requests.push(r.url()));
  // Hydration mismatches are recovered from (and reported as warnings), but are still bugs.
  const bad = (m) => (m.type() === "error" && !m.text().includes("404")) || m.text().startsWith("[forge] hydration");
  p.on("console", (m) => bad(m) && p.errors.push(m.text()));
  return p;
}

const browser = await chromium.launch({ args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader"] });
try {
  // React, server target: hydration, prefetch, navigation, streaming.
  build("react", "server");
  let base = await start("react");
  let p = await page(browser);
  const requests = [];
  p.on("request", (r) => requests.push(r.url()));
  await p.goto(base + "/");
  await p.click("#inc");
  check("react: hydrates and handles clicks", (await p.textContent("#inc")) === "Clicked 1 times");
  await p.click('header a[href="/blog"]');
  await p.waitForSelector("h1:text('Blog')");
  const n = requests.length;
  await p.hover('a[href="/blog/hello"]');
  await p.waitForTimeout(200);
  check("react: Link prefetches route data on hover", requests.slice(n).some((u) => u.includes("/_forge/data?path=%2Fblog%2Fhello")));
  await p.click('a[href="/blog/hello"]');
  await p.waitForSelector("h1:text('Hello, Forge')");
  check("react: client navigation updates the title", (await p.title()) === "Hello, Forge · Blog");
  await p.goBack();
  await p.waitForSelector("h1:text('Blog')");
  await p.goto(base + "/slow");
  await p.waitForSelector("#slow");
  check("react: streamed Suspense boundary resolves", (await p.$$("#fallback")).length === 0);
  check("react: no console errors", p.errors.length === 0, JSON.stringify(p.errors));

  // React, static target under a plain file server.
  build("react", "static");
  base = serveDir(join(examples, "react/dist"));
  p = await page(browser);
  await p.goto(base + "/");
  await p.click('header a[href="/blog"]');
  await p.click('a[href="/blog/streaming"]');
  await p.waitForSelector("h1:text('Streaming SSR')");
  check("react static: navigates with prebuilt data", p.url().endsWith("/blog/streaming"));
  check("react static: no console errors", p.errors.length === 0, JSON.stringify(p.errors));

  // React, single file from file://. "auto" compression packs it (deflate + base-122).
  build("react", "single-html");
  const html = await readFile(join(examples, "react/dist/index.html"), "utf8");
  check("react single-html: compressed automatically", html.includes("DecompressionStream") && html.length < 120_000, String(html.length));
  p = await page(browser);
  const fileRequests = [];
  p.on("request", (r) => fileRequests.push(r.url()));
  const file = "file://" + join(examples, "react/dist/index.html");
  await p.goto(file);
  await p.click("#inc");
  await p.click('header a[href$="/blog"]');
  await p.click('a[href$="/blog/hello"]');
  await p.waitForSelector("h1:text('Hello, Forge')");
  check("react single-html: hash navigation with embedded data", p.url().endsWith("#/blog/hello"));
  await p.goto(file + "#/blog/single-file");
  await p.waitForSelector("h1:text('One HTML file')");
  check("react single-html: no network requests", fileRequests.every((u) => /^(file|blob|data):/.test(u)), fileRequests.join(","));
  check("react single-html: no console errors", p.errors.length === 0, JSON.stringify(p.errors));

  // JSX clicker: persistent state across reloads.
  build("jsx", "single-html");
  p = await page(browser);
  await p.goto("file://" + join(examples, "jsx/dist/index.html"));
  for (let i = 0; i < 3; i++) await p.click("#gain");
  await p.reload();
  await p.waitForTimeout(100);
  check("jsx: score survives reload", (await p.textContent("#score")) === "3");

  // three.js game: compressed single file renders with WebGL.
  build("three-game", "single-html");
  p = await page(browser, { viewport: { width: 800, height: 600 } });
  await p.goto("file://" + join(examples, "three-game/dist/index.html"));
  await p.waitForSelector("canvas");
  await p.waitForTimeout(400);
  const size = await p.evaluate(() => [document.querySelector("canvas").width, document.querySelector("canvas").height]);
  check("three: canvas sized to viewport", size[0] === 800 && size[1] === 600, JSON.stringify(size));
  check("three: no console errors", p.errors.length === 0, JSON.stringify(p.errors));

  // Vanilla: mount/cleanup and navigation without a framework.
  build("vanilla", "server");
  base = await start("vanilla");
  p = await page(browser);
  await p.goto(base + "/");
  await p.click("#inc");
  await p.click('a[href="/notes/rust"]');
  await p.waitForSelector("h1:text('Why Rust')");
  await p.click('a[href="/"]');
  await p.waitForSelector("#count");
  check("vanilla: state persisted through navigation", (await p.textContent("#count")) === "1");

  // AI chat: streaming UI against the offline echo.
  build("ai-chat", "server");
  base = await start("ai-chat");
  p = await page(browser);
  await p.goto(base + "/");
  await p.fill("input", "ping");
  await p.click("button");
  await p.waitForSelector(".msg.assistant:not(#streaming)", { timeout: 10000 });
  check("ai: streamed reply rendered", (await p.textContent(".msg.assistant")).includes("You said: ping"));

  // Arena: live lobby socket, a socket room, training in a worker, deploying, offline file.
  build("arena", "server");
  base = await start("arena");
  p = await page(browser, { viewport: { width: 900, height: 560 } });
  await p.goto(base + "/");
  await p.waitForSelector(".stats .pill.open", { timeout: 10000 });
  check("arena: lobby socket is live", true);
  await p.goto(base + "/play/smoke");
  await p.waitForFunction(() => document.querySelectorAll("#scores li").length === 4, null, { timeout: 20000 });
  check("arena: joined a room with three bots", true);
  await p.goto(base + "/lab");
  await p.click("#train");
  await p.waitForFunction(() => Number(document.querySelector("#generation")?.textContent) >= 2, null, { timeout: 60000 });
  await p.click("#train");
  await p.click("text=Deploy to every room");
  await p.waitForSelector("text=Rooms switch within 2 seconds", { timeout: 10000 });
  check("arena: lab trains in a Web Worker and deploys the brain", true);
  check("arena: no console errors", p.errors.length === 0, JSON.stringify(p.errors));
  build("arena", "single-html");
  p = await page(browser, { viewport: { width: 900, height: 560 } });
  await p.goto("file://" + join(examples, "arena/dist/index.html") + "#/play/solo");
  await p.waitForFunction(() => document.querySelectorAll("#scores li").length === 5, null, { timeout: 20000 });
  check("arena single-html: offline match against four bots", (await p.textContent(".pill")) === "offline vs bots");

  // Pixel Pad: a single file that saves a copy of itself with the art embedded (forge:file).
  build("pixel", "single-html");
  const pixelFile = join(examples, "pixel/dist/index.html");
  const saves = await mkdtemp(join(tmpdir(), "forge-pixel-"));
  const openPixel = async (path) => {
    const q = await page(browser, { viewport: { width: 700, height: 900 } });
    await q.goto("file://" + path);
    await q.waitForSelector("#canvas");
    await q.waitForTimeout(150);
    return q;
  };
  const pixels = (q) => q.textContent("#count");
  p = await openPixel(pixelFile);
  check("pixel: starts with the starter sprite", (await p.textContent("#status")) === "new" && (await pixels(p)) !== "0 px");
  await p.click("text=clear");
  await p.click('[data-color="5"]');
  const box = await p.locator("#canvas").boundingBox();
  const cell = box.width / 32;
  await p.mouse.move(box.x + 3.5 * cell, box.y + 3.5 * cell);
  await p.mouse.down();
  for (let x = 4; x <= 10; x++) await p.mouse.move(box.x + (x + 0.5) * cell, box.y + 3.5 * cell);
  await p.mouse.up();
  check("pixel: paints a stroke and marks it unsaved", (await pixels(p)) === "8 px" && (await p.textContent("#status")) === "unsaved changes");
  const [saved] = await Promise.all([p.waitForEvent("download"), p.keyboard.press("Control+s")]);
  const copy = join(saves, "copy.html");
  await saved.saveAs(copy);
  check("pixel: Ctrl+S saves a copy named after the file", saved.suggestedFilename() === "index.html", saved.suggestedFilename());
  check("pixel: saving clears the unsaved flag", (await p.textContent("#status")) !== "unsaved changes");
  check("pixel: no network requests, no errors", p.requests.every((u) => /^(file|blob|data):/.test(u)) && !p.errors.length, JSON.stringify([p.requests, p.errors]));
  p = await openPixel(copy);
  check("pixel: the copy opens with the saved art", (await p.textContent("#status")) === "opened with saved art" && (await pixels(p)) === "8 px");
  const [again] = await Promise.all([p.waitForEvent("download"), p.click("#save")]);
  await again.saveAs(join(saves, "again.html"));
  const [a, b, original] = await Promise.all([readFile(copy), readFile(join(saves, "again.html")), readFile(pixelFile)]);
  check("pixel: re-saving unchanged is byte-identical", a.equals(b), `${a.length} vs ${b.length}`);
  check("pixel: the copy is the original plus the state", a.length > original.length && a.length - original.length < 4000, `${original.length} -> ${a.length}`);
  await p.evaluate(async () => {
    const c = Object.assign(document.createElement("canvas"), { width: 2, height: 2 });
    const g = c.getContext("2d");
    g.fillStyle = "#b13e53";
    g.fillRect(0, 0, 2, 2);
    const dt = new DataTransfer();
    dt.items.add(new File([await new Promise((r) => c.toBlob(r))], "red.png", { type: "image/png" }));
    for (const type of ["dragenter", "dragover", "drop"]) dispatchEvent(new DragEvent(type, { dataTransfer: dt, bubbles: true, cancelable: true }));
  });
  await p.waitForFunction(() => document.querySelector("#count").textContent === "1024 px", null, { timeout: 5000 }).catch(() => {});
  check("pixel: a dropped image is imported onto the palette", (await pixels(p)) === "1024 px", await pixels(p));
  check("pixel: the copy has no errors (no hydration over stale markup)", !p.errors.length, JSON.stringify(p.errors));
} catch (e) {
  failures.push(String(e?.stack ?? e));
  console.error(e);
} finally {
  await browser.close();
  for (const c of children) c.kill();
}
console.log(failures.length ? `\n${failures.length} failed` : "\nall browser checks passed");
process.exit(failures.length ? 1 : 0);

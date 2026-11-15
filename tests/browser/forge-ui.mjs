// Run after npm ci in examples/ui-showcase and npm install in tests/browser.
// node tests/browser/forge-ui.mjs [path to forge binary]
// FORGE_UI_BROWSER=webkit selects WebKit; CHROMIUM_PATH can select a system Chromium.
import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { once } from "node:events";
import { readFile, readdir } from "node:fs/promises";
import { createServer } from "node:http";
import { createRequire } from "node:module";
import { extname, join, resolve, sep } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { pathToFileURL } from "node:url";

const require = createRequire(import.meta.url);
const playwright = require("playwright");
const engine = process.env.FORGE_UI_BROWSER ?? "chromium";
assert.ok(["chromium", "webkit"].includes(engine), `Unsupported browser engine: ${engine}`);
const forge = resolve(process.argv[2] ?? join(import.meta.dirname, "../../target/debug/forge"));
const showcase = resolve(import.meta.dirname, "../../examples/ui-showcase");
const dist = join(showcase, "dist");
const index = join(dist, "index.html");
const heading = "Release overview";

let axePath;
try {
  axePath = require.resolve("axe-core/axe.min.js");
} catch (error) {
  if (error.code !== "MODULE_NOT_FOUND" || process.env.FORGE_UI_REQUIRE_AXE === "1") throw error;
  console.log("axe-core unavailable; running interaction and semantic accessibility checks.");
}

async function listen(server) {
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  return server.address().port;
}

function close(server) {
  return new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
}

async function withForgeServer(check) {
  const reservation = createServer();
  const port = await listen(reservation);
  await close(reservation);
  const base = `http://127.0.0.1:${port}`;
  const child = spawn(forge, ["start", showcase, "--host", "127.0.0.1", "--port", String(port)], {
    stdio: ["ignore", "pipe", "pipe"],
  });
  const exited = once(child, "exit");
  let output = "";
  child.stdout.on("data", (chunk) => { output += chunk; });
  child.stderr.on("data", (chunk) => { output += chunk; });
  try {
    for (let attempt = 0; attempt < 100 && child.exitCode === null; attempt++) {
      const ready = await fetch(`${base}/health/live`, { signal: AbortSignal.timeout(250) }).catch(() => null);
      if (ready?.ok) {
        await ready.body?.cancel();
        await check(`${base}/`);
        return;
      }
      await delay(100);
    }
    throw new Error(`Forge server failed to start.\n${output}`);
  } finally {
    child.kill();
    await exited;
  }
}

async function withStaticServer(check) {
  const types = { ".html": "text/html; charset=utf-8", ".js": "text/javascript", ".css": "text/css", ".json": "application/json" };
  const server = createServer(async (request, response) => {
    try {
      const path = decodeURIComponent(new URL(request.url, "http://127.0.0.1").pathname);
      const file = resolve(dist, `.${extname(path) ? path : join(path, "index.html")}`);
      if (!file.startsWith(`${dist}${sep}`)) throw new Error("Path is outside dist");
      const body = await readFile(file);
      response.writeHead(200, { "content-type": types[extname(file)] ?? "application/octet-stream" }).end(body);
    } catch {
      response.writeHead(404).end("Not found");
    }
  });
  const port = await listen(server);
  try {
    await check(`http://127.0.0.1:${port}/`);
  } finally {
    await close(server);
  }
}

async function noJavaScript(browser, url, colorScheme) {
  const context = await browser.newContext({ javaScriptEnabled: false, colorScheme });
  try {
    const page = await context.newPage();
    await page.goto(url);
    const main = page.getByRole("main");
    await main.getByRole("heading", { level: 1, name: heading, exact: true }).waitFor();
    await main.getByRole("tabpanel", { name: "Releases" }).waitFor();
    const scope = page.locator(".forge-ui");
    await scope.waitFor();
    assert.equal(await scope.evaluate((element) => getComputedStyle(element).colorScheme), colorScheme,
      "the theme stylesheet follows system preferences with JavaScript disabled");
    assert.equal(await page.getByRole("dialog").count(), 0, "closed dialog is absent from the accessible tree");
    if (url.startsWith("http") && colorScheme === "light") {
      await page.goto(new URL("guide", url).href);
      await page.getByRole("heading", { level: 1, name: "Component guide", exact: true }).waitFor();
    }
  } finally {
    await context.close();
  }
}

async function focusIs(locator, reason) {
  await locator.evaluate((element, label) => {
    if (element !== document.activeElement) throw new Error(`Expected focus on ${label}`);
  }, reason);
}

async function assertAxe(page, state, setup = false) {
  if (!axePath) return;
  if (setup) await page.addScriptTag({ path: axePath });
  const violations = await page.evaluate(async () => {
    const { violations } = await window.axe.run({
      runOnly: { type: "tag", values: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"] },
    });
    return violations.map(({ id, nodes }) => ({
      id, nodes: nodes.map(({ target, failureSummary }) => ({ target, failureSummary })),
    }));
  });
  assert.deepEqual(violations, [], `${state} has WCAG violations`);
}

async function checkTabs(page) {
  const list = page.getByRole("tablist", { name: "Release workspace" });
  const releases = list.getByRole("tab", { name: "Releases" });
  const activity = list.getByRole("tab", { name: "Activity" });
  const activityPanel = page.getByRole("tabpanel", { name: "Activity" });
  await releases.focus();
  await releases.press("ArrowLeft");
  await activityPanel.getByRole("heading", { name: "Release activity" }).waitFor();
  await focusIs(activity, "Activity after a left-arrow wrap");
  assert.equal(await activity.getAttribute("aria-selected"), "true");
  assert.equal(await releases.getAttribute("tabindex"), "-1");
  assert.equal(await activity.getAttribute("aria-controls"), await activityPanel.getAttribute("id"));
  assert.equal(await activityPanel.getAttribute("aria-labelledby"), await activity.getAttribute("id"));
  assert.equal(await page.getByRole("tabpanel", { name: "Releases" }).count(), 0);
  await page.keyboard.press("Tab");
  await focusIs(activityPanel, "the active tab panel");
  await page.keyboard.press("Shift+Tab");
  await focusIs(activity, "the active tab after Shift+Tab");
  await activity.press("Home");
  await focusIs(releases, "the first tab after Home");
  await releases.press("End");
  await focusIs(activity, "the last tab after End");
  await activity.press("ArrowRight");
  await focusIs(releases, "Releases after a right-arrow wrap");
  assert.equal(await releases.getAttribute("aria-selected"), "true");
}

async function checkDialog(page, audit) {
  const trigger = page.getByRole("button", { name: "New release" });
  const dialog = page.getByRole("dialog", { name: "Schedule a release" });
  await trigger.press("Enter");
  await dialog.waitFor();
  assert.ok(await dialog.evaluate((element) => element.contains(document.activeElement)), "dialog takes focus on open");
  for (let step = 0; step < 12; step++) {
    await page.keyboard.press(step < 6 ? "Tab" : "Shift+Tab");
    assert.ok(await dialog.evaluate((element) => element.contains(document.activeElement)), "Tab must stay inside the modal");
  }
  if (audit) await assertAxe(page, "Dark modal");

  await page.keyboard.press("Escape");
  await dialog.waitFor({ state: "hidden" });
  await focusIs(trigger, "the New release trigger after Escape");

  await trigger.press("Enter");
  await dialog.getByRole("button", { name: "Close dialog" }).click();
  await dialog.waitFor({ state: "hidden" });
  await focusIs(trigger, "the New release trigger after Close dialog");

  await trigger.press("Enter");
  const name = dialog.getByRole("textbox", { name: "Release name" });
  const submit = dialog.getByRole("button", { name: "Schedule release" });
  await submit.click();
  const error = dialog.getByText("Enter a release name.", { exact: true });
  await error.waitFor();
  await focusIs(name, "the invalid release name field");
  assert.equal(await name.getAttribute("aria-invalid"), "true");
  const errorId = await error.getAttribute("id");
  assert.ok(errorId, "field error has an ID");
  assert.ok((await name.getAttribute("aria-describedby"))?.split(/\s+/).includes(errorId), "field is described by its error");

  await name.fill("Browser smoke");
  const env = dialog.getByRole("combobox", { name: "Environment" });
  assert.equal(await env.inputValue(), "staging");
  await env.selectOption("production");
  await dialog.getByRole("textbox", { name: "Release notes" }).fill("Browser coverage for release scheduling.");
  await dialog.getByRole("checkbox", { name: "Notify the release team" }).check();
  await submit.click();
  await dialog.waitFor({ state: "hidden" });
  await focusIs(trigger, "the New release trigger after scheduling");
  await page.getByRole("tabpanel", { name: "Releases" }).getByText("Browser smoke", { exact: true }).waitFor();
  const notificationLog = page.getByRole("log", { name: "Notifications" });
  assert.equal(await notificationLog.getAttribute("aria-live"), "polite");
  const notice = notificationLog.getByText("Release scheduled", { exact: true });
  await notice.waitFor();
  if (audit) await assertAxe(page, "Dark notification");
  await notificationLog.getByRole("button", { name: "Dismiss Release scheduled" }).click();
  await notice.waitFor({ state: "hidden" });
  await focusIs(trigger, "the New release trigger after dismissing the notification");

  await page.getByRole("tab", { name: "Activity" }).click();
  const activity = page.getByRole("tabpanel", { name: "Activity" }).getByRole("listitem")
    .filter({ hasText: "Browser smoke" });
  await activity.getByText("Scheduled · Production", { exact: true }).waitFor();
  await activity.getByText("Browser coverage for release scheduling.", { exact: true }).waitFor();
  await activity.getByText("Team notification requested", { exact: true }).waitFor();
}

async function checkNavigation(page, target, scope) {
  const navigation = page.getByRole("navigation", { name: "Workspace" });
  const guide = navigation.getByRole("link", { name: "Component guide" });
  const overview = navigation.getByRole("link", { name: "Overview" });
  assert.equal(await overview.getAttribute("aria-current"), "page");
  await guide.click();
  await page.getByRole("heading", { level: 1, name: "Component guide", exact: true }).waitFor();
  assert.equal(await guide.getAttribute("aria-current"), "page");
  assert.equal(await page.title(), "Component guide · Relay");
  assert.equal(await scope.getAttribute("data-theme"), "dark", "layout theme survives route changes");
  assert.ok(page.url().endsWith(target === "single-html" ? "#/guide" : "/guide"));
  await page.getByRole("link", { name: "Back to overview" }).click();
  await page.getByRole("heading", { level: 1, name: heading, exact: true }).waitFor();
  assert.equal(await overview.getAttribute("aria-current"), "page");
}

async function checkTarget(browser, target, url) {
  await noJavaScript(browser, url, "light");
  await noJavaScript(browser, url, "dark");
  const offline = target === "single-html";
  const context = await browser.newContext({ colorScheme: "light", viewport: { width: 320, height: 820 } });
  try {
    const page = await context.newPage();
    page.setDefaultTimeout(8_000);
    const errors = [];
    const network = [];
    page.on("pageerror", (err) => errors.push(err.message));
    page.on("console", (message) => {
      if (message.text().startsWith("[forge] hydration") ||
          (message.type() === "error" && !message.text().includes("404"))) errors.push(message.text());
    });
    if (offline) {
      await page.route(/^https?:/i, (route) => route.abort());
      page.on("request", (request) => {
        const resource = request.url();
        if (resource !== url && !/^(blob|data):/i.test(resource)) network.push(resource);
      });
    }

    await page.goto(url);
    await page.getByRole("heading", { level: 1, name: heading, exact: true }).waitFor();
    await page.evaluate(() => document.fonts.ready.then(() => null));
    const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
    assert.ok(overflow <= 1, `320px viewport overflows by ${overflow}px`);

    const theme = page.getByRole("combobox", { name: "Color theme" });
    const scope = page.locator(".forge-ui");
    const lightBackground = await scope.evaluate((element) => getComputedStyle(element).backgroundColor);
    if (offline) await assertAxe(page, "Light theme", true);
    // The standalone build decompresses its script asynchronously. A theme change
    // is a user-observable hydration check and can be attempted before React is ready.
    for (let attempt = 0; attempt < 30 && await scope.getAttribute("data-theme") !== "dark"; attempt++) {
      await theme.selectOption("dark");
      await delay(50);
    }
    assert.equal(await scope.getAttribute("data-theme"), "dark", "theme control was hydrated");
    assert.equal(await scope.evaluate((element) => getComputedStyle(element).colorScheme), "dark");
    assert.notEqual(await scope.evaluate((element) => getComputedStyle(element).backgroundColor), lightBackground);
    if (offline) await assertAxe(page, "Dark theme");
    await checkTabs(page);
    await checkDialog(page, offline);
    await checkNavigation(page, target, scope);
    await theme.selectOption("light");
    await page.locator('.forge-ui[data-theme="light"]').waitFor();
    assert.equal(await scope.evaluate((element) => getComputedStyle(element).colorScheme), "light");

    assert.deepEqual(errors, [], `${target} emitted page or hydration errors`);
    assert.deepEqual(network, [], `${target} requested network assets`);
    console.log(`${engine} ${target}: pre-JavaScript content, theme, reflow, tabs, dialog, form, and navigation passed${offline ? "; no network requests" : ""}.`);
  } finally {
    await context.close();
  }
}

const options = engine === "chromium" && process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {};
const browser = await playwright[engine].launch(options);
const failures = [];
try {
  const targets = {
    server: withForgeServer,
    static: withStaticServer,
    async "single-html"(check) {
      assert.deepEqual(await readdir(dist), ["index.html"], "single-html build emits exactly one file");
      await check(pathToFileURL(index).href);
    },
  };
  for (const [target, serve] of Object.entries(targets)) {
    try {
      execFileSync(forge, ["build", showcase, "--target", target], { stdio: "inherit" });
      await serve((url) => checkTarget(browser, target, url));
    } catch (error) {
      failures.push(error);
      console.error(`${engine} ${target} failed`, error);
    }
  }
} finally {
  await browser.close();
}
if (failures.length) process.exitCode = 1;
else if (axePath) console.log(`${engine}: axe-core WCAG checks passed for light, dark, modal, and notification states.`);

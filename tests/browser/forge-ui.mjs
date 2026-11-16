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
  const types = { ".html": "text/html; charset=utf-8", ".js": "text/javascript", ".css": "text/css", ".json": "application/json", ".svg": "image/svg+xml" };
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

// Text and background of an element as sRGB (via a canvas, so oklch() and color-mix()
// resolve the way they render), and their WCAG contrast ratio.
function contrastOf(locator) {
  return locator.evaluate((element) => {
    const context = Object.assign(document.createElement("canvas"), { width: 1, height: 1 })
      .getContext("2d", { willReadFrequently: true });
    const rgb = (color) => {
      context.clearRect(0, 0, 1, 1);
      context.fillStyle = color;
      context.fillRect(0, 0, 1, 1);
      return [...context.getImageData(0, 0, 1, 1).data].slice(0, 3);
    };
    const luminance = (channels) => {
      const [r, g, b] = channels.map((value) => {
        const v = value / 255;
        return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
      });
      return 0.2126 * r + 0.7152 * g + 0.0722 * b;
    };
    const style = getComputedStyle(element);
    const background = rgb(style.backgroundColor);
    const [a, b] = [luminance(rgb(style.color)), luminance(background)];
    return { ratio: (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05), background: background.join(",") };
  });
}

async function checkGallery(page, target, audit) {
  const navigation = page.getByRole("navigation", { name: "Workspace" });
  await navigation.getByRole("link", { name: "Component gallery" }).click();
  await page.getByRole("heading", { level: 1, name: "Component gallery", exact: true }).waitFor();
  assert.ok(page.url().endsWith(target === "single-html" ? "#/components" : "/components"));
  assert.equal(await page.title(), "Component gallery · Relay");
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
  assert.ok(overflow <= 1, `gallery overflows a 320px viewport by ${overflow}px`);

  // Theme engine: every accent, in both schemes, keeps AA contrast on solid and soft buttons.
  const playground = page.locator("#playground");
  const primary = page.locator("#primary");
  const soft = page.getByRole("button", { name: "Soft", exact: true });
  const scheme = page.getByRole("combobox", { name: "Color theme" });
  const accent = page.getByRole("combobox", { name: "Accent" });
  const palettes = new Set();
  for (const mode of ["dark", "light"]) {
    await scheme.selectOption(mode);
    await page.locator(`.relay[data-theme="${mode}"]`).waitFor();
    for (const name of ["violet", "ember", "cobalt", "verdigris", "gold", "rose", "steel"]) {
      await accent.selectOption(name);
      await page.locator(`#playground[data-accent="${name}"]`).waitFor();
      const solid = await contrastOf(primary);
      const tint = await contrastOf(soft);
      assert.ok(solid.ratio >= 4.5, `${mode} ${name}: primary button contrast is ${solid.ratio.toFixed(2)}`);
      assert.ok(tint.ratio >= 4.5, `${mode} ${name}: soft button contrast is ${tint.ratio.toFixed(2)}`);
      palettes.add(`${mode} ${solid.background}`);
    }
  }
  assert.equal(palettes.size, 14, "each accent and scheme has its own primary color");
  const preset = (await contrastOf(primary)).background;
  await page.getByRole("textbox", { name: "Custom accent" }).fill("#0a7f5a");
  await page.locator('#playground[data-accent="from"]').waitFor();
  const custom = await contrastOf(primary);
  assert.ok(custom.ratio >= 4.5, `custom accent contrast is ${custom.ratio.toFixed(2)}`);
  assert.notEqual(custom.background, preset, "a custom color changes the palette");
  const [r, g, b] = custom.background.split(",").map(Number);
  assert.ok(g > r && g > b, `a green source gives a green accent, got rgb(${custom.background})`);
  await page.getByRole("textbox", { name: "Custom accent" }).fill("");

  const corner = () => primary.evaluate((element) => parseFloat(getComputedStyle(element).borderTopLeftRadius));
  const height = () => primary.evaluate((element) => element.getBoundingClientRect().height);
  await page.getByRole("radio", { name: "none", exact: true }).check();
  assert.equal(await corner(), 0);
  await page.getByRole("radio", { name: "full", exact: true }).check();
  assert.ok(await corner() > 100, "radius=full rounds controls completely");
  await page.getByRole("radio", { name: "md", exact: true }).check();
  const comfortable = await height();
  await page.getByRole("radio", { name: "Compact", exact: true }).check();
  assert.ok(await height() < comfortable - 4, "compact density shrinks controls");
  await page.getByRole("radio", { name: "Spacious", exact: true }).check();
  assert.ok(await height() > comfortable + 4, "spacious density grows controls");
  await page.getByRole("radio", { name: "Comfortable", exact: true }).check();
  assert.equal(await playground.getAttribute("data-theme"), "inherit");

  // Menu: arrows skip disabled items, type-ahead, Escape returns focus, Enter runs the action.
  const menuButton = page.getByRole("button", { name: "Release", exact: true });
  const item = (name) => page.getByRole("menuitem", { name, exact: false });
  await menuButton.focus();
  await page.keyboard.press("ArrowDown");
  await focusIs(item("Rename"), "the first menu item");
  await page.keyboard.press("ArrowDown");
  await focusIs(item("Duplicate"), "the second menu item");
  await page.keyboard.press("ArrowDown");
  await focusIs(item("Delete"), "the item after the disabled one");
  await page.keyboard.press("ArrowDown");
  await focusIs(item("Rename"), "the first item after wrapping");
  await page.keyboard.press("d");
  await focusIs(item("Duplicate"), "the type-ahead match");
  await page.keyboard.press("End");
  await focusIs(item("Delete"), "the last item");
  await page.keyboard.press("Escape");
  await page.getByRole("menu").waitFor({ state: "detached" });
  await focusIs(menuButton, "the menu button after Escape");
  assert.equal(await menuButton.getAttribute("aria-expanded"), "false");
  await menuButton.click();
  await page.keyboard.press("Enter");
  const toasts = page.getByRole("log", { name: "Gallery notifications" });
  await toasts.getByText("Renamed", { exact: true }).waitFor();
  await page.getByRole("menu").waitFor({ state: "detached" });
  await menuButton.click();
  await page.getByRole("heading", { level: 1 }).click();
  await page.getByRole("menu").waitFor({ state: "detached" });

  // Tooltip, switch, slider (which also drives the HUD meter), loading button.
  const copy = page.getByRole("button", { name: "Copy link" });
  await copy.focus();
  const tooltip = page.getByRole("tooltip");
  await tooltip.waitFor();
  assert.equal(await copy.getAttribute("aria-describedby"), await tooltip.getAttribute("id"));
  await page.keyboard.press("Escape");
  await tooltip.waitFor({ state: "hidden" });
  const live = page.getByRole("switch", { name: "Live updates" });
  assert.equal(await live.isChecked(), true);
  await live.focus();
  await page.keyboard.press("Space");
  assert.equal(await live.isChecked(), false);
  const slider = page.getByRole("slider", { name: "Health" });
  const meter = page.getByRole("meter", { name: "Health" });
  await slider.focus();
  await page.keyboard.press("ArrowRight");
  assert.equal(await slider.inputValue(), "73");
  assert.equal(await meter.getAttribute("aria-valuenow"), "73");
  await page.getByRole("button", { name: "Take a hit" }).click();
  assert.equal(await meter.getAttribute("aria-valuenow"), "58", "HUD buttons receive clicks through the overlay");
  const save = page.getByRole("button", { name: "Save changes" });
  await save.click();
  assert.equal(await save.getAttribute("aria-busy"), "true");
  await toasts.getByText("Changes saved", { exact: true }).waitFor();
  assert.equal(await save.getAttribute("aria-busy"), null);

  // Table sorting (text, then numbers) and pagination.
  const table = page.getByRole("table", { name: "Services" });
  const firstCell = () => table.locator("tbody tr").first().locator("td").first().textContent();
  const lastCell = () => table.locator("tbody tr").last().locator("td").first().textContent();
  assert.equal(await firstCell(), "Gateway");
  await table.getByRole("button", { name: "Service" }).click();
  assert.equal(await table.getByRole("columnheader", { name: "Service" }).getAttribute("aria-sort"), "ascending");
  assert.equal(await firstCell(), "Auth");
  await table.getByRole("button", { name: "Service" }).click();
  assert.equal(await table.getByRole("columnheader", { name: "Service" }).getAttribute("aria-sort"), "descending");
  assert.equal(await firstCell(), "Search");
  await table.getByRole("button", { name: "p95 latency" }).click();
  assert.deepEqual([await firstCell(), await lastCell()], ["Auth", "Billing"], "numbers sort numerically");
  assert.equal(await table.getByRole("columnheader", { name: "Service" }).getAttribute("aria-sort"), "none");
  const pages = page.getByRole("navigation", { name: "Pagination" });
  assert.equal(await pages.getByRole("button", { name: "Previous page" }).isDisabled(), true);
  await pages.getByRole("button", { name: "Next page" }).click();
  assert.equal(await pages.getByRole("button", { name: "Page 2", exact: true }).getAttribute("aria-current"), "page");
  await page.getByText("Page 2 of 9", { exact: true }).waitFor();

  // Drawer, popover and accordion.
  const opener = page.getByRole("button", { name: "Open details" });
  await opener.click();
  const drawer = page.getByRole("dialog", { name: "Gateway" });
  await drawer.waitFor();
  await delay(250);
  const edge = await drawer.evaluate((element) => window.innerWidth - element.getBoundingClientRect().right);
  assert.ok(Math.abs(edge) <= 1, `the drawer sits on the right edge (off by ${edge}px)`);
  if (audit) await assertAxe(page, "Drawer");
  await page.keyboard.press("Escape");
  await drawer.waitFor({ state: "hidden" });
  await focusIs(opener, "the drawer's opener");
  await page.getByRole("button", { name: "Filters" }).click();
  const filterHeading = page.getByRole("heading", { name: "Filter services" });
  await filterHeading.waitFor();
  const box = await page.locator(".forge-ui-popover").boundingBox();
  assert.ok(box.x >= 0 && box.x + box.width <= 320 && box.y >= 0, "the popover stays inside the viewport");
  await page.keyboard.press("Escape");
  await filterHeading.waitFor({ state: "hidden" });
  const section = (title) => page.locator("details", { hasText: title });
  await section("How do I theme it?").locator("summary").click();
  assert.equal(await section("How do I theme it?").evaluate((element) => element.open), true);
  await section("Does it work offline?").locator("summary").click();
  assert.equal(await section("How do I theme it?").evaluate((element) => element.open), false, "accordion sections are exclusive");

  // Image: space reserved, loaded, visible.
  const image = page.getByRole("img", { name: "An anvil glowing in a dark forge" });
  await page.locator(".forge-ui-image[data-loaded]").waitFor();
  const shape = await image.evaluate((element) => {
    const frame = element.parentElement.getBoundingClientRect();
    return { natural: element.naturalWidth, ratio: frame.width / frame.height, opacity: getComputedStyle(element).opacity };
  });
  assert.ok(shape.natural > 0 && Math.abs(shape.ratio - 640 / 360) < 0.02, JSON.stringify(shape));

  if (audit) await assertAxe(page, "Component gallery");

  // Command palette: hotkey, filter, Enter navigates through the router.
  await page.keyboard.press("Control+k");
  const search = page.getByRole("combobox", { name: "Command palette" });
  await search.waitFor();
  await focusIs(search, "the palette's search field");
  const options = page.getByRole("listbox", { name: "Command palette" }).getByRole("option");
  assert.equal(await options.count(), 6);
  if (audit) await assertAxe(page, "Command palette");
  await page.keyboard.type("guide");
  assert.equal(await options.count(), 1, "typing filters the commands");
  assert.equal(await options.first().textContent(), "Component guide");
  assert.equal(await options.first().getAttribute("aria-selected"), "true");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await page.getByRole("heading", { level: 1, name: "Component guide", exact: true }).waitFor();
  assert.ok(page.url().endsWith(target === "single-html" ? "#/guide" : "/guide"));
  await search.waitFor({ state: "hidden" });
  await page.keyboard.press("Control+k");
  await search.waitFor();
  await page.keyboard.type("dark");
  await page.keyboard.press("Enter");
  await page.locator('.relay[data-theme="dark"]').waitFor();
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
    await checkGallery(page, target, offline);

    assert.deepEqual(errors, [], `${target} emitted page or hydration errors`);
    assert.deepEqual(network, [], `${target} requested network assets`);
    console.log(`${engine} ${target}: pre-JavaScript content, theme, reflow, tabs, dialog, form, and navigation, and the component gallery passed${offline ? "; no network requests" : ""}.`);
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

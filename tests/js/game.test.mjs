import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { afterEach, beforeEach, test } from "node:test";

// Execute the shipped browser module, with a deterministic DOM/animation clock.
// These test hosts implement EventTarget semantics; no module is intercepted.
const source = await readFile(new URL("../../src/runtime/js/game.js", import.meta.url), "utf8");
const game = await import(`data:text/javascript;base64,${Buffer.from(source).toString("base64")}`);

class Surface extends EventTarget {
  listeners = new Map();
  addEventListener(type, listener, options) {
    if (!this.listeners.has(type)) this.listeners.set(type, new Set());
    this.listeners.get(type).add(listener);
    super.addEventListener(type, listener, options);
  }
  removeEventListener(type, listener, options) {
    this.listeners.get(type)?.delete(listener);
    super.removeEventListener(type, listener, options);
  }
  get listenerCount() {
    return [...this.listeners.values()].reduce((n, listeners) => n + listeners.size, 0);
  }
}

class Element extends Surface {
  constructor(document, rect = { left: 0, top: 0, width: 100, height: 100 }) {
    super();
    this.ownerDocument = document;
    this.rect = rect;
    this.style = { touchAction: "pan-y" };
    this.captures = new Set();
    this.clientWidth = rect.width;
    this.clientHeight = rect.height;
  }
  getBoundingClientRect() { return this.rect; }
  setPointerCapture(id) { this.captures.add(id); }
  hasPointerCapture(id) { return this.captures.has(id); }
  releasePointerCapture(id) {
    this.captures.delete(id);
    fire(this, "lostpointercapture", { pointerId: id });
  }
}

function fire(target, type, values = {}) {
  const event = new Event(type, { cancelable: true });
  Object.assign(event, values);
  target.dispatchEvent(event);
  return event;
}

let env;
let original;
beforeEach(() => {
  const window = new Surface();
  const document = new Surface();
  document.hidden = false;
  document.defaultView = window;
  window.document = document;
  const frames = new Map();
  const observers = new Set();
  let id = 0;
  env = {
    window, document, frames, observers, now: 0, pads: [],
    frame(ms) {
      this.now += ms;
      const callbacks = [...frames.values()];
      frames.clear();
      for (const callback of callbacks) callback(this.now);
    },
    element: (rect) => new Element(document, rect),
  };
  const globals = {
    window, document,
    devicePixelRatio: 3,
    navigator: { getGamepads: () => env.pads },
    performance: { now: () => env.now },
    requestAnimationFrame(callback) { frames.set(++id, callback); return id; },
    cancelAnimationFrame(handle) { frames.delete(handle); },
    ResizeObserver: class {
      constructor(callback) { this.callback = callback; observers.add(this); }
      observe() {}
      disconnect() { observers.delete(this); }
    },
  };
  original = new Map();
  for (const [key, value] of Object.entries(globals)) {
    original.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { configurable: true, writable: true, value });
  }
});
afterEach(() => {
  for (const [key, descriptor] of original) {
    if (descriptor) Object.defineProperty(globalThis, key, descriptor);
    else delete globalThis[key];
  }
});

test("fixed loop bounds catch-up, suspends on hidden, and cannot restart after disposal", () => {
  const updates = [], renders = [];
  const stop = game.loop({ step: 0.01, maxSteps: 3, update: (dt) => updates.push(dt), render: (alpha) => renders.push(alpha) });
  env.frame(100);
  assert.equal(updates.length, 3);
  assert.ok(renders[0] >= 0 && renders[0] < 1);
  env.document.hidden = true;
  fire(env.document, "visibilitychange");
  assert.equal(env.frames.size, 0);
  env.frame(1000);
  env.document.hidden = false;
  fire(env.document, "visibilitychange");
  env.frame(10);
  assert.equal(updates.length, 4, "resuming must not simulate the hidden interval");
  stop(); stop();
  fire(env.document, "visibilitychange");
  assert.equal(env.frames.size, 0);
  assert.equal(env.document.listenerCount, 0);
});

test("loop rejects invalid timing and honors stop from inside an update", () => {
  for (const options of [{ step: 0 }, { step: -1 }, { step: Infinity }, { maxSteps: 0 }, { maxSteps: 1.5 }]) {
    assert.throws(() => game.loop(options), RangeError);
  }
  let updates = 0, renders = 0;
  const stop = game.loop({ step: 0.01, update() { updates++; stop(); }, render() { renders++; } });
  env.frame(100);
  assert.equal(updates, 1);
  assert.equal(renders, 0);
  assert.equal(env.frames.size, 0);
});

test("input preserves legacy keys/pointer and clears both held and pressed on blur", () => {
  const keys = game.input();
  fire(env.window, "keydown", { code: "KeyW" });
  fire(env.window, "keydown", { code: "KeyW", repeat: true });
  assert.equal(keys.down("KeyW"), true);
  assert.equal(keys.pressed("KeyW"), true);
  assert.equal(keys.axis("KeyW", "KeyS"), -1);
  keys.endFrame();
  assert.equal(keys.pressed("KeyW"), false);
  fire(env.window, "keydown", { code: "Space" });
  fire(env.window, "pointerdown", { pointerId: 1, pointerType: "touch", clientX: 20, clientY: 30 });
  assert.equal(keys.pointer.down, true);
  fire(env.window, "blur");
  assert.equal(keys.down("Space"), false);
  assert.equal(keys.pressed("Space"), false);
  assert.equal(keys.pointer.down, false);
  keys.dispose(); keys.dispose();
  assert.equal(env.window.listenerCount, 0);
  assert.equal(env.document.listenerCount, 0);
});

test("input tracks simultaneous pointer IDs and does not clear another finger on cancellation", () => {
  const element = env.element({ left: 10, top: 20, width: 100, height: 100 });
  const keys = game.input(element);
  fire(element, "pointerdown", { pointerId: 1, pointerType: "touch", clientX: 20, clientY: 30 });
  fire(element, "pointerdown", { pointerId: 2, pointerType: "touch", clientX: 50, clientY: 60 });
  assert.equal(keys.pointers.size, 2);
  assert.equal(keys.pointers.get(1).x, 10);
  fire(element, "pointercancel", { pointerId: 2 });
  assert.equal(keys.pointers.size, 1);
  assert.equal(keys.pointer.down, true);
  fire(env.window, "pointerup", { pointerId: 1 });
  assert.equal(keys.pointer.down, false);
  assert.equal(keys.pointers.size, 0);
  fire(element, "keydown", { code: "KeyD" });
  env.document.hidden = true;
  fire(env.document, "visibilitychange");
  assert.equal(keys.down("KeyD"), false);
  keys.dispose();
  assert.equal(element.listenerCount + env.window.listenerCount + env.document.listenerCount, 0);
});

test("joystick and action button keep separate fingers active and release captures", () => {
  assert.equal(typeof game.joystick, "function");
  assert.equal(typeof game.actionButton, "function");
  const pad = env.element(), boost = env.element();
  const stick = game.joystick(pad, { radius: 40, deadZone: 0 });
  const action = game.actionButton(boost);
  fire(pad, "pointerdown", { pointerId: 1, clientX: 90, clientY: 50, button: 0 });
  fire(boost, "pointerdown", { pointerId: 2, button: 0 });
  assert.equal(stick.x, 1);
  assert.equal(stick.y, 0);
  assert.equal(action.down, true);
  assert.equal(action.pressed, true);
  action.endFrame();
  assert.equal(action.pressed, false);
  fire(pad, "pointermove", { pointerId: 2, clientX: 0, clientY: 100 });
  assert.equal(stick.x, 1, "another finger cannot steal steering");
  fire(boost, "pointercancel", { pointerId: 2 });
  assert.equal(action.down, false);
  assert.equal(stick.active, true);
  fire(pad, "pointerup", { pointerId: 1 });
  assert.deepEqual([stick.x, stick.y, stick.active], [0, 0, false]);
  assert.equal(pad.captures.size + boost.captures.size, 0);
  stick.dispose(); action.dispose();
  assert.equal(pad.style.touchAction, "pan-y");
  assert.equal(boost.style.touchAction, "pan-y");
  assert.equal(pad.listenerCount + boost.listenerCount + env.window.listenerCount + env.document.listenerCount, 0);
});

test("joystick uses radial dead zone and normalizes diagonal magnitude", () => {
  const element = env.element();
  const stick = game.joystick(element, { radius: 50, deadZone: 0.2 });
  fire(element, "pointerdown", { pointerId: 4, clientX: 55, clientY: 50, button: 0 });
  assert.deepEqual([stick.x, stick.y], [0, 0]);
  fire(element, "pointermove", { pointerId: 4, clientX: 100, clientY: 100 });
  assert.ok(Math.abs(Math.hypot(stick.x, stick.y) - 1) < 1e-10);
  env.document.hidden = true;
  fire(env.document, "visibilitychange");
  assert.equal(stick.active, false);
  stick.dispose();
  assert.throws(() => game.joystick(element, { radius: 0 }), RangeError);
  assert.throws(() => game.joystick(element, { deadZone: 1 }), RangeError);
});

test("action button stays down until every finger/key is released and clears on capture loss", () => {
  const element = env.element();
  const action = game.actionButton(element, { keys: ["Space"] });
  fire(element, "pointerdown", { pointerId: 1, button: 0 });
  fire(element, "pointerdown", { pointerId: 2, button: 0 });
  fire(env.window, "keydown", { code: "Space" });
  fire(element, "pointerup", { pointerId: 1 });
  fire(element, "lostpointercapture", { pointerId: 2 });
  assert.equal(action.down, true, "held keyboard action survives pointer release");
  fire(env.window, "keyup", { code: "Space" });
  assert.equal(action.down, false);
  fire(element, "pointerdown", { pointerId: 3, button: 0 });
  fire(env.window, "blur");
  assert.equal(action.down, false);
  assert.equal(action.pressed, false);
  action.dispose();
  fire(element, "pointerdown", { pointerId: 4, button: 0 });
  assert.equal(action.down, false);
});

test("action button focus loss clears keyboard edges while preserving a held touch", () => {
  const element = env.element();
  const action = game.actionButton(element);
  fire(element, "keydown", { code: "Space" });
  assert.equal(action.pressed, true);
  fire(element, "blur");
  assert.equal(action.down, false);
  assert.equal(action.pressed, false);
  fire(element, "pointerdown", { pointerId: 1, button: 0 });
  fire(element, "blur");
  assert.equal(action.down, true, "focus changes must not release another input source");
  element.disabled = true;
  fire(element, "pointerdown", { pointerId: 2, button: 0 });
  fire(element, "pointerup", { pointerId: 1 });
  assert.equal(action.down, false, "disabled controls must not accept another finger");
  action.dispose();
});

test("gamepad polls current device, applies dead zones, and clears stale disconnect state", () => {
  assert.equal(typeof game.gamepad, "function");
  const pad = game.gamepad({ deadZone: 0.2 });
  env.pads = [null, { connected: true, index: 1, id: "pad", axes: [0.1, -0.6, 0.9, 0], buttons: [{ pressed: true, value: 1 }, { pressed: false, value: 0.3 }] }];
  pad.poll();
  assert.equal(pad.connected, true);
  assert.equal(pad.x, 0);
  assert.ok(Math.abs(pad.y + 0.5) < 1e-10);
  assert.equal(pad.down(0), true);
  assert.equal(pad.value(1), 0.3);
  assert.equal(pad.value(99), 0);
  assert.ok(pad.axis(2) > 0.8);
  env.pads = [];
  pad.poll();
  assert.deepEqual([pad.connected, pad.x, pad.y, pad.down(0)], [false, 0, 0, false]);
  pad.dispose();
  assert.equal(env.window.listenerCount + env.document.listenerCount, 0);
});

test("gamepad handles denied access, explicit index, hidden tabs and disposal", () => {
  const pad = game.gamepad({ index: 2 });
  env.pads = [{ connected: true, index: 0, axes: [1, 1], buttons: [] }];
  pad.poll();
  assert.equal(pad.connected, false);
  navigator.getGamepads = () => { throw new DOMException("Blocked", "SecurityError"); };
  assert.doesNotThrow(() => pad.poll());
  navigator.getGamepads = () => [null, null, { connected: true, index: 2, axes: [1, 0], buttons: [] }];
  pad.poll();
  assert.equal(pad.x, 1);
  env.document.hidden = true;
  fire(env.document, "visibilitychange");
  assert.equal(pad.x, 0);
  pad.poll();
  assert.equal(pad.connected, false);
  env.document.hidden = false;
  pad.dispose(); pad.poll();
  assert.equal(pad.connected, false);
  assert.throws(() => game.gamepad({ deadZone: -1 }), RangeError);
  assert.throws(() => game.gamepad({ index: -1 }), RangeError);
});

test("adaptive quality waits for a sample window and cooldown, with hysteresis and bounds", () => {
  assert.equal(typeof game.adaptiveQuality, "function");
  const changes = [];
  const quality = game.adaptiveQuality({ minDpr: 0.75, maxDpr: 2, initialDpr: 1.5, sampleFrames: 4, cooldownFrames: 4, onChange: (dpr) => changes.push(dpr) });
  for (let i = 0; i < 3; i++) quality.sample(30);
  assert.equal(quality.dpr, 1.5);
  quality.sample(30);
  assert.equal(quality.dpr, 1.25);
  for (let i = 0; i < 4; i++) quality.sample(30);
  assert.equal(quality.dpr, 1.25, "cooldown prevents immediate repeated adjustments");
  for (let i = 0; i < 4; i++) quality.sample(16.7);
  assert.equal(quality.dpr, 1.25, "in-band frame times must not oscillate");
  for (let i = 0; i < 4; i++) quality.sample(8);
  assert.equal(quality.dpr, 1.5);
  for (let i = 0; i < 100; i++) quality.sample(60);
  assert.equal(quality.dpr, 0.75);
  assert.ok(changes.every((dpr) => dpr >= 0.75 && dpr <= 2));
  quality.reset();
  assert.equal(quality.dpr, 1.5);
  quality.dispose();
  for (let i = 0; i < 50; i++) quality.sample(60);
  assert.equal(quality.dpr, 1.5);
});

test("adaptive quality rejects invalid configuration and ignores suspension gaps", () => {
  assert.throws(() => game.adaptiveQuality({ minDpr: 2, maxDpr: 1 }), RangeError);
  assert.throws(() => game.adaptiveQuality({ targetFps: 0 }), RangeError);
  assert.throws(() => game.adaptiveQuality({ sampleFrames: 0 }), RangeError);
  const quality = game.adaptiveQuality({ initialDpr: 1, sampleFrames: 2 });
  for (const value of [NaN, Infinity, -1, 0, 3000]) quality.sample(value);
  assert.equal(quality.dpr, 1);
  quality.sample(30);
  assert.equal(quality.dpr, 1, "invalid samples must not count toward a window");
  quality.dispose();
});

test("fit responds to DPR changes and explicit adaptive refresh, then disconnects", () => {
  const canvas = env.element({ left: 0, top: 0, width: 320, height: 180 });
  let maxDpr = 2;
  const sizes = [];
  const stop = game.fit(canvas, (w, h, dpr) => sizes.push([w, h, dpr]), { maxDpr: () => maxDpr });
  assert.deepEqual(sizes[0], [320, 180, 2]);
  maxDpr = 1;
  stop.refresh();
  assert.deepEqual(sizes.at(-1), [320, 180, 1]);
  globalThis.devicePixelRatio = 0.75;
  fire(env.window, "resize");
  assert.deepEqual(sizes.at(-1), [320, 180, 0.75]);
  stop();
  const count = sizes.length;
  stop.refresh(); fire(env.window, "resize");
  assert.equal(sizes.length, count);
  assert.equal(env.observers.size + env.window.listenerCount, 0);
});

test("renderer selects WebGL without WebGPU and honors force-WebGL even when available", async () => {
  assert.equal(typeof game.createRenderer, "function");
  let gpu = 0, gl = 0;
  const webgpu = () => { gpu++; return { init: async () => {} }; };
  const webgl = () => { gl++; return {}; };
  let selected = await game.createRenderer({ webgpu, webgl });
  assert.equal(selected.backend, "webgl");
  assert.ok(selected.fallbackReason.includes("WebGPU"));
  navigator.gpu = {};
  selected = await game.createRenderer({ webgpu, webgl, prefer: "webgl" });
  assert.equal(selected.backend, "webgl");
  assert.equal(selected.fallbackReason, null);
  assert.deepEqual([gpu, gl], [0, 2]);
});

test("renderer awaits initialization and reports a backend's own internal fallback", async () => {
  navigator.gpu = {};
  const order = [];
  const renderer = { async init() { await Promise.resolve(); order.push("init"); }, backend: "webgl" };
  const selected = await game.createRenderer({ webgpu: () => renderer, webgl: () => { throw new Error("must not be used"); }, backendOf: (r) => r.backend });
  order.push("resolved");
  assert.deepEqual(order, ["init", "resolved"]);
  assert.equal(selected.renderer, renderer);
  assert.equal(selected.backend, "webgl");
  assert.ok(selected.fallbackReason.includes("WebGL"));
});

test("renderer disposes failed WebGPU init before trying WebGL, and reports both failures", async () => {
  navigator.gpu = {};
  const order = [];
  const gpu = { async init() { throw new Error("adapter unavailable"); }, dispose() { order.push("dispose gpu"); } };
  const gl = {};
  const result = await game.createRenderer({ webgpu: () => gpu, webgl() { order.push("make gl"); return gl; } });
  assert.deepEqual(order, ["dispose gpu", "make gl"]);
  assert.equal(result.backend, "webgl");
  assert.ok(result.fallbackReason.includes("adapter unavailable"));
  await assert.rejects(game.createRenderer({ webgpu: () => gpu, webgl: () => { throw new Error("WebGL context denied"); } }), (error) => {
    assert.ok(error instanceof AggregateError);
    assert.ok(error.message.includes("WebGPU") && error.message.includes("WebGL"));
    assert.equal(error.errors.length, 2);
    assert.ok(error.errors.some((failure) => failure.message.includes("adapter unavailable")));
    return true;
  });
});

test("renderer cancellation disposes late initialization and does not create a fallback", async () => {
  navigator.gpu = {};
  const controller = new AbortController();
  let dispose = 0, fallback = 0;
  const renderer = { async init() { controller.abort(); }, dispose() { dispose++; } };
  await assert.rejects(game.createRenderer({ webgpu: () => renderer, webgl: () => { fallback++; return {}; }, signal: controller.signal }), { name: "AbortError" });
  assert.equal(dispose, 1);
  assert.equal(fallback, 0);
  await assert.rejects(game.createRenderer({ webgl: () => { fallback++; return {}; }, signal: controller.signal }), { name: "AbortError" });
  assert.equal(fallback, 0);
});

test("renderer rejects invalid options and invalid factory results with useful causes", async () => {
  await assert.rejects(game.createRenderer({ prefer: "canvas", webgl: () => ({}) }), /prefer/);
  await assert.rejects(game.createRenderer({}), /webgl fallback factory/);
  await assert.rejects(game.createRenderer({ webgpu: 42, webgl: () => ({}) }), /renderer factory/);
  for (const value of [null, undefined, 42, "renderer"]) {
    await assert.rejects(game.createRenderer({ webgl: () => value }), (error) => {
      assert.ok(error instanceof AggregateError);
      assert.match(error.errors[0].message, /factory did not return a renderer/);
      return true;
    });
  }
  let disposed = 0;
  await assert.rejects(game.createRenderer({
    webgl: () => ({ dispose() { disposed++; } }),
    backendOf: () => "canvas",
  }), (error) => {
    assert.match(error.errors[0].message, /backendOf must return/);
    return true;
  });
  assert.equal(disposed, 1);
});

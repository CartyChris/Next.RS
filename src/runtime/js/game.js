// forge:game: browser timing, input, sizing, quality and renderer selection.
// No dependencies or renderer imports. Existing GLSL games can keep WebGL.
const clamp = (value, min, max) => Math.min(max, Math.max(min, value));
function positive(name, value) {
  if (!Number.isFinite(value) || value <= 0) throw new RangeError(`${name} must be a finite positive number`);
  return value;
}
function deadZoneValue(value) {
  if (!Number.isFinite(value) || value < 0 || value >= 1) throw new RangeError("deadZone must be in [0, 1)");
  return value;
}
function on(target, type, listener, options) {
  target.addEventListener(type, listener, options);
  return () => target.removeEventListener(type, listener, options);
}
function surfaces(target) {
  const doc = target.ownerDocument ?? target.document ?? document;
  return { doc, win: doc.defaultView ?? window };
}
function lifecycle(target, reset) {
  const { doc, win } = surfaces(target);
  return [on(win, "blur", reset), on(doc, "visibilitychange", () => { if (doc.hidden) reset(); })];
}
function capture(target, id) {
  // The browser may have cancelled a pointer before its handler runs.
  try { target.setPointerCapture?.(id); } catch { /* No active pointer to capture. */ }
}
function release(target, id) {
  try { if (target.hasPointerCapture?.(id)) target.releasePointerCapture(id); } catch { /* Already released. */ }
}

/** Fixed steps in seconds and render interpolation. Hidden tabs accumulate no
 * simulation time. stop() is idempotent and may be called from either callback. */
export function loop({ update, render, step = 1 / 60, maxSteps = 5 }) {
  positive("step", step);
  if (!Number.isSafeInteger(maxSteps) || maxSteps < 1) throw new RangeError("maxSteps must be a positive integer");
  let raf = 0, last = 0, acc = 0, stopped = false;
  const frame = (now) => {
    if (stopped || document.hidden) return;
    acc += clamp((now - last) / 1000, 0, step * maxSteps);
    last = now;
    // Avoid losing a whole step to repeated subtraction's floating-point error.
    const steps = Math.min(maxSteps, Math.floor((acc + step * 1e-9) / step));
    for (let i = 0; i < steps; i++) {
      update?.(step);
      if (stopped) return;
    }
    acc = Math.max(0, acc - steps * step);
    render?.(Math.min(acc / step, 1 - Number.EPSILON));
    if (!stopped) raf = requestAnimationFrame(frame);
  };
  const begin = () => {
    cancelAnimationFrame(raf);
    if (stopped || document.hidden) return;
    last = performance.now(); acc = 0;
    raf = requestAnimationFrame(frame);
  };
  const onVisibility = () => (document.hidden ? cancelAnimationFrame(raf) : begin());
  document.addEventListener("visibilitychange", onVisibility);
  begin();
  return () => {
    stopped = true;
    cancelAnimationFrame(raf);
    document.removeEventListener("visibilitychange", onVisibility);
  };
}

/** Keyboard + pointer state. The original API is retained. pointers holds every
 * active pointer in target-local CSS pixels. Cancellation clears its pointer;
 * blur and hidden clear all state. */
export function input(target = window) {
  const held = new Set(), fresh = new Set(), pointers = new Map();
  const pointer = { x: 0, y: 0, down: false };
  const { win } = surfaces(target);
  const move = (event) => {
    const rect = target.getBoundingClientRect?.() ?? { left: 0, top: 0 };
    const x = event.clientX - rect.left, y = event.clientY - rect.top;
    const active = pointers.get(event.pointerId);
    if (active) Object.assign(active, { x, y });
    if (active || !pointers.size) Object.assign(pointer, { x, y });
  };
  const finish = (event) => {
    if (!pointers.delete(event.pointerId)) return;
    release(target, event.pointerId);
    const next = pointers.values().next().value;
    if (next) Object.assign(pointer, { x: next.x, y: next.y, down: true });
    else pointer.down = false;
  };
  const reset = () => {
    held.clear(); fresh.clear();
    const ids = [...pointers.keys()];
    pointers.clear(); pointer.down = false;
    for (const id of ids) release(target, id);
  };
  const off = [
    on(target, "keydown", (event) => {
      if (!held.has(event.code)) fresh.add(event.code);
      held.add(event.code);
    }),
    on(target, "keyup", (event) => held.delete(event.code)),
    on(target, "pointermove", move),
    on(target, "pointerdown", (event) => {
      pointers.set(event.pointerId, { x: 0, y: 0, down: true, pointerId: event.pointerId, pointerType: event.pointerType });
      move(event); pointer.down = true;
      capture(target, event.pointerId);
    }),
    on(target, "pointerup", finish), on(target, "pointercancel", finish), on(target, "lostpointercapture", finish),
    ...lifecycle(target, reset),
  ];
  if (target !== win) {
    off.push(on(win, "pointerup", finish), on(win, "pointercancel", finish), on(target, "blur", reset));
    off.push(on(win, "keyup", (event) => held.delete(event.code)));
  }
  return {
    pointer, pointers,
    down: (code) => held.has(code), pressed: (code) => fresh.has(code),
    axis: (negative, positive) => (held.has(positive) ? 1 : 0) - (held.has(negative) ? 1 : 0),
    endFrame: () => fresh.clear(), reset,
    dispose() { off.forEach((remove) => remove()); reset(); },
  };
}

/** Fixed-center touch joystick, one owning pointer per control. Other fingers
 * can operate independent sticks/buttons. x/y are normalized, y points down. */
export function joystick(target, { radius, deadZone = 0.12, onChange } = {}) {
  if (radius !== undefined) positive("radius", radius);
  deadZoneValue(deadZone);
  const { win } = surfaces(target);
  const oldTouchAction = target.style.touchAction;
  target.style.touchAction = "none";
  let pointerId = null, x = 0, y = 0, disposed = false;
  const notify = () => onChange?.({ x, y, active: pointerId !== null });
  const move = (event) => {
    if (event.pointerId !== pointerId) return;
    event.preventDefault();
    const rect = target.getBoundingClientRect();
    const distance = radius ?? Math.max(1, Math.min(rect.width, rect.height) / 2);
    const dx = (event.clientX - rect.left - rect.width / 2) / distance;
    const dy = (event.clientY - rect.top - rect.height / 2) / distance;
    const magnitude = Math.hypot(dx, dy);
    const scaled = magnitude > deadZone ? (Math.min(magnitude, 1) - deadZone) / (1 - deadZone) : 0;
    x = scaled ? dx / magnitude * scaled : 0;
    y = scaled ? dy / magnitude * scaled : 0;
    notify();
  };
  const reset = () => {
    const id = pointerId, changed = id !== null || x !== 0 || y !== 0;
    pointerId = null; x = 0; y = 0;
    if (id !== null) release(target, id);
    if (changed) notify();
  };
  const finish = (event) => { if (event.pointerId === pointerId) reset(); };
  const off = [
    on(target, "pointerdown", (event) => {
      if (pointerId !== null || (event.button !== undefined && event.button !== 0)) return;
      pointerId = event.pointerId;
      capture(target, pointerId); move(event);
    }),
    on(target, "pointermove", move),
    on(target, "pointerup", finish), on(target, "pointercancel", finish), on(target, "lostpointercapture", finish),
    on(win, "pointerup", finish), on(win, "pointercancel", finish),
    ...lifecycle(target, reset),
  ];
  return {
    get x() { return x; }, get y() { return y; }, get active() { return pointerId !== null; }, reset,
    dispose() {
      if (disposed) return;
      disposed = true;
      off.forEach((remove) => remove()); reset();
      target.style.touchAction = oldTouchAction;
    },
  };
}

/** Independent boost/fire action. Each pointer and configured key holds its own
 * state. Focused controls accept Space/Enter. pressed persists until endFrame(). */
export function actionButton(target, { keys = [], onChange } = {}) {
  const { win } = surfaces(target);
  const pointers = new Set(), heldKeys = new Set(), codes = new Set(keys);
  const oldTouchAction = target.style.touchAction;
  target.style.touchAction = "none";
  let down = false, pressed = false, disposed = false;
  const sync = () => {
    const next = pointers.size > 0 || heldKeys.size > 0;
    if (next && !down) pressed = true;
    if (next !== down) { down = next; onChange?.(down); }
  };
  const reset = () => {
    const ids = [...pointers];
    pointers.clear(); heldKeys.clear(); pressed = false;
    sync();
    for (const id of ids) release(target, id);
  };
  const finish = (event) => {
    if (!pointers.delete(event.pointerId)) return;
    sync();
    if (event.type !== "pointerup" && !down) pressed = false;
    release(target, event.pointerId);
  };
  const keyDown = (event, focused) => {
    if (target.disabled || !(codes.has(event.code) || (focused && (event.code === "Space" || event.code === "Enter")))) return;
    event.preventDefault(); heldKeys.add(event.code); sync();
  };
  const keyUp = (event) => { heldKeys.delete(event.code); sync(); };
  const off = [
    on(target, "pointerdown", (event) => {
      if (target.disabled || (event.button !== undefined && event.button !== 0)) return;
      event.preventDefault(); pointers.add(event.pointerId); capture(target, event.pointerId); sync();
    }),
    on(target, "pointerup", finish), on(target, "pointercancel", finish), on(target, "lostpointercapture", finish),
    on(win, "pointerup", finish), on(win, "pointercancel", finish),
    on(target, "keydown", (event) => keyDown(event, true)), on(target, "keyup", keyUp),
    on(win, "keydown", (event) => keyDown(event, false)), on(win, "keyup", keyUp),
    on(target, "blur", () => { heldKeys.clear(); sync(); if (!down) pressed = false; }),
    ...lifecycle(target, reset),
  ];
  return {
    get down() { return down; }, get pressed() { return pressed; },
    endFrame() { pressed = false; }, reset,
    dispose() {
      if (disposed) return;
      disposed = true;
      off.forEach((remove) => remove()); reset();
      target.style.touchAction = oldTouchAction;
    },
  };
}

/** Poll once per update. Defaults to the first connected gamepad; axes are dead-
 * zone remapped and buttons keep analog values. Denied access yields zero state. */
export function gamepad({ index, deadZone = 0.15 } = {}) {
  deadZoneValue(deadZone);
  if (index !== undefined && (!Number.isSafeInteger(index) || index < 0)) throw new RangeError("index must be a nonnegative integer");
  let connected = false, id = "", activeIndex = -1, axes = [], buttons = [], disposed = false, blurred = false;
  const clear = () => { connected = false; id = ""; activeIndex = -1; axes = []; buttons = []; };
  const remap = (axis) => {
    const value = Number.isFinite(axis) ? clamp(axis, -1, 1) : 0;
    return Math.abs(value) > deadZone ? Math.sign(value) * (Math.abs(value) - deadZone) / (1 - deadZone) : 0;
  };
  const off = [
    on(window, "blur", () => { blurred = true; clear(); }), on(window, "focus", () => { blurred = false; }),
    on(window, "gamepaddisconnected", (event) => { if (event.gamepad?.index === activeIndex) clear(); }),
    on(document, "visibilitychange", () => { if (document.hidden) clear(); else blurred = false; }),
  ];
  const state = {
    get connected() { return connected; }, get id() { return id; }, get index() { return activeIndex; },
    get x() { return axes[0] ?? 0; }, get y() { return axes[1] ?? 0; },
    axis: (number) => axes[number] ?? 0, down: (number) => buttons[number]?.pressed ?? false,
    value: (number) => buttons[number]?.value ?? 0,
    poll() {
      if (disposed || document.hidden || blurred) { clear(); return state; }
      let available;
      try { available = navigator.getGamepads?.() ?? []; } catch { clear(); return state; }
      const pad = index === undefined ? Array.from(available).find((candidate) => candidate?.connected) : available[index];
      if (!pad?.connected) { clear(); return state; }
      connected = true; id = pad.id; activeIndex = pad.index;
      axes = Array.from(pad.axes, remap);
      buttons = Array.from(pad.buttons, (button) => ({
        pressed: button.pressed === true, value: Number.isFinite(button.value) ? clamp(button.value, 0, 1) : 0,
      }));
      return state;
    },
    dispose() { disposed = true; off.forEach((remove) => remove()); clear(); },
  };
  return state;
}

/** DPR-aware canvas sizing. maxDpr may be a getter, such as () => quality.dpr.
 * The returned disposer has refresh() for applying a quality change immediately. */
export function fit(canvas, onResize, { maxDpr = 2 } = {}) {
  // This is the public number-or-getter boundary, not a domain type assertion.
  const readMax = typeof maxDpr === "function" ? maxDpr : () => maxDpr;
  positive("maxDpr", readMax());
  const { win } = surfaces(canvas);
  let disposed = false;
  const apply = () => {
    if (disposed) return;
    const dpr = Math.min(globalThis.devicePixelRatio || 1, positive("maxDpr", readMax()));
    const width = canvas.clientWidth || 1, height = canvas.clientHeight || 1;
    onResize?.(width, height, dpr);
    if (!onResize) {
      canvas.width = Math.max(1, Math.round(width * dpr));
      canvas.height = Math.max(1, Math.round(height * dpr));
    }
  };
  const observer = new ResizeObserver(apply);
  observer.observe(canvas);
  const off = on(win, "resize", apply);
  apply();
  const dispose = () => { disposed = true; observer.disconnect(); off(); };
  dispose.refresh = apply;
  return dispose;
}

/** Bounded adaptive resolution: a complete sample window above 115% of the
 * target budget lowers DPR; below 80% raises it. Changes respect a frame cooldown.
 * Feed actual rendered-frame milliseconds. This is a budget, not an FPS promise. */
export function adaptiveQuality({
  minDpr = 0.75, maxDpr = 2, initialDpr = Math.min(globalThis.devicePixelRatio || 1, maxDpr),
  targetFps = 60, sampleFrames = 60, cooldownFrames = 120, step = 0.25, onChange,
} = {}) {
  positive("minDpr", minDpr); positive("maxDpr", maxDpr); positive("initialDpr", initialDpr);
  positive("targetFps", targetFps); positive("step", step);
  if (minDpr > maxDpr) throw new RangeError("minDpr must not exceed maxDpr");
  if (!Number.isSafeInteger(sampleFrames) || sampleFrames < 1) throw new RangeError("sampleFrames must be a positive integer");
  if (!Number.isSafeInteger(cooldownFrames) || cooldownFrames < 0) throw new RangeError("cooldownFrames must be a nonnegative integer");
  const initial = clamp(initialDpr, minDpr, maxDpr), budget = 1000 / targetFps;
  let dpr = initial, count = 0, total = 0, cooldown = 0, disposed = false;
  const change = (value, reason, averageMs) => {
    const next = clamp(Math.round(value * 1000) / 1000, minDpr, maxDpr);
    if (next === dpr) return;
    dpr = next; onChange?.(dpr, { reason, averageMs });
  };
  return {
    get dpr() { return dpr; },
    sample(frameMs) {
      if (disposed) return dpr;
      if (!Number.isFinite(frameMs) || frameMs <= 0 || frameMs > 1000 || globalThis.document?.hidden) {
        count = 0; total = 0; return dpr;
      }
      if (cooldown > 0) { cooldown--; return dpr; }
      count++; total += frameMs;
      if (count < sampleFrames) return dpr;
      const averageMs = total / count;
      count = 0; total = 0;
      const previous = dpr;
      if (averageMs > budget * 1.15) change(dpr - step, "slow", averageMs);
      else if (averageMs < budget * 0.8) change(dpr + step, "fast", averageMs);
      if (dpr !== previous) cooldown = cooldownFrames;
      return dpr;
    },
    reset() {
      if (disposed) return;
      count = 0; total = 0; cooldown = 0; change(initial, "reset", null);
    },
    dispose() { disposed = true; count = 0; total = 0; },
  };
}

function asError(cause) { return cause instanceof Error ? cause : new Error(String(cause)); }

/** Initialize factories without importing a graphics library. Failed candidates
 * are disposed before fallback. Factories own their canvases: use a fresh canvas
 * if a failed API already claimed a context. backendOf reports internal fallback,
 * as in Three WebGPURenderer. GLSL/EffectComposer apps should request WebGL. */
export async function createRenderer({ webgpu, webgl, prefer = "auto", backendOf, signal } = {}) {
  if (!["auto", "webgpu", "webgl"].includes(prefer)) throw new RangeError("prefer must be auto, webgpu or webgl");
  // Factory validation is confined to this external API boundary.
  if (typeof webgl !== "function") throw new TypeError("createRenderer requires a webgl fallback factory");
  if (webgpu !== undefined && typeof webgpu !== "function") throw new TypeError("webgpu must be a renderer factory");
  const checkAbort = () => { if (signal?.aborted) throw new DOMException("Renderer initialization was cancelled", "AbortError"); };
  checkAbort();
  const errors = [];
  let fallbackReason = null;
  const start = async (factory, requested) => {
    let renderer;
    try {
      renderer = await factory();
      checkAbort();
      if (!renderer || (typeof renderer !== "object" && typeof renderer !== "function")) {
        throw new TypeError(`${requested} factory did not return a renderer`);
      }
      await renderer.init?.();
      checkAbort();
      const backend = backendOf?.(renderer) ?? requested;
      if (backend !== "webgpu" && backend !== "webgl") throw new TypeError("backendOf must return webgpu or webgl");
      if (requested === "webgpu" && backend === "webgl") fallbackReason = "The renderer initialized its internal WebGL fallback.";
      return { renderer, backend, fallbackReason };
    } catch (cause) {
      try { await renderer?.dispose?.(); } catch (disposeCause) {
        if (!signal?.aborted) errors.push(asError(disposeCause));
      }
      checkAbort(); throw asError(cause);
    }
  };
  if (prefer !== "webgl" && webgpu) {
    if (prefer === "webgpu" || globalThis.navigator?.gpu) {
      try { return await start(webgpu, "webgpu"); } catch (cause) {
        checkAbort(); errors.push(cause);
        fallbackReason = `WebGPU initialization failed: ${cause.message}`;
      }
    } else fallbackReason = "WebGPU is unavailable in this browser/context; using WebGL.";
  }
  checkAbort();
  try { return await start(webgl, "webgl"); } catch (cause) {
    checkAbort(); errors.push(cause);
    throw new AggregateError(errors, "Unable to initialize WebGPU or WebGL. Check browser graphics support, context availability and the renderer factories.");
  }
}

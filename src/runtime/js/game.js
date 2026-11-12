// forge:game: the three things every canvas game re-implements: a fixed-timestep
// loop, input state, and DPR-aware canvas sizing. No dependencies; pairs with three.js.

/**
 * Fixed-timestep loop. `update(dt)` runs at a steady `step` (seconds); `render(alpha)`
 * runs once per frame with the interpolation factor. Pauses while the tab is hidden.
 * Returns `stop()`.
 */
export function loop({ update, render, step = 1 / 60, maxSteps = 5 }) {
  let raf = 0;
  let last = 0;
  let acc = 0;
  const frame = (now) => {
    raf = requestAnimationFrame(frame);
    acc += Math.min((now - last) / 1000, step * maxSteps);
    last = now;
    while (acc >= step) {
      update?.(step);
      acc -= step;
    }
    render?.(acc / step);
  };
  const begin = () => {
    cancelAnimationFrame(raf);
    last = performance.now();
    raf = requestAnimationFrame(frame);
  };
  const onVisibility = () => (document.hidden ? cancelAnimationFrame(raf) : begin());
  document.addEventListener("visibilitychange", onVisibility);
  begin();
  return () => {
    cancelAnimationFrame(raf);
    document.removeEventListener("visibilitychange", onVisibility);
  };
}

/**
 * Keyboard + pointer state. `down(code)` is held, `pressed(code)` is true once per
 * key press (call `endFrame()` at the end of each update), `axis(neg, pos)` is -1/0/1.
 */
export function input(target = window) {
  const held = new Set();
  const fresh = new Set();
  const pointer = { x: 0, y: 0, down: false };
  const on = (type, fn, opts) => (target.addEventListener(type, fn, opts), () => target.removeEventListener(type, fn, opts));
  const move = (e) => {
    const r = target.getBoundingClientRect?.() ?? { left: 0, top: 0 };
    pointer.x = e.clientX - r.left;
    pointer.y = e.clientY - r.top;
  };
  const off = [
    on("keydown", (e) => {
      if (!held.has(e.code)) fresh.add(e.code);
      held.add(e.code);
    }),
    on("keyup", (e) => held.delete(e.code)),
    on("blur", () => held.clear()),
    on("pointermove", move),
    on("pointerdown", (e) => (move(e), (pointer.down = true))),
    on("pointerup", () => (pointer.down = false)),
  ];
  return {
    pointer,
    down: (code) => held.has(code),
    pressed: (code) => fresh.has(code),
    axis: (neg, pos) => (held.has(pos) ? 1 : 0) - (held.has(neg) ? 1 : 0),
    endFrame: () => fresh.clear(),
    dispose: () => off.forEach((f) => f()),
  };
}

/**
 * Keep a canvas's backing store matched to its CSS size and the device pixel ratio.
 * Calls `onResize(width, height, dpr)` (CSS pixels) on every change. Returns `dispose()`.
 */
export function fit(canvas, onResize, { maxDpr = 2 } = {}) {
  const apply = () => {
    const dpr = Math.min(devicePixelRatio || 1, maxDpr);
    const w = canvas.clientWidth || 1;
    const h = canvas.clientHeight || 1;
    onResize?.(w, h, dpr);
    if (!onResize) {
      canvas.width = Math.round(w * dpr);
      canvas.height = Math.round(h * dpr);
    }
  };
  const ro = new ResizeObserver(apply);
  ro.observe(canvas);
  apply();
  return () => ro.disconnect();
}

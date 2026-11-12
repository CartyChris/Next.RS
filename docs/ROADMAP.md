# Roadmap

Tickets, roughly in priority order. Each is sized to be one focused change with tests. The original gap register is in [`handoff/`](../handoff/README.md); items below supersede it.

## Next

1. **Layout loaders and layout-level `Loading`/`ErrorBoundary`.** Today only pages have loaders and boundaries. Add `layout.loader.ts`, pass merged data, wrap each layout level in its own Suspense/Error boundary.
2. **Cancellation into JS.** Give loaders and API handlers an `AbortSignal` (`ctx.signal`) that fires on client disconnect or timeout, and abort React rendering through the same signal.
3. **Fast Refresh in dev.** Replace full-page reload with module hot replacement for React components (Rolldown has HMR primitives); keep full reload as fallback.
4. **Runtime error overlay in dev.** Show server render errors and client errors with source-mapped stacks, not only build errors.
5. **CSS Modules and `url()` in CSS.** Run CSS through Lightning CSS: modules, nesting, minification, and asset URLs inside stylesheets (today: plain global CSS; reference assets from JS or `public/`).
6. **Web Workers and WASM for AI/ML.** Support `new Worker(new URL("./w.ts", import.meta.url))` and `new URL("./model.onnx", import.meta.url)` so transformers.js / onnxruntime-web style apps bundle cleanly, including single-file builds.
7. **`basePath`.** Serve static builds from a sub-path (GitHub Pages project sites).
8. **HTTP range requests** for `public/` media (video/audio seeking).
9. **Form actions / mutations.** A typed `action` export next to loaders, with progressive enhancement for `<form>`.
10. **Cookies and sessions.** `ctx.cookies` read/write helpers; an auth example.

## Runtime

11. **V8 backend (cargo feature).** Same host-op interface and `server.js` protocol, backed by `deno_core`/`rusty_v8`, for JIT-speed SSR on JS-heavy pages. Keep QuickJS as the default for size and sandboxing.
12. **Bytecode snapshot of the server bundle** so new and recycled workers start without re-parsing.
13. **Request-scoped cache** (dedupe identical `fetch`/`cache()` calls within one render) and a **disk or Redis backend** for `forge:cache` and the page cache.
14. **Structured logs and metrics.** JSON request logs, per-route latency histograms, worker recycle counters, a `/health/details` endpoint.
15. **Process isolation mode.** Run JS workers in child processes (optionally under cgroups/seccomp) for multi-tenant hosting.
16. **`Intl` in server code.** Bundle a small Intl polyfill when the app uses `Intl` APIs, so server and client format identically.

## Build and targets

17. **WASM-host target.** Package the client app plus a WASM module host for embedding in native shells.
18. **Persistent build cache** (Rolldown incremental builds) to bring dev rebuilds of large apps under 100 ms.
19. **Image pipeline.** Responsive sizes and modern formats at build time for imported images.
20. **Dependency capability policy.** Per-package allow/deny rules from the bundle graph (e.g. forbid network or eval in specific packages).
21. **Windows support.** CI runs on Linux; path handling in the bundler plugin and the file watcher are untested on Windows.
22. **Desktop shell.** Wrap a single-html or server build in a native window (Tauri/WebView) from `forge build --target desktop`.

## Games

23. **Asset manifest for games.** Glob imports (`import.meta.glob`) for levels/sprites/audio, with preload progress hooks.
24. **WebSocket / WebTransport routes** for multiplayer and realtime AI.
25. **WebGPU example** and a compute-shader template.

## Done (this repository)

Compiling crate and tests, in-process Rolldown, QuickJS SSR worker pool, streaming SSR, data endpoint, API routes, server/static/single-html targets, page cache, security scan and CSP, dev server with live reload, five examples, CI. See [STATUS.md](STATUS.md).

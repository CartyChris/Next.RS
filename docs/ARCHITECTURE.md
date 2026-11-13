# Architecture

Next.RS ("Forge") is one Rust binary that contains the bundler, the JavaScript engine and the HTTP server. Node.js is only used to install npm packages; it is never needed to build or run an app.

```text
                 forge build                                   forge start / forge dev
  app/ ──scan──► route table ──analyze (Oxc)──► plan       ┌──────────────── one process ────────────────┐
                                                 │          │  Axum (multi-thread Tokio)                  │
                       codegen ◄─────────────────┘          │    │ one fallback handler routes everything │
          .forge/generated/{server,client}-entry.js         │    ▼                                        │
                         │                                  │  Pool ── least-loaded dispatch ──┐          │
              Rolldown (in-process, Forge plugin)           │   │                              │          │
            ┌────────────┴─────────────┐                    │   ▼                              ▼          │
      server bundle (1 file)    client bundle (split)       │  worker thread 1  …  worker thread N        │
            │                          │                    │  QuickJS + current-thread Tokio loop        │
            └──► .forge/server/        └──► .forge/client/  │  runs server.js; host ops: timers, fetch,   │
                 manifest.json, capsule.json, stats         │  streams, cache, env                        │
                                                            └─────────────────────────────────────────────┘
```

## Build (`src/build/`)

1. **Scan** (`routes.rs`): walk `app/`, resolve `page/layout/loader/route/not-found` with tsx > jsx > ts > js precedence, sort by specificity, reject ambiguous routes.
2. **Analyze** (`build/analyze.rs`): parse each route file once with Oxc. Collect export names (decides React vs vanilla page, validates loaders and API methods) and literal exports (`revalidate`, `ssr`, `streaming`).
3. **Generate** (`build/generated.rs`): write the runtime (embedded in the binary) and two entries:
   - `server-entry.js` statically imports every page, layout, loader and API route (client-only pages are not imported at all) and calls `createServer(...)`.
   - `client-entry.js` holds the route table with one `import()` per route, so each route is its own chunk.
4. **Bundle** (`build/bundle.rs`): Rolldown as a Rust library, with one `forge` plugin that
   - rewrites `new URL("./x", import.meta.url)`: scripts are bundled by a nested Rolldown build into a self-contained worker file (classic IIFE plus a Worker shim when inlined into single-file builds), other files become hashed or inlined assets,
   - resolves `forge:*` to the server or browser runtime file,
   - enforces the import policy (internals, remote URLs, Node built-ins in the browser),
   - collects CSS in module order, emits hashed or inlined assets, loads shaders/text as strings,
   - runs the AST security scan on app modules inside Rolldown's parallel transform phase (server build only, which sees every module).
5. **Emit** per target: `server` writes client chunks (Brotli + gzip precompressed), the server bundle, `manifest.json`, `capsule.json` (blake3 of every file) and `bundle-stats.json`. `static` and `single-html` start a build-time worker pool to run loaders, `staticParams()` and prerendering.

## Runtime (`src/runtime/`)

- **Pool**: N OS threads. Each owns one QuickJS runtime and a single-threaded Tokio event loop (`LocalSet`), so one worker interleaves many requests that await `fetch` or timers. Engines are never shared across threads. Requests go to the worker with the fewest in-flight requests.
- **Host ops** (`host.rs`): a `__host` object of Rust functions, claimed once by the runtime (`__forge_take_host`) so app code never sees it. Async ops (timers, fetch, body reads, drains) are spawned on the worker's loop and resolved back into JS by id.
- **Web platform** (`js/host.js`): `fetch`, `Request/Response/Headers`, `URL`, streams, encoders, `AbortController`, timers and `MessageChannel` built on the host ops. It is enough for `react-dom/server.browser`'s `renderToReadableStream`.
- **Protocol** (`js/server.js`): Rust sends `{ kind: page|data|api|paths, route, params, url, headers, nonce, stream }`. JS answers `head(json)`, then `write(chunk)*` or `pipe(fetchHandle)`, then `end()`. For pages the head carries `meta` and the boot JSON; Rust wraps the streamed markup in the document (`render.rs` + `build/html.rs`).
- **Backpressure**: `write` reports when more than 1 MiB is queued for a client; JS then awaits a drain. A disconnected client makes `write` fail, which stops rendering.
- **Cancellation**: request ids are assigned in Rust. An abort guard is created with each dispatch and travels inside the response body; if the future or the body is dropped before the response ends (client gone, timeout), the guard sends `Abort(id)` to the worker, which fires that request's `AbortController`. `fetch()` listens to the signal, rejects immediately and tells Rust to drop the late result.
- **Sockets and rooms**: a WebSocket's frames become `Socket(open | message | close)` jobs for one worker, chosen by hashing the URL path; plain HTTP requests to a socket route use the same worker. A process-wide registry maps socket ids to bounded outgoing queues and topics to subscribers, so `publish()` from any worker fans out in Rust. When an engine is replaced, the registry closes that worker's sockets (1012).
- **Limits**: QuickJS heap limit; an interrupt handler stops any uninterrupted JS run longer than `cpuMs` (and any pump longer than `max(5 × cpuMs, timeoutMs)`); requests time out after `timeoutMs`. A tripped limit or out-of-memory tears the engine down and builds a new one; requests on that worker fail, other workers are unaffected.

## Server (`src/server/`)

One Axum fallback handler, in order: probes → `/_forge/static/*` (immutable, precompressed) → `public/` files (paths with an extension) → `/_forge/data` → route table (WebSocket upgrades for socket routes) → not-found. A thin wrapper times every request (`Server-Timing`, access log). Page responses stream `document open + React stream + document close`, with a per-response CSP nonce. Routes with `revalidate` are buffered into a page cache and served stale-while-revalidate; `invalidate()` from server code evicts pages by path or prefix.

`forge dev` watches the project (excluding `node_modules`, `.forge`, `dist`), rebuilds in process, swaps in a new worker pool atomically, and tells browsers to reload over server-sent events. Build failures render as an overlay and recover on the next save.

## Single files (`src/build/html.rs`, `src/server/single.rs`)

A single-html build is one module script with everything inlined. Large scripts are deflated and stored as **base-122** text: the deflated bytes are read 7 bits at a time, each group becomes one ASCII character, and the six values that cannot appear raw in a `<script>` JS string (NUL and CR are rewritten by the HTML parser; LF, `"` and `\` end or escape the string; `<` could start `</script`) are folded with the next group into one two-byte UTF-8 character. That is about 14% overhead instead of base64's 33%. A ~300-byte decoder rebuilds the bytes, `DecompressionStream("deflate-raw")` inflates them, and the code is imported from a blob URL, so it runs as the same ES module.

`forge:file`'s `saveSelf` needs the document as it was parsed: the client captures `<head>` and the prerendered root at startup (single-html builds only, under an `import.meta.env.TARGET` constant), and the app script carries an id, so a saved copy is the original document plus one `<script type="application/json">` with the state. A copy with saved state client-renders instead of hydrating markup that no longer matches it.

`forge dev --target single-html` and `forge pack --watch` share `server/single.rs`: watch, rebuild the real artifact, serve it at `/` with the live-reload script, show build errors in the page.

## Pack (`src/pack/`)

`forge pack` turns an HTML page, or one component file, into a single offline file.

1. **Scan** (`html.rs`, lol_html): record every element, its attributes, and script/style text.
2. **Load**, concurrently, through one resolver (`Packer::load`): `data:` URLs; local files inside the page's folder (canonicalized, no hidden paths); CDN URLs (`cdn.rs`) mapped to npm packages; anything else over HTTP with a disk cache (`net.rs`).
3. **npm** (`npm.rs`): abbreviated registry metadata (gzip, cached for an hour), npm semver ranges with the `latest` tag preferred, tarballs verified against the published sha512 (or sha1) and extracted with every path checked, then placed npm-style: hoisted to the top when free, nested under the package that needs it on a conflict, required peers included. Each install round fetches metadata and tarballs concurrently. Nothing from a package is ever executed.
4. **Module scripts** (`js.rs`): one Rolldown build whose entry imports every module script in document order. A pack plugin resolves import maps (with scopes), CDN URLs (pinned files by path, packages through their `exports`), bare imports (a nearby `node_modules` first, then the npm workspace), and relative imports from remote modules. Page code is swept for asset-like string literals, which become data URLs.
5. **Classic scripts and styles** are inlined where they were. Large ones are deflated to base-122 text and turned back into a `<script>`/`<style>` by a synchronous 1.5 kB inflater (`inline.rs`, tested against flate2 in QuickJS) that inserts the element right after itself, which the browser runs or applies immediately, so ordering and render-blocking CSS hold.
6. **`text/babel`** is compiled with Oxc (classic JSX runtime, optional TypeScript) and placed at the end of `<body>`, where Babel would run it; the Babel runtime is dropped.
7. **Rewrite** in one more lol_html pass, by element index. The output always starts with `<meta charset="utf-8">`.

A component file (`component.rs`) is analyzed first (default export, JSX, React or Preact imports, `mount`, CSS imports, Tailwind-like classes), and a small page that mounts it goes through the same pipeline.

## Client (`src/runtime/js/client.js`, `react*.js`)

The client entry starts a small router: match the route table, load the route chunk and its data in parallel, render through an adapter (React or vanilla), update `history` (or `location.hash` in single-file builds), and restore scroll. Links are intercepted when they point at app routes; `Link` prefetches on hover, focus and touch. Server-rendered pages hydrate with the boot JSON; pages that were not server-rendered mount fresh.

## Why QuickJS instead of V8

The handoff design used `deno_core`/V8. This implementation embeds QuickJS (via `rquickjs`) instead:

| | QuickJS | V8 (deno_core) |
|---|---|---|
| Build | compiles from C in seconds, no downloads | prebuilt static library download (~100 MB), slow links |
| Binary | whole `forge` is ~25 MB | +40–60 MB |
| Per-worker memory | a few MB | tens of MB per isolate |
| Startup | ~10–20 ms to load a React server bundle | snapshot needed for comparable startup |
| Limits | heap limit + interrupt handler built in | heap limit + `terminate_execution` |
| Throughput | interpreter: React SSR ≈ 1.2–1.4k req/s on 4 cores for a small page | JIT: several times faster for JS-heavy renders |

For pages, data and APIs the engine is rarely the bottleneck; for very JS-heavy SSR it is. The engine sits behind a narrow interface (the host ops and the `server.js` protocol), so a V8 backend can be added behind a cargo feature without changing apps. That is on the roadmap.

## Why Rolldown in-process

The 8D fixture generated a `rolldown.config.mjs` and shelled out to the Rolldown CLI, which needs Node.js and an installed `rolldown` package. Linking the `rolldown` crate instead removes both, gives typed access to chunks and module graphs (manifest, preload lists and stats come straight from the output), and lets the security scan run inside the bundler's own parallel transform phase.

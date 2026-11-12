# Next.RS guide

Everything an app author needs. For how it works inside, see [ARCHITECTURE.md](ARCHITECTURE.md).

- [Project layout](#project-layout)
- [Routing](#routing)
- [Pages](#pages)
- [Layouts](#layouts)
- [Loaders (server data)](#loaders)
- [API routes](#api-routes)
- [Cancellation](#cancellation)
- [WebSockets and rooms](#websockets-and-rooms)
- [Web Workers, WASM and models](#web-workers-wasm-and-models)
- [AI: chat, tools and agents](#ai-chat-tools-and-agents)
- [Build targets](#build-targets)
- [Virtual modules](#virtual-modules)
- [Assets and CSS](#assets-and-css)
- [Configuration](#configuration)
- [Security model](#security-model)
- [Server JavaScript environment](#server-javascript-environment)
- [Deploying](#deploying)

## Project layout

```text
my-app/
  app/                 routes (pages, layouts, loaders, API routes)
  public/              files served as-is at the site root (server and static targets)
  forge.config.json    optional configuration
  package.json         npm dependencies (react, three, ...), installed with npm/pnpm/bun
  .env                 optional; only variables listed in server.env are exposed
  .forge/              build output (generated)
  dist/                static and single-html output (generated)
```

Run `forge new my-app --template <react|jsx|vanilla|three|ai>` to start from a working example.

## Routing

Routes come from directories under `app/`. In each directory Forge looks for:

| File | Purpose |
|---|---|
| `page.tsx` / `.jsx` / `.ts` / `.js` | the page for this URL |
| `layout.tsx` / `.jsx` / `.ts` / `.js` | wraps every page in this directory and below |
| `loader.ts` / `.tsx` / `.js` / `.jsx` | server-only data for the page beside it |
| `route.ts` / `.tsx` / `.js` / `.jsx` | API endpoint (cannot share a directory with a page) |
| `not-found.*` (in `app/` only) | the 404 page |

When several extensions exist, the first in the order **tsx, jsx, ts, js** wins, so TypeScript is preferred and JavaScript is fully supported.

| Directory | URL |
|---|---|
| `app/` | `/` |
| `app/blog/` | `/blog` |
| `app/blog/[slug]/` | `/blog/:slug` (one segment) |
| `app/docs/[...path]/` | `/docs/*` (one or more segments, joined with `/`) |
| `app/(marketing)/about/` | `/about` (groups organise files without changing URLs) |
| `app/_lib/` | nothing (underscore directories are private: put shared code there) |

Static segments beat dynamic ones, and dynamic beat catch-alls, segment by segment. Two directories that match the same URLs are a build error.

## Pages

A page is either a **React component** (default export) or a **vanilla page** (`render` and/or `mount` exports). The build detects which.

```tsx
// app/page.tsx: React
import { useState } from "react";

export const meta = { title: "Home", description: "…" };

export default function Page() {
  const [n, setN] = useState(0);
  return <button onClick={() => setN(n + 1)}>{n}</button>;
}
```

```js
// app/page.js: vanilla (no framework, no dependencies)
export const render = ({ data, params }) => `<main><h1>Hello</h1><button>+</button></main>`; // SSR / prerender
export function mount(el, ctx) {                                                             // browser
  const onClick = () => {};
  el.querySelector("button").addEventListener("click", onClick);
  return () => el.querySelector("button").removeEventListener("click", onClick);          // cleanup on navigation
}
```

Optional page exports:

| Export | Meaning |
|---|---|
| `meta` | object or `({ params, data, pathname }) => object`: `title`, `description`, `themeColor`, `icon`, `image`, `robots`, `canonical` |
| `ssr = false` | client-only page: the server never imports the module (safe for `window`, WebGL, three.js at module scope) |
| `revalidate = <seconds>` | cache the rendered HTML per URL and serve it stale-while-revalidate. Only for pages that look the same for every visitor (the cache ignores cookies); a cached copy keeps the CSP nonce it was rendered with |
| `streaming = false` | wait for all Suspense boundaries before sending (default: stream) |
| `Loading` | React component used as the Suspense fallback around the page |
| `ErrorBoundary` | React component `({ error, reset })` shown if the page throws |

A vanilla page without `render` is client-only automatically. A vanilla page inside React layouts is hosted by the React adapter.

## Layouts

```tsx
// app/layout.tsx
import { Link } from "forge:navigation";
import "./globals.css";

export default function Layout({ children }) {
  return (<><nav><Link to="/">Home</Link></nav>{children}</>);
}
```

Layouts nest from `app/` down. Their state survives client-side navigation between pages that share them.

## Loaders

`loader.ts` beside a page runs **only on the server** (or at build time for static and single-html builds). Its return value must be JSON-serialisable.

```ts
// app/blog/[slug]/loader.ts
import { notFound, redirect } from "forge:navigation";

export async function loader({ params, url, searchParams, request, env }: ForgeContext<{ slug: string }>) {
  const post = await db(params.slug);
  if (!post) notFound();               // renders not-found, status 404
  if (post.moved) redirect(post.url);  // 302 (or pass a status)
  return post;
}

// For static and single-html builds: which params to prerender.
export const staticParams = async () => [{ slug: "hello" }, { slug: "world" }];
```

Read it in the page with `useData()` (React) or `ctx.data` (vanilla). On client-side navigation the browser fetches `/_forge/data?path=…` (or a prebuilt JSON file in static builds, or embedded data in single-html builds).

## API routes

```ts
// app/api/items/[id]/route.ts
export async function GET(request: Request, { params, searchParams, env }: ForgeContext) {
  return { id: params.id };                   // plain values become JSON
}

export async function POST(request: Request) {
  const body = await request.json();
  return Response.json(body, { status: 201 }); // or any Response, including streams
}
```

Unlisted methods answer `405` with an `Allow` header. `HEAD` falls back to `GET`. Request bodies are capped by `server.maxBodyBytes`.

Returning `new Response(upstream.body)` where `upstream` came from `fetch()` streams the upstream bytes to the client **in Rust** without passing them through JavaScript: the fast path for LLM and media proxies.

## Cancellation

Every loader and API handler gets `ctx.signal` (also `request.signal`). It aborts when the client disconnects, at any point including mid-stream, or when the request exceeds `server.timeoutMs`. Pass it on so abandoned work stops:

```ts
export async function POST(request: Request, { signal, env }: ForgeContext) {
  const upstream = await fetch("https://api.openai.com/v1/chat/completions", { method: "POST", body: await request.text(), signal });
  return new Response(upstream.body); // a closed tab stops the upstream request (and its token bill)
}
```

`fetch()` rejects with an `AbortError` and late responses are dropped in Rust. Streaming React renders are aborted with the same signal.

## WebSockets and rooms

An API route that exports `socket` accepts WebSocket upgrades:

```ts
// app/api/room/[room]/route.ts
import { publish } from "forge:realtime";

const rooms = new Map<string, { members: number; log: string[] }>();

export const socket: ForgeSocketHandlers<{ room: string }> = {
  open(ws) {
    const room = rooms.get(ws.params.room) ?? { members: 0, log: [] };
    rooms.set(ws.params.room, room);
    room.members++;
    ws.subscribe(`room:${ws.params.room}`);
    ws.send({ type: "history", log: room.log });
  },
  message(ws, msg) {                       // JSON is parsed for you
    rooms.get(ws.params.room)!.log.push(msg.text);
    ws.publish(`room:${ws.params.room}`, { type: "say", text: msg.text }); // everyone else
  },
  close(ws) {
    rooms.get(ws.params.room)!.members--;
  },
};

// HTTP requests to a socket route run on the same worker as its sockets.
export const GET = (_req: Request, { params }: ForgeContext<{ room: string }>) => rooms.get(params.room) ?? { members: 0 };
```

**Rooms:** every socket for one URL path, and every HTTP request to it, runs on the same JS worker. So module-level state (like `rooms` above) is one consistent room. Different paths spread across workers. If that worker is replaced (crash or limit), its sockets close with code 1012 and clients reconnect.

**Pub/sub:** `ws.subscribe(topic)`, `ws.publish(topic, msg)` (all subscribers except this socket), and `publish(topic, msg)` / `subscribers(topic)` from `forge:realtime` in *any* server code. Fan-out happens in Rust across all workers, so a room can broadcast 20 snapshots a second without JS cost per recipient.

**Limits:** messages up to `server.maxBodyBytes`; 120 incoming messages/s per socket (bursts of 240), beyond that the socket closes with 1008; a client that falls 1024 outgoing messages behind is disconnected; a handler that throws closes its socket with 1011; pings every 25 s.

**Browser:** `forge:realtime` gives a reconnecting JSON socket:

```tsx
import { connect, useSocket } from "forge:realtime";

const { status, send } = useSocket(`/api/room/${room}`, (msg) => setLog((l) => [...l, msg]));
// status: "connecting" | "open" | "reconnecting" | "closed" | "offline" (file:// builds)
```

## Web Workers, WASM and models

`new URL("./file", import.meta.url)` is bundled, Vite-style:

```ts
const worker = new Worker(new URL("./train.worker.ts", import.meta.url), { type: "module" });
const model = await fetch(new URL("./models/net.onnx", import.meta.url));
```

- Script targets (`.js .ts .jsx .tsx .mjs .mts`) become self-contained worker bundles (`/_forge/static/workers/<name>.<hash>.js`), scanned like app code. Workers may start workers.
- Anything else becomes a hashed asset (models, `.wasm`, data files).
- In single-html builds workers are inlined as Blob URLs and assets as data URLs. Module workers cannot start from Blob URLs on `file://`, so Forge bundles inline workers as classic scripts and adapts `{ type: "module" }` for you. The Arena example trains neural networks this way, offline.

## AI: chat, tools and agents

`forge:ai` speaks four wire protocols behind one interface, in the browser and in server routes:

```ts
import { chat, runAgent } from "forge:ai";

// Stream text from any provider.
for await (const ev of chat({ provider: "anthropic", model: "claude-…", apiKey, messages: [{ role: "user", content: "Hi" }] })) {
  if (ev.type === "text") process(ev.text);
}
const answer = await chat({ provider: "ollama", model: "llama3.2", messages }).text();

// Tool-calling agent loop.
for await (const ev of runAgent({
  provider: "openai", model: "gpt-4o-mini", apiKey, signal,
  messages: [{ role: "user", content: "Weather in Paris?" }],
  tools: {
    get_weather: {
      description: "Current weather for a city",
      parameters: { type: "object", properties: { city: { type: "string" } }, required: ["city"] },
      run: async ({ city }) => ({ city, tempC: 21 }),
    },
  },
})) {
  // ev: text | tool_call | tool_result | usage | done | final
}
```

| Provider | Default base URL | Notes |
|---|---|---|
| `openai` | `https://api.openai.com/v1` | any OpenAI-compatible API: OpenRouter, Groq, Together, vLLM, LM Studio, llama.cpp, Ollama's `/v1` (set `baseUrl`) |
| `anthropic` | `https://api.anthropic.com/v1` | Messages API; adds the browser-access header when run in a browser |
| `gemini` | `https://generativelanguage.googleapis.com/v1beta` | `streamGenerateContent` over SSE |
| `ollama` | `http://localhost:11434` | native `/api/chat` NDJSON |
| `mock` | | offline: echoes, and calls a tool when the message names it |

Events are normalized: tool-call arguments arrive fully assembled (streamed partial JSON is joined for you), `usage` reports input/output tokens, `done` reports why the model stopped. Pass `signal: ctx.signal` in server routes so a closed tab cancels generation. Server-side keys come from `ctx.env` (listed in `server.env`), and the provider's host must be in `server.allowFetch`.

## Build targets

```bash
forge build                        # server: .forge/ for `forge start`
forge build --target static        # dist/: prerendered HTML + data JSON, any static host
forge build --target single-html   # dist/index.html: one self-contained file
```

| | server | static | single-html |
|---|:-:|:-:|:-:|
| SSR per request, streaming Suspense | ✓ | | |
| Loaders | per request | at build | at build (embedded) |
| API routes | ✓ | | |
| Client navigation | history | history | hash (`#/about`) |
| Page cache / `forge:cache` server side | ✓ | | |
| Works from `file://`, offline | | | ✓ |
| Code splitting | per route | per route | one script |
| Assets | hashed files | hashed files | data URLs |

Single-html details:
- The `/` route is prerendered into the file, so content shows before the script runs.
- Set `"build": { "compress": true }` to ship the script deflated (typically 60–70% smaller) and inflate it with `DecompressionStream` on open.
- `public/` is not included; import assets from code so they are inlined.
- Dynamic `import()` of app code is not allowed (everything is one script).

## Virtual modules

| Module | Exports |
|---|---|
| `forge:navigation` | `Link`, `navigate`, `prefetch`, `href`, `useRouter`, `useParams`, `usePathname`, `useSearchParams`, `currentRoute`, `notFound`, `redirect` |
| `forge:data` | `useData`, `getData`, `preloadData` |
| `forge:cache` | `cache(key, fn, { ttl, tags })` memoize; `get(key)`, `set(key, value, { ttl, tags })`, `keys(prefix)` key-value store; `invalidate(keyOrTag)`. Server: shared by all workers; `invalidate("/path")`, `"/path/*"` or `"*"` also evicts cached pages |
| `forge:store` | `store.get/set/remove`, `usePersistentState(key, initial)`: localStorage-backed, survives reloads, syncs tabs |
| `forge:ai` | `chat()`, `runAgent()` ([above](#ai-chat-tools-and-agents)); `streamText(url, init)`, `sse(response)`, `sseResponse(asyncIterable)`, `useStream()` |
| `forge:realtime` | browser: `connect(path, handlers)`, `useSocket(path, onMessage)`; server: `publish(topic, msg)`, `subscribers(topic)` |
| `forge:game` | `loop({ update, render, step })` fixed-timestep loop, `input()` keyboard/pointer state, `fit(canvas, onResize)` DPR-aware sizing |

Types are written to `.forge/types/forge.d.ts` on every build; include `.forge/types` in `tsconfig.json`.

## Assets and CSS

- `import "./styles.css"`: global CSS, bundled into one stylesheet in import order.
- `import url from "./logo.png"`: images, audio, video, fonts, `.glb/.gltf/.hdr/.ktx2`, `.wasm`, `.onnx`, `.gguf`, `.safetensors` become a hashed URL (single-html: a data URL).
- `import src from "./shader.frag"`: `.glsl .vert .frag .wgsl .txt .md .csv .prompt` become strings.
- `.json` imports are parsed objects.
- Files in `public/` are served at the site root (`public/robots.txt` → `/robots.txt`).

## Configuration

`forge.config.json` (every key optional; unknown keys are an error):

```json
{
  "appDir": "app",
  "publicDir": "public",
  "outDir": ".forge",
  "distDir": "dist",
  "title": "Forge App",
  "lang": "en",
  "server": {
    "host": "127.0.0.1",
    "port": 3000,
    "workers": 0,
    "env": ["OPENAI_API_KEY"],
    "allowFetch": ["api.openai.com", "*.example.com"],
    "memoryLimitMb": 256,
    "timeoutMs": 15000,
    "cpuMs": 2000,
    "maxBodyBytes": 1048576,
    "pageCacheEntries": 1024,
    "accessLog": false
  },
  "security": {
    "allowEval": false,
    "csp": true,
    "connectSrc": ["https://api.example.com"]
  },
  "build": {
    "minify": true,
    "sourcemap": false,
    "compress": false,
    "define": { "__VERSION__": "\"1.0.0\"" }
  }
}
```

- `workers: 0` means one JS worker per core (max 8). `PORT` in the environment overrides `server.port`.
- `server.accessLog: true` prints one line per request in production (`forge dev` always does). Every response carries a `Server-Timing` header, so devtools show server time.
- `security.csp`: `true` (default policy), `false` (no header), or a string (your policy; `{nonce}` is replaced per response).
- Compile-time constants: `process.env.NODE_ENV`, `import.meta.env.MODE/DEV/PROD/SSR`, plus `build.define`.

## Security model

Build time (every app module, with positions):
- `eval`, indirect `eval`, `globalThis.eval`, `new Function` and `Function(...)` are rejected unless `security.allowEval`.
- Imports of Forge internals (`ext:*`, `forge:internal*`, `.forge/generated`) and remote imports (`http:`, `https:`, `data:`, `blob:`) are rejected. Node built-ins are rejected in browser code.
- Strings and comments never trigger the scanner (it walks the AST).

Runtime:
- Server code sees only the env variables listed in `server.env`.
- `fetch()` reaches only hosts in `server.allowFetch`; redirects are re-checked; only `http(s)`.
- Every JS worker has a heap cap, a CPU budget per uninterrupted run, and a wall-clock timeout per request. A worker that exceeds a hard limit is replaced; other workers keep serving.
- HTML responses carry a strict CSP with a fresh nonce per response, `nosniff` and a referrer policy. Production errors never reach the client; development errors show details.
- `forge start` refuses a build whose files no longer match `capsule.json` (blake3 hashes).
- Static file serving cannot escape its directory and never serves dotfiles.

Known limits: workers share one process (no process-per-worker isolation yet), the in-memory caches are per process, and code already inside `node_modules` is not scanned.

## Server JavaScript environment

Server code runs in QuickJS (ES2023, modules, async). Available globals: `fetch`, `Request`, `Response`, `Headers`, `URL`, `URLSearchParams`, `ReadableStream`, `TransformStream`, `TextEncoder`/`TextDecoder` (+ streams), `AbortController`/`AbortSignal`, `setTimeout`/`setInterval`, `queueMicrotask`, `MessageChannel`, `crypto.getRandomValues`/`randomUUID`, `performance.now`, `atob`/`btoa`, `console`.

Not available: Node.js built-ins (`fs`, `net`, ...), `Intl` beyond what QuickJS provides, `process`, `Buffer`. Packages that need Node fail at build time with a clear error.

## Deploying

Server target: copy the `forge` binary, `.forge/`, `public/` and `forge.config.json`, then `forge start` (it binds `server.host:server.port`; set `"host": "0.0.0.0"` behind a load balancer). Health checks: `/health/live`, `/health/ready`. SIGTERM drains gracefully. Static assets are precompressed (Brotli and gzip) and immutable-cached; put a CDN or reverse proxy in front for HTML compression and TLS.

Static target: upload `dist/` anywhere (S3, GitHub Pages, Netlify, nginx) at the domain root; serve `404.html` for misses.

Single-html: send the file. It works from disk, email attachments, USB sticks and intranets.

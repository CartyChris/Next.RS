# Building with Next.RS: a guide for agents

Next.RS is a React framework in one binary, `forge`. It bundles, renders on the server, serves, tests and packs without Node.js; npm only installs packages. This page is the short version of everything you need. `forge docs <words>` searches the full documentation offline, and `forge mcp` serves the same documentation and tools over the Model Context Protocol.

## Work loop

```bash
forge new my-app --template starter   # then: cd my-app && npm install
forge dev                             # http://127.0.0.1:3000, rebuilds on save, errors show in the page
forge check                           # routes + security scan + writes .forge/types; run after every change
npx tsc -p .                          # type errors
forge test                            # *.test.ts files, in the same engine as server code
forge build                           # or --target static | single-html
```

Templates: `starter` (app shell on `forge:ui`; the default choice for applications), `react`, `jsx`, `vanilla`, `three` (3D game), `models` (Rust/WASM procedural 3D editor), `pixel` (self-saving single file), `ai` (chat), `arena` (multiplayer). `models` requires Cargo and `rustup target add wasm32-unknown-unknown`; use `npm ci` for its locked dependencies. `forge new --help` lists them.

Find things instead of guessing:

```bash
forge docs loader redirect            # search every document
forge docs --read design              # print one: agents, guide, ui, design, architecture, models
forge themes frosted glass            # find a look for the interface from a description
```

## Files are routes

```text
app/layout.tsx              wraps every page; put <Theme> here
app/page.tsx                /
app/projects/[id]/page.tsx  /projects/:id
app/projects/[id]/loader.ts server-only data for that page
app/api/projects/route.ts   API endpoint: export GET, POST, …
app/_lib/                   private folder (underscore): shared code, never a route
app/not-found.tsx           404
public/                     served as-is at the site root
forge.config.json           optional settings
```

```tsx
// app/projects/[id]/loader.ts — runs on the server, or at build time for static output
import { notFound } from "forge:navigation";

export async function loader({ params, env, signal }: ForgeContext<{ id: string }>) {
  const project = await findProject(params.id);
  if (!project) notFound();
  return { project };
}
export const staticParams = async () => [{ id: "atlas" }];   // which pages to prerender
```

```tsx
// app/projects/[id]/page.tsx
import { useData } from "forge:data";
import { Container, PageHeader, Stack } from "forge:ui";

type Data = { project: { name: string } };

export const meta = ({ data }: { data: Data }) => ({ title: data.project.name });

export default function Page() {
  const { project } = useData<Data>();
  return (
    <main>
      <Container>
        <Stack gap="lg">
          <PageHeader title={project.name} />
        </Stack>
      </Container>
    </main>
  );
}
```

```ts
// app/api/projects/route.ts
export async function GET() {
  return { projects: [] };                         // plain values become JSON
}
export async function POST(request: Request) {
  const body = await request.json();
  return Response.json(body, { status: 201 });
}
```

Page exports: `meta`, `ssr = false` (client-only: WebGL, `window` at module scope), `revalidate = 60` (cache the HTML), `Loading`, `ErrorBoundary`.

## Built-in modules

Import these; do not install replacements.

| Module | Use it for |
|---|---|
| `forge:ui` | Every piece of interface: `Theme`, layout, buttons, fields, dialogs, tables, menus, toasts, `AppShell`, game `Hud`. See the `ui` document. |
| `forge:navigation` | `Link to`, `navigate`, `useRouter`, `useParams`, `usePathname`, `notFound`, `redirect` |
| `forge:data` | `useData()` in pages, `getData`, `preloadData` |
| `forge:store` | `usePersistentState(key, initial)`: state that survives reloads (settings, save games) |
| `forge:cache` | server: `cache(key, fn, { ttl, tags })`, `get`, `set`, `invalidate` |
| `forge:realtime` | WebSocket rooms: `useSocket(path, onMessage)` in the browser, `publish(topic, msg)` on the server |
| `forge:ai` | `chat()` and a tool-calling `runAgent()` for OpenAI-compatible, Anthropic, Gemini and Ollama APIs |
| `forge:n8n` | `trigger(url, data)` calls an n8n workflow; `webhook(handler, { secret })` receives calls from one |
| `forge:file` | open, save and drop files; single HTML files that save themselves (`useDocument`) |
| `forge:game` | `loop`, `input`, `fit`; independent `joystick`/`actionButton`, `gamepad`, `adaptiveQuality`, async `createRenderer` fallback |
| `forge:mesh` | Load/validate the Rust mesh ABI and return copied positions, normals, UVs, indices and bounds |
| `forge:wasm/<name>` | URL of an automatically compiled `build.wasm` library; embedded in single-HTML output |
| `forge:test` | `test`, `describe`, `expect` for `forge test` |

## Interface

1. Wrap the app once in `app/layout.tsx`: `<Theme preset="forge">…<Toaster /></Theme>`.
2. Pick the look from the request. `forge themes <words>` finds a preset from a description, and `forge themes` lists them: `forge`, `liquid-glass`, `graphite`, `paper`, `clinic`, `midnight`, `terminal`, `arcade`. Adjust with `accent` (a name, any CSS color, or a hue), `radius`, `density`, `surface="glass"` and `tokens`.
3. Build pages from `forge:ui` components. Style your own parts with the `--forge-ui-*` tokens, never hard-coded colors.
4. Follow the `design` document. Its checklist is the definition of done for anything visual.

## Which target

| The user wants | Build with |
|---|---|
| A web app with a server: accounts, live data, APIs, sockets | `forge build`, run `forge start` |
| A site for static hosting | `forge build --target static` |
| One file to send or open offline (tools, games, demos) | `forge build --target single-html` |
| An existing HTML page or one component file made into one offline file | `forge pack page.html` or `forge pack App.tsx` |

Loaders run per request on the server target and at build time on the other two. API routes and sockets exist only on the server target; a single-file app keeps its state with `forge:store` or `forge:file`.

## Games

Start from `three` (3D) or `vanilla` (2D canvas). Make the page client-only (`export const ssr = false`) when it touches WebGL at module scope. Run the simulation in `loop()` from `forge:game` with a fixed step, read `input()`, size the canvas with `fit()`. Put interface over the canvas in `Hud` and `HudSlot` from `forge:ui`; menus are `Dialog`s. Save progress with `usePersistentState`. Multiplayer is a `socket` export in a `route.ts`: every connection to one path shares a worker, so module state is the room state.

For Rust-generated models, start from `models` and read `forge docs --read models`.
Declare the Cargo manifest in `build.wasm`, import `forge:wasm/<name>`, and use
`loadMeshGenerator` from `forge:mesh`. Rust generates mesh data; Three.js renders
it. Use compatible materials for WebGPU fallback; existing GLSL/EffectComposer
games need their WebGL renderer. On mobile, give joystick and action controls
independent pointers, poll `gamepad`, and feed rendered-frame times into
`adaptiveQuality`. Dispose input, geometry and renderers on unmount. Verify the
actual single HTML with network blocked in Chromium and WebKit; report physical
device and hardware-WebGPU testing separately.

## Pitfalls

- **Server code is not Node.** There is no `fs`, `process` or `Buffer`. Use `fetch`, and read secrets from `ctx.env` after listing them in `server.env` in `forge.config.json`. Outbound hosts must be listed in `server.allowFetch`.
- **Server and browser must render the same text.** Format dates with `timeZone: "UTC"`; `Math.random()` and `Date.now()` belong in effects or loaders, not in render.
- **`eval` and `new Function` are build errors**, as are imports from URLs. Install packages from npm.
- **A folder holds a page or a `route.ts`, never both.**
- **`Link` from `forge:navigation` takes `to`; `NavLink`, `ButtonLink` and `AppLink` from `forge:ui` take `href`.**
- **Loader data is JSON.** Convert dates and maps before returning them.
- **Single-file builds have no server**: no API routes, no `public/` folder. Import assets from code so they are inlined.
- **Types come from `.forge/types`.** Keep `"include": ["app", ".forge/types"]` in `tsconfig.json` and run `forge check` once before `tsc`.
- **Rust compilation is explicit.** Configured `build.wasm` crates run through Cargo with locked dependencies and normal host build permissions, including during `forge check` and `forge test`. Browser WASM does not run in the QuickJS server engine; the separate `wasm-host` target remains planned.

## For agents with MCP

`forge mcp` speaks MCP over stdio (`forge mcp --http 3333` for network clients such as n8n's MCP Client node). Register it once:

```json
{ "mcpServers": { "forge": { "command": "forge", "args": ["mcp"] } } }
```

Tools: `forge_docs_search`, `forge_docs_read`, `forge_ui_component` (props and types of one component), `forge_ui_themes` (find a look from a description), `forge_templates`, `forge_new`, `forge_routes`, `forge_check`, `forge_test`, `forge_build`, `forge_pack`.

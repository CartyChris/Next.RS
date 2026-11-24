# Model Lab

An offline-capable procedural model editor built with Next.RS. A dependency-free
Rust crate generates the actual mesh positions, unit normals, UVs and triangle
indices. Three.js displays those arrays; it does not generate the editable models.

## Run and build

Install Rust, Node.js and the Next.RS `forge` CLI, then:

```sh
rustup target add wasm32-unknown-unknown
npm ci
forge dev
```

Create a portable single HTML file, with the compiled WASM and all JavaScript/CSS
embedded:

```sh
forge build --target single-html
```

Open `dist/index.html`, or serve that file over HTTPS. No CDN, model downloads,
worker scripts, cross-origin isolation or backend is required. On iPhone and iPad,
opening the HTTPS page in Safari is the most predictable distribution route;
attachment previews and file-manager previews are different applications.

For an uncompressed file, set `build.compress` to `false` in `forge.config.json`
and run the same build command. The default compressed output uses
`DecompressionStream`, supported by Safari
16.4 and newer. WebGPU is available in Safari 26; earlier supported browsers use
WebGL 2. Actual availability also depends on the device and browser settings.
The viewer catches initialization errors and displays them in the interface.

## Controls and output

- Choose a sphere, torus, seeded rock or terrain patch. Change detail, scale,
  variation or seed, then select **Generate model**.
- Drag to orbit; use two fingers to pinch on touch screens. Scroll or the zoom
  buttons adjust distance. With the canvas focused, arrow keys orbit, `+` / `-`
  zoom and `Home` resets the view.
- Pause rotation, inspect wireframe or surface-normal shading, and reset the view.
  Reduced-motion preferences disable automatic rotation on startup.
- **Export GLB** downloads the current geometry with its normals, UVs, indices and
  PBR material. Export uses right-handed, Y-up coordinates in metres. Preview
  rotation, wireframe, the grid and normal-debug material are excluded.
  A synchronous binary writer serializes this single mesh without file reads;
  it does not support exporting arbitrary scenes, textures or animations.
- `?renderer=webgl` forces the WebGL 2 path for reproducible compatibility checks.
  The displayed renderer reports the backend that actually initialized.

Detail is bounded to 8–128 segments. The largest terrain contains 16,641 vertices
and 32,768 triangles. Device pixel ratio starts at no more than 1.5 and adapts to
measured frame time. These are safeguards, not a claim of a particular phone's
frame rate. Native Rust generation tests and browser verification are separate
from physical iPhone/iPad performance testing.

## Rust and browser boundary

`forge.config.json` declares the local Cargo manifest:

```json
{
  "build": {
    "wasm": [{ "name": "mesh", "manifestPath": "rust/mesh/Cargo.toml" }]
  }
}
```

Next.RS compiles the locked crate for `wasm32-unknown-unknown` and makes its output
available through the configured import:

```ts
import wasmUrl from "forge:wasm/mesh";
import { loadMeshGenerator } from "forge:mesh";

const generator = await loadMeshGenerator(wasmUrl);
const mesh = generator.generate({ kind: "rock", detail: 48, seed: 17 });
// mesh.positions, mesh.normals, mesh.uvs: Float32Array
// mesh.indices: Uint32Array; mesh.bounds, vertexCount, triangleCount
generator.dispose();
// Previously returned arrays remain valid: they are owned by JavaScript.
```

The ABI is version 1. It exports linear `memory`, `mesh_abi_version()`,
`mesh_generate(kind, detail, seed, scale, variation)`, `mesh_clear()`, scalar
vertex/index counts and four scalar buffer pointers. Kind codes are sphere `0`,
torus `1`, rock `2`, terrain `3`. Generation returns `0` on success or an error
code: `1` kind, `2` detail, `3` scale, `4` variation, `5` allocation failure.
Failed generation preserves the last Rust-owned mesh. Getters remain valid until
the next successful generation or clear call. No Rust structs or ownership are
passed across the ABI.

`forge:mesh` checks options, ABI version, buffer alignment, counts, memory bounds,
finite attributes, normal lengths and triangle indices. It takes a fresh memory
view after each generation and copies the arrays immediately, because WASM memory
growth can detach old views. Embedded data URLs are decoded directly instead of
being fetched. No generated wasm-bindgen glue or host imports are required.

The Rust implementation uses analytic primitive surfaces and seeded continuous
trigonometric deformation. It is a compact starting point for procedural modeling,
not a boolean-solid, sculpting or mesh-remeshing engine. Seeds reproduce geometry
within the same compiled module; floating-point transcendental functions may
produce small differences across toolchains or native architectures.

## Verification

After a build generates `.forge/types`:

```sh
npm run typecheck
cargo test --manifest-path rust/mesh/Cargo.toml --locked
cargo clippy --manifest-path rust/mesh/Cargo.toml --all-targets -- -D warnings
```

From the Next.RS repository root, `node --test tests/js/mesh.test.mjs` compiles and
loads the real WASM binary, checks all shapes, invalid inputs, malformed ABI
fixtures, deterministic seeds, buffer-copy lifetime, heap reuse and disposal.
`tests/browser/models.mjs` verifies the built application in configured browser
engines, including offline rendering and export. Playwright WebKit coverage is
engine testing; it does not substitute for a physical iPhone test.

## Design references

- [Rust's wasm32-unknown-unknown target](https://doc.rust-lang.org/rustc/platform-support/wasm32-unknown-unknown.html)
- [Three.js indexed BufferGeometry](https://threejs.org/docs/pages/BufferGeometry.html)
- [Three.js WebGPURenderer and WebGL fallback](https://threejs.org/manual/pages/webgpurenderer)
- [WebAssembly memory growth and detached buffers](https://developer.mozilla.org/en-US/docs/WebAssembly/Reference/JavaScript_interface/Memory/grow)
- [Safari 16.4 Compression Streams](https://webkit.org/blog/13966/webkit-features-in-safari-16-4/)
- [Safari 26 WebGPU](https://webkit.org/blog/17333/webkit-features-in-safari-26-0/)
- [Playwright browser engine coverage](https://playwright.dev/docs/browsers)

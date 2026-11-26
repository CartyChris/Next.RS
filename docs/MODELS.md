# Rust models, mobile input and browser rendering

Next.RS can compile independent Rust geometry and rendering modules and embed
them alongside browser code in one offline HTML file. Choose the renderer,
geometry source and deployment target separately.

| Template | Browser rendering | Package contents |
|---|---|---|
| `models` | Three.js using WebGPU or WebGL2 | Three.js, mesh WASM, shared editor |
| `models-rust` | Rust/WASM driving WebGL2 | Mesh WASM, renderer WASM, WebGL2 adapter and shared editor. No Three.js npm dependency |
| `models-mixed` | Switch between Three.js and Rust in the editor | Both rendering implementations in one download. Select Rust with `?renderer=rust`; `?renderer=webgl` forces Three's WebGL2 path |

All profiles accept the built-in sphere, torus, seeded rock and terrain, and
versioned mesh JSON. Changing engines in the mixed editor reframes the current
geometry and retains the active inspection settings. Different materials,
cameras and lighting can produce different images of the same mesh.

## Start with the models template

Install Rust with Cargo and the browser target, then create the example:

```bash
rustup target add wasm32-unknown-unknown
forge new model-lab --template models
cd model-lab
npm ci
forge dev --target single-html
```

Build the portable artifact:

```bash
forge check
npm run typecheck
forge build --target single-html
```

The result is `dist/index.html`. It contains the application, Three.js, styles and
compiled Rust WASM. No CDN, remote model, shader download or separate `.wasm` file
is required. The example generates a sphere, torus, seeded rock or terrain and
offers detail, seed, scale and variation controls, wireframe and normal views,
orbit/pinch controls and binary glTF (`.glb`) export.

The source is in `examples/model-lab`. Its Rust crate has no third-party Rust
dependencies; its npm dependencies are locked. Normal Next.RS applications that
do not configure Rust modules still use the existing build toolchain.

## Render a mesh with Rust and WebGL2

Start with `forge new lab --template models-rust`, then `cd lab && npm ci`.
In this project `app/_model/rust-renderer.ts` is a reusable canvas adapter.
A game can call it directly, independent of the React editor and its generators:

```ts
import { createRustRenderer } from "./_model/rust-renderer";
import { validateMesh } from "forge:mesh";

const stop = new AbortController();
const renderer = await createRustRenderer(canvas, stop.signal);
const mesh = validateMesh({ positions, normals, uvs, indices });
renderer.setMesh(mesh, [0.6, 0.4, 0.2]); // linear RGB
renderer.resize(800, 600);              // drawing-buffer pixels
renderer.draw(0, false, false);         // rotation (radians), wireframe, normals
// In the game's update/input path: renderer.orbit(dx, dy) or renderer.zoom(factor).
// On unmount:
stop.abort();
```

This API expects a fresh canvas and validated indexed triangles. The optional
renderer crate (`rust/renderer`) owns the orbit camera, projection, shaders,
triangle-to-line expansion, and draw parameters. Each canvas gets one WASM
instance. Two synchronous imports connect it to the browser. `webgl2.program`
compiles and links the Rust-supplied shaders. `webgl2.present` binds browser GPU
objects, uploads uniforms and submits the draw. The host uploads mesh buffers
when geometry changes. The geometry creator and renderer have separate memories
and lifetimes. The mesh ABI stays at version 1.

The Rust preview draws one opaque lit mesh, its wireframe, or its normal colors.
It consumes positions, normals and triangle indices. The editor retains UVs for
export. The Three.js route remains available for scene graphs, PBR materials,
textures and richer effects. A project can use the Rust adapter on its own canvas
beside a second renderer and share the same validated `MeshData` on the CPU.
Each graphics context needs its own GPU buffers. The mixed editor runs one
active renderer at a time.

## Use Python, JS, asset tools or a full-stack producer

```sh
python3 tools/mesh.py pawn --segments 32 -o pawn.mesh.json
python3 tools/mesh.py obj scene.obj -o scene.mesh.json
```

Open the JSON in any Model Lab. The Python commands above need only the standard
library. The editor also accepts JSON from an independent tool or service.
For example, this triangle is valid input:

```json
{"version":1,"positions":[0,0,0,1,0,0,0,1,0],"normals":[0,0,1,0,0,1,0,0,1],"uvs":[0,0,1,0,0,1],"indices":[0,1,2]}
```

| Authoring path | Connection to the renderer |
|---|---|
| Custom Rust | Implement mesh ABI v1, configure `build.wasm` and use `loadMeshGenerator`. |
| JS/TS game logic | Supply typed arrays to `validateMesh`. Reuse its result for Rust, Three.js and GLB export. |
| Python, NumPy or an offline simulator | Write JSON v1. Coordinates use Y-up metres. The file input caps each upload at 8 MiB. |
| Blender, CAD or asset-generation tools | Export a static, triangulated OBJ in Y-up metres and convert with `tools/mesh.py obj`. OBJ material libraries, textures, rigs and animation have no field in this contract. |
| Hosted backend | Fetch mesh JSON from a same-origin API and pass the parsed body to `parseMeshJson`. Run Python in a separate service or asset build step. The Next.RS server runtime executes JS route handlers in QuickJS. |

To iterate offline, run `forge dev --target single-html`. For a standalone
artifact use `forge build --target single-html`; for static hosting use
`forge build --target static`. For live API routes or multiplayer sockets use
`forge build --target server && forge start`. Hosted builds fetch local hashed
WASM assets. The portable artifact embeds both module bytes and shader sources
and can open a user-selected JSON file without network access.

### Bring a GLB from another tool

An optional Python adapter accepts the same static GLB layout that this editor
exports. It extracts a single mesh with one triangle primitive, float32
positions and normals, and optional UV0. Apply node transforms before export.
There must be one scene with one root mesh node, embedded binary data, no
external URIs, skins, animations, morphs or extensions. Output contains
geometry only. Texture images and PBR materials stay in the original GLB.
The converter bounds input and output to 8 MiB each, metadata to 256 KiB, and
geometry to 100,000 vertices and 100,000 triangles.

```sh
python3 -m venv .venv && . .venv/bin/activate
python -m pip install pygltflib==1.16.5
python tools/mesh.py glb prop.glb -o prop.mesh.json
```

This is also an interchange route for generated assets. For example, Tripo can
supply a GLB. Check its actual scene against the subset above or bake a static
triangulated OBJ in a DCC tool. Textured or animated assets can instead stay in
their original GLB for a Three.js game using GLTFLoader. No credentials or
service calls are involved in Model Lab. [Tripo output format](https://developers.tripo3d.ai/en/docs/quick-start)
and [Three.js GLTFLoader](https://threejs.org/docs/pages/GLTFLoader.html).

## Why the prototype uses direct WebGL2 bindings

| Option | GPU paths and Safari implications | Build and bundle implications |
|---|---|---|
| Existing Three.js | WebGPU with WebGL2 fallback. Preserves existing materials, controls and scenes. | Working single-file route. The editor ships the Three.js code that it uses. |
| Rust + narrow browser imports (implemented) | WebGL2. Runtime checks actual context creation. Works independently of WebGPU availability. | Standard Cargo cdylib and two imported host functions. Uses the existing URL/embedded-byte path; no generated binding step. |
| `web-sys` + `wasm-bindgen` | Typed WebGL2 browser API; WebGPU requires its own pipeline. | Feature-gated bindings can be small. Package the matching generated JS and WASM and initialize from embedded bytes; current Forge does not produce the glue automatically. |
| `glow` or `three-d` | `glow` shares GL calls between native and WebGL; `three-d` adds cameras, rendering and materials on GL/WebGL2. | Useful when sharing a native GL renderer or needing scene facilities. Their browser bindings and chosen features must be linked, packaged and measured. |
| `wgpu` | Browser WebGPU, optional WebGL2/GLES backend, plus native graphics APIs. Select and exercise both backends explicitly. | Strong candidate for a shared native/browser game renderer. Wider feature surface and binding integration; measure the linked binary and glue for the chosen scene before selecting a size budget. |

WebGL2 was enabled by default in Safari 15. Safari 26 introduced WebGPU on
iOS. WebGPU also requires a secure context and a usable adapter. Browser API
availability alone does not establish that a complete build works on a given
phone. Compare the byte size of the actual generated HTML for each profile;
include shared UI, loader, modules and editor in every measurement.

- [Three.js WebGPURenderer](https://threejs.org/docs/pages/WebGPURenderer.html)
- [wasm-bindgen WebGL example](https://wasm-bindgen.github.io/wasm-bindgen/examples/webgl.html)
- [Initializing wasm-bindgen without a bundler](https://wasm-bindgen.github.io/wasm-bindgen/examples/without-a-bundler.html)
- [glow](https://github.com/grovesNL/glow) and [three-d](https://github.com/asny/three-d)
- [wgpu features and supported backends](https://docs.rs/wgpu/latest/wgpu/)
- [Khronos WebGL Working Group Safari update](https://www.khronos.org/assets/uploads/developers/presentations/WebGL_WG_Updates.pdf)
- [WebGPU secure-context requirement](https://developer.mozilla.org/en-US/docs/Web/API/WebGPU_API)

## Configure Rust compilation

In `forge.config.json`:

```json
{
  "build": {
    "wasm": [
      {
        "name": "mesh",
        "manifestPath": "rust/mesh/Cargo.toml",
        "features": [],
        "defaultFeatures": true
      }
    ]
  }
}
```

`name` is a unique lowercase alias matching `[a-z][a-z0-9_-]*`, at most 64
characters. `manifestPath` is a relative path to a package's `Cargo.toml`, inside
the project and outside its output directories. Point to a concrete package,
rather than a virtual workspace manifest. `features` and `defaultFeatures` are
optional; their defaults are an empty list and `true`.

The package must produce a library:

```toml
[lib]
crate-type = ["cdylib", "rlib"]

[profile.release]
opt-level = "s"
lto = true
panic = "abort"
```

`cdylib` produces the browser module. `rlib` also permits native Rust unit tests.
Commit `Cargo.lock`; for a new crate, create it with
`cargo generate-lockfile --manifest-path rust/mesh/Cargo.toml`.

Forge runs Cargo with `--locked --lib --release --target wasm32-unknown-unknown`
before checking or bundling configured modules. This applies to `forge check`,
build/dev and run/test. It reads Cargo's artifact messages, so a custom library
name is supported. Cargo's incremental outputs live under
`<outDir>/wasm-target`, and the imported artifact is `<outDir>/wasm/<name>.wasm`.
Cargo reuses unchanged compilations. A failed compilation fails the Forge build;
Forge does not substitute an earlier successful module. A compilation has a
ten-minute timeout.

Cargo executes trusted build inputs, including build scripts and procedural
macros, with its normal host permissions. The server's QuickJS sandbox does not
apply to the build toolchain. Manifest paths that resolve outside the project and
symlinks in the compiler's output directories are rejected.

## Import the compiled module

```ts
import wasmUrl from "forge:wasm/mesh";
import { loadMeshGenerator } from "forge:mesh";

const generator = await loadMeshGenerator(wasmUrl);
const model = generator.generate({
  kind: "rock",
  detail: 48,
  seed: 17,
  scale: 1,
  variation: 0.5,
});
```

`forge:wasm/<name>` exports a URL string. Server and static builds use a hashed
local asset URL. Single-HTML builds use an embedded data URL, which
`loadMeshGenerator` decodes directly without calling `fetch`. Configured aliases
receive exact TypeScript declarations in `.forge/types/forge-wasm.d.ts`; include
the whole `.forge/types` directory in `tsconfig.json`.

The alias can package any compatible Rust `cdylib`. `forge:mesh` is specifically
the loader for the versioned mesh ABI below. A different Rust library may need its
own JavaScript adapter or wasm-bindgen glue; Forge does not invent that adapter.

## Use the geometry with Three.js

```ts
import { BufferAttribute, BufferGeometry, Mesh, MeshStandardMaterial } from "three";

const geometry = new BufferGeometry();
geometry.setAttribute("position", new BufferAttribute(model.positions, 3));
geometry.setAttribute("normal", new BufferAttribute(model.normals, 3));
geometry.setAttribute("uv", new BufferAttribute(model.uvs, 2));
geometry.setIndex(new BufferAttribute(model.indices, 1));
geometry.computeBoundingSphere();

const material = new MeshStandardMaterial({ color: 0x82a794, roughness: 0.7 });
const object = new Mesh(geometry, material);
scene.add(object);
```

On replacement or unmount, remove the object and dispose its geometry and
material. Dispose the generator when generation is no longer needed. Previously
returned arrays remain usable after another generation or `generator.dispose()`.
The example's GLB export contains the generated geometry; it does not require the
Rust generator to view the exported model. Its small synchronous writer handles
indexed positions, normals, UVs and one PBR material. It exports the original
mesh orientation, excluding preview rotation, the grid and debug shading. It is
an example-specific mesh exporter, not a general scene or animation exporter.

## Mesh parameters and validation

| Option | Accepted values | Default |
|---|---|---|
| `kind` | `sphere`, `torus`, `rock`, `terrain` | `torus` |
| `detail` | Integer 8–128 | 48 |
| `seed` | Integer 0–4,294,967,295; affects rock and terrain | 17 |
| `scale` | Finite number 0.01–100, in metres | 1 |
| `variation` | Finite number 0–1; tube thickness or surface variation | 0.5 |

The result contains `Float32Array` positions/normals/UVs, `Uint32Array` indices,
vertex and triangle counts, and axis-aligned bounds. `validateMesh(attributes)`
can also validate supplied arrays without copying them. It checks finite values,
matching attribute lengths, unit normals, index bounds and nondegenerate
triangles. Invalid input throws a descriptive error.

The loader accepts a URL, `ArrayBuffer` or `Uint8Array`, plus an optional
`{ signal }`. Module downloads are bounded to 16 MiB. ABI results are bounded to
one million vertices and six million indices before copying. The example's
detail limit produces much smaller meshes. `generator.memoryBytes` exposes the
WASM memory size for diagnostics; the allocator can retain a high-water mark
between generations.

## Mesh ABI version 1

The example exports linear memory and these scalar functions:

| Export | Meaning |
|---|---|
| `mesh_abi_version()` | Returns `1` |
| `mesh_generate(kind, detail, seed, scale, variation)` | Replaces the current mesh; returns a status code |
| `mesh_vertex_count()`, `mesh_index_count()` | Number of vertices and indices |
| `mesh_positions_ptr()`, `mesh_normals_ptr()` | Aligned pointers to `vertexCount × 3` packed `f32` values |
| `mesh_uvs_ptr()` | Aligned pointer to `vertexCount × 2` packed `f32` values |
| `mesh_indices_ptr()` | Aligned pointer to `indexCount` packed `u32` values |
| `mesh_clear()` | Releases the currently owned Rust mesh |

Kind codes are sphere `0`, torus `1`, rock `2`, terrain `3`. Generate status `0`
means success; `1` unknown kind; `2` invalid detail; `3` invalid scale; `4` invalid
variation; `5` allocation failure. The crate validates input before allocation.

Rust owns the buffers. JavaScript obtains `memory.buffer` after generation,
validates each pointer/count span and copies the data before a later Rust call can
grow memory. No Rust `Vec`, reference, string or struct layout crosses the ABI.
The baseline uses no WASM threads, shared memory or cross-origin isolation.

## WebGPU with WebGL2 fallback

`createRenderer` from `forge:game` coordinates caller-supplied factories:

```js
import { WebGLRenderer } from "three";
import { WebGPURenderer } from "three/webgpu";
import { createRenderer } from "forge:game";

const selection = await createRenderer({
  webgpu: () => new WebGPURenderer({ antialias: true }),
  webgl: () => new WebGLRenderer({ antialias: true }),
  backendOf: (renderer) => renderer.backend?.isWebGPUBackend ? "webgpu" : "webgl",
});
const { renderer, backend, fallbackReason } = selection;
```

The helper awaits initialization, disposes a failed candidate and then attempts
WebGL. Factories should create their own canvases: a canvas that already acquired
one context type cannot simply be reused for a different type. `prefer: "webgl"`
forces the fallback path; `prefer: "webgpu"` attempts the WebGPU factory even if
feature detection is absent. `backendOf` reports a renderer's internal fallback,
such as Three.js switching its own backend. Both failures produce an
`AggregateError`; cancellation disposes late results after initialization settles.

Three.js's WebGPU renderer uses a different material/postprocessing system.
Existing `ShaderMaterial`, `RawShaderMaterial`, `onBeforeCompile` customizations
and `EffectComposer` effects are not interchangeable with it. Orb Runner and
Arena retain their GLSL-compatible WebGL renderer. The models example uses
materials compatible with the new renderer.

## Mobile controls and adaptive resolution

`forge:game` keeps `loop`, `input` and `fit`, and adds independent control helpers:

```js
import { joystick, actionButton, gamepad, adaptiveQuality, fit } from "forge:game";

const stick = joystick(stickElement, { deadZone: 0.12 });
const action = actionButton(actionElement, { keys: ["Space"] });
const pad = gamepad({ deadZone: 0.15 });
const quality = adaptiveQuality({
  minDpr: 1,
  maxDpr: 2,
  onChange: () => stopFit.refresh(),
});
const stopFit = fit(canvas, resize, { maxDpr: () => quality.dpr });

// In the simulation update:
pad.poll();
const moveX = stick.x || pad.x;
const moveY = stick.y || pad.y;
const boosting = action.down || pad.down(0);

// Before rendering, using measured milliseconds since the previous frame:
quality.sample(frameMs);
action.endFrame();
```

Joystick coordinates are normalized to `[-1, 1]`, with Y pointing down. One finger
owns a stick while other fingers can operate action buttons. Buttons track their
pointers and keys independently. Cancellation, lost capture, blur and hidden
state clear the appropriate input; every helper has cleanup. Use a semantic
button for actions and provide keyboard equivalents. Gamepad availability is
detected at runtime and denied access is handled as disconnected input.

Adaptive quality changes the DPR cap only after complete sample windows and a
cooldown. Separate slow and fast thresholds prevent constant oscillation. Feed it
real rendered-frame durations, rather than the fixed simulation timestep.
Hidden-tab and very long suspension samples are ignored. Dispose controls,
quality sampling and canvas resizing when leaving the page.
Call `fit`'s refresh method when the quality cap changes, rather than resizing
the graphics buffer on every frame.

## iOS and verification limits

Rust WASM and graphics support are separate requirements. WebGPU is an optional
path: Safari 26 introduced it, while the WebGL2 fallback serves browsers without
a usable WebGPU adapter. Feature detection must include successful renderer
initialization; `navigator.gpu` alone is insufficient.

Compressed single-HTML output also needs `DecompressionStream`; Safari added
Compression Streams in 16.4. Setting `build.compress` to `false` removes this
particular requirement, but does not by itself certify every older browser.
For iPhone distribution, serve the single file over HTTPS and test that URL in
Safari. A downloaded HTML opened in a file preview is not equivalent to a normal
browser tab.

CI runs the model checks in Chromium and Playwright WebKit, including offline
single-file rendering, controls, resize, fallback and failure behavior. WebKit
automation is engine coverage, not physical iPhone performance or thermal
testing. Hardware WebGPU coverage is reported separately from the WebGL2 path;
a missing hardware adapter is never counted as a WebGPU render pass.

For a physical-device check, record the iPhone model, iOS build and Safari version.
Serve the built HTML over HTTPS, select each available renderer, and note the
backend shown. Try maximum-detail terrain, a two-finger pinch, both orientations,
background/resume, a Python mesh import and GLB export. For performance numbers,
record frame-time percentiles and the adaptive drawing-buffer resolution over a
stated interval, including sustained use. A successful Linux WebKit run does
not supply these device measurements.

## Verify changes

```bash
rustup target add wasm32-unknown-unknown
cargo test --locked --features e2e --test wasm
cargo test --locked --manifest-path examples/model-lab/rust/mesh/Cargo.toml
npm ci --prefix examples/model-lab
node --test tests/js/*.test.mjs
cargo build --locked
node tests/browser/models.mjs target/debug/forge
```

To exercise the new renderer crate and both new templates:

```sh
cargo test --locked --manifest-path examples/model-lab/rust/renderer/Cargo.toml
node --test examples/model-lab/tools/mesh.test.mjs
for profile in models models-rust models-mixed; do
  for engine in chromium webkit; do
    FORGE_MODELS_TEMPLATE="$profile" FORGE_MODELS_BROWSER="$engine" \
      node tests/browser/models.mjs target/debug/forge || exit 1
  done
done
```

Use the already installed Playwright package and Chromium/WebKit runtimes. The
browser runner fails if a required engine or WebGL2 context is missing; it does
not silently turn a rendering test into a DOM-only pass.

## Primary references

- [Rust browser target](https://doc.rust-lang.org/rustc/platform-support/wasm32-unknown-unknown.html)
- [Cargo JSON artifact messages](https://doc.rust-lang.org/cargo/reference/external-tools.html#json-messages)
- [Three.js WebGPU renderer and migration constraints](https://threejs.org/manual/pages/webgpurenderer)
- [WebAssembly memory growth and detached views](https://developer.mozilla.org/en-US/docs/WebAssembly/Reference/JavaScript_interface/Memory/grow)
- [WebKit features in Safari 26](https://webkit.org/blog/17333/webkit-features-in-safari-26-0/)
- [WebKit features in Safari 16.4](https://webkit.org/blog/13966/webkit-features-in-safari-16-4/)
- [Playwright browser builds and Safari differences](https://playwright.dev/docs/browsers)

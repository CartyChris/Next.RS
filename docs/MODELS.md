# Rust models, mobile input and browser rendering

Next.RS can compile Rust into browser WebAssembly, generate procedural meshes and
ship the viewer as one offline HTML file. Rust calculates vertices, normals, UVs
and triangle indices. Three.js turns those arrays into GPU geometry and presents
it through WebGPU or WebGL2. The Rust source is compiled during the build; the
browser receives the resulting executable module, not Rust source text.

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

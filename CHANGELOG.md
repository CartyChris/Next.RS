# Changelog

## 2026-11-26 — Rust rendering and portable mesh authoring

- Added `forge new --template models-rust`. A dependency-free Rust/WASM renderer drives WebGL2 through two browser imports. It draws one indexed mesh with lighting, wireframe, or surface-normal shading.
- Added `models-mixed` to switch between Rust and Three.js while retaining the current mesh, pause state, and inspection settings. Each renderer reframes its camera. The existing `models` template continues to use Three.js with WebGPU and WebGL2 fallback.
- Kept mesh ABI v1 and the shared controls, adaptive resolution, and GLB exporter. Added validated mesh JSON import for JS/TS, Python, and server-produced geometry. The Python tools generate a chess pawn, convert triangulated OBJ, and optionally extract geometry from a bounded, single-mesh static GLB.
- Added both templates to Forge's CLI and MCP template catalog. All three model profiles build for the Next.RS server, static hosting, and offline single-HTML distribution. The measured standalone files were 209,235 bytes for Rust, 399,380 bytes for Three.js, and 428,200 bytes for the combined editor.
- Expanded CI to exercise all three templates in Chromium and WebKit. Local verification passed 132 framework tests, seven native mesh/renderer tests, 40 JavaScript and Python integration tests, 50 browser check groups, and eight static-host cases. Physical iPhone performance and hardware WebGPU remain unmeasured.

See the [renderer and authoring guide](docs/MODELS.md) for the supported asset subset, API examples, and reproduction commands.

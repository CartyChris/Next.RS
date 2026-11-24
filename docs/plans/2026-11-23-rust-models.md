# Rust models and browser compatibility

## Scope and completion criteria

Chris requested a complete, committed upgrade on `main`, including Rust/WASM that
generates 3D models in a single HTML file. Preserve the existing framework,
templates, GLSL games and offline exports. This is browser-side Rust mesh
generation; it does not implement the separate, planned `wasm-host` server target.

The implementation is complete when:

- An explicitly configured Rust library is compiled for `wasm32-unknown-unknown`
  by Forge, with locked dependencies and actionable failures.
- `forge:wasm/<name>` imports work in server, static and single-HTML builds. A
  single-HTML export embeds the real compiled module and needs no asset network.
- `forge:mesh` validates the mesh ABI and returns owned positions, normals, UVs
  and indices. The example exercises procedural geometry and a useful export.
- WebGPU initialization can fall back to WebGL2, while existing custom GLSL and
  bloom examples retain their compatible renderers.
- Independent touch controls, gamepads, adaptive resolution and cleanup have
  tests that cover cancellation, failure and disposal as well as normal use.
- Rust checks, existing regressions and Chromium/WebKit model tests run. Browser
  engine evidence is distinguished from unperformed physical iPhone testing.
- Documentation, scaffold, types and MCP documentation agree with the shipped
  API. The reviewed changes are committed and pushed without overwriting remote
  work.

## Stages and failable checks

1. **Baseline and scope** → this plan plus the inspected `main` revision.
   Check: repository identity, clean baseline and actual source reads.
2. **Architecture and research** → documented WASM ABI and browser strategy.
   Check: primary Rust/Three.js/browser documentation and prototype compilation.
3. **Implementation** → build integration, runtime modules and model template.
   Check: configuration and runtime tests, real Rust-generated meshes, invalid
   input and initialization failure tests.
4. **Integration verification** → command results and browser assertions.
   Check: formatting, strict Clippy, Rust tests, JavaScript tests, existing smoke
   tests and offline model rendering in Chromium and WebKit.
5. **Review and delivery** → final review record and commits on `main`.
   Check: independent review, clean diff, remote ancestry and remote commit SHA.

## Architecture decisions

- Rust owns procedural mesh generation. JavaScript passes copied typed arrays to
  Three.js for GPU presentation and controls. A synchronous example-specific GLB
  writer serializes the mesh arrays and its PBR material.
- A small versioned ABI avoids generated glue and a JavaScript package toolchain
  inside the Rust crate. Buffers are copied after generation; callers do not
  retain views that Rust memory growth can invalidate.
- Cargo compilation is opt-in through `build.wasm`. Ordinary Next.RS projects
  keep their existing build requirements. Configured Cargo crates are trusted
  build inputs and execute with Cargo's normal host permissions.
- WebGPU is optional. Existing ShaderMaterial/EffectComposer games cannot be
  migrated by replacing a constructor. The new example uses compatible material
  APIs and explicitly handles fallback to WebGL2.
- Build caches live under the configured Forge output directory. Cargo uses a
  dedicated target directory to avoid recursive rebuilds from crate artifacts.

## Ownership

- Root: configuration, Rust compiler integration, aliases, scaffold, docs,
  end-to-end integration, final review and commits.
- Mesh agent: dependency-free Rust crate, `forge:mesh`, model example and tests.
- Runtime agent: compatible `forge:game` upgrades, public types and unit tests.
- Browser agent: Chromium/WebKit tests and CI, including offline checks.

## Work log

- Baseline: `main` at `4bc6cb8c8d2142fd30e4bed641da23304ab339d2` (merged
  prior framework work). Repository instructions and relevant build/runtime
  sections read before implementation.
- Installed an isolated Rust 1.100.0 toolchain, rustfmt, Clippy and the
  `wasm32-unknown-unknown` target. Baseline `cargo test --locked` started.
- Existing single-HTML asset handling already supports `.wasm` and its CSP
  already permits WebAssembly compilation. These mechanisms will be reused.
- Confirmed the Three.js WebGPU renderer's shader/postprocessing differences;
  the existing game renderers will be preserved.
- Implemented locked Cargo compilation, real artifact discovery, exact typed
  aliases, path checks, all-target asset integration and the `models` scaffold.
- Added the dependency-free Rust generator and a versioned, validated ABI with
  owned JS arrays. All four generators produce real geometry and GLB exports.
- Added reusable mobile/gamepad/adaptive-quality helpers and integrated the
  controls into Orb Runner while preserving its original GLSL renderer.
- Regression tests first demonstrated that browser-only code edits did not change
  the reload identifier. Identifiers now include client/CSS/asset bytes and
  configuration; a Rust-only edit is also covered independently.
- Independent review confirmed that custom output directories could retrigger
  the watcher. Both dev targets now update their output filter before rebuilding,
  including after configuration changes; a live regression test checks settling
  and subsequent source edits.
- The original pack network test was blocked by automatic approval review: its
  private-network opt-in reused a fixture containing a cloud metadata address.
  The allowed case now uses only its own local fixture; strict rejection checks
  remain. No metadata access is required by the corrected test.
- Native debug artifacts exhausted the workspace during an initial full check.
  Removed generated build outputs and reran with debug symbols/incremental
  compilation disabled; no source changes were lost.
- Review fixed actual async export and stale-startup cleanup races, caller-owned
  Node Buffer snapshot semantics, and an invalid disabled seed field. Startup
  status/errors are placed before the preview so failure is visible immediately.
- WebKit's test-runner offline emulation was independently shown to reject local
  Blob/FileReader operations. Its tests therefore block and assert zero network
  requests throughout, without that emulation. Chromium also uses offline mode.
- The final WebKit export checks exposed an uncaught error in Three.js's
  asynchronous FileReader export path. Replaced it with a synchronous numeric GLB
  writer. Binary structure, loader interoperability, normal exports and export
  followed immediately by navigation now pass in both browser engines.

## Verification record

Final local checks completed on 2026-11-24 UTC:

| Check | Result |
|---|---|
| `cargo fmt --all --check`, plus the separate mesh crate | Passed |
| `cargo clippy --locked --all-targets --features e2e -- -D warnings` | Passed |
| `cargo test --locked --features e2e` | 131 passed, zero failures or ignored tests |
| Native mesh crate tests and strict Clippy | 4 tests passed; Clippy passed |
| `node --test tests/js/*.test.mjs` | 31 passed: game helpers, real WASM boundary, GLB encoder/import |
| Fresh `forge check` and TypeScript checks for Model Lab | Passed |
| Arena and starter TypeScript checks; starter `forge test` | Passed; 2 starter tests passed |
| Existing Chromium browser regression suite | 62 checks passed |
| Model matrix in Chromium 151.0.7922.34 | 5 scenarios passed |
| Model matrix in Playwright WebKit 26.5 | 5 scenarios passed |
| Independent focused GLB/scaffold/CI review and `git diff --check` | No confirmed blocker; passed |

The five browser scenarios cover runtime input/visibility/resize/fallback, hosted
WASM with the default CSP, offline model editing/export, mobile viewport/touch/DPR
behavior, and a corrupt-WASM startup failure. Rendering checks require actual
WebGL pixels. Every generated shape exports a GLB that the real Three.js loader
reopens; geometry hashes agree across the two engines. Offline scenarios observe
zero HTTP requests, and export followed immediately by reload produces no uncaught
error. Orb Runner's existing suite also exercises simultaneous native touch input.

The final single-file Model Lab is 398,057 bytes (388.73 KiB). Its SHA-256 is
`f5f1867abc3ca972979f28e59e0729b7bbb2be770dc78274b7718d5fc4578992`.
The distribution directory contains only `index.html`; Rust WASM, JavaScript and
styles are embedded. Build outputs and browser evidence remain outside git.

## Review and remaining limitations

Independent reviews of the compiler/configuration/build-ID path, watch filtering,
mesh ABI, app lifecycle and helper documentation are complete. Their confirmed
findings were corrected with targeted regression coverage. The final export
writer, its integration, scaffold and CI also received an independent read-only
review. No physical iPhone
session is available; WebKit automation is engine coverage, not device
certification. Hardware WebGPU rendering remains separate from WebGL2 and
renderer-selection/failure checks.

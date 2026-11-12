# Status

Where each milestone of the original design ([handoff](../handoff/README.md)) stands in this repository. "Verified" means covered by tests in `src/`, `tests/` or `tests/browser/smoke.mjs`.

Legend: ✅ implemented and verified · 🟡 partial · ❌ not yet · ⤳ replaced by a different design

## Summary

| | Handoff assessment | Now |
|---|---|---|
| Compiling crate, `cargo fmt/check/test` | not run | ✅ fmt, clippy `-D warnings`, 60 Rust tests |
| Rolldown e2e | blocked | ✅ in-process, every build |
| SSR app served end to end | not done | ✅ streaming React SSR + hydration in Chromium |
| `/_forge/data` | design | ✅ |
| Single-file HTML | fixture logic | ✅ React, JSX, three.js, vanilla; verified from `file://` |
| Static export | design | ✅ |
| WASM-host target | skeleton | ❌ (reports "planned") |

## Milestones

| # | Milestone | Status | Where / notes |
|---|---|---|---|
| 1 | V8 interop bridge | ⤳ ✅ | QuickJS instead of V8 (`runtime/host.rs`): engines owned by worker threads, never shared; JSON request contract; async loop; strings and bytes back to Rust. Rationale in ARCHITECTURE.md |
| 2 | Async server + file router | ✅ | `server/mod.rs`, `routes.rs` |
| 3 | Native TS pipeline | ⤳ ✅ | Rolldown + Oxc in-process (`build/bundle.rs`) |
| 3B–3E | Build hardening | 🟡 | Capsule ✅ (`build/manifest.rs`), module confinement via import policy ✅. Custom vendoring and CJS lowering dropped: Rolldown handles CJS |
| 4 | Client hydration bridge | ✅ | `hydrateRoot` with boot JSON, modulepreload per route, `ErrorBoundary` export, works without JS, lazy route chunks |
| 4B | Navigation, prefetch, metrics | 🟡 | Link interception, history, hover/focus/touch prefetch ✅. Viewport prefetch, performance marks, adaptive hydration ❌ |
| 4C | Route data protocol + layout graph | ✅ | `/_forge/data`, `{data}` / `{notFound}` / `{redirect}`, client dedupe cache, layouts per route in the manifest |
| 4D | Persistent layouts | 🟡 | Layout state persists across navigation ✅. Layout-level `Loading`/`ErrorBoundary` ❌ (page-level ✅) |
| 5A | Streaming transport | ✅ | chunked responses from the worker |
| 5B | React Web Streams | ✅ | `renderToReadableStream` on the host's `ReadableStream` |
| 5C | Op-backed push streaming | ✅ | `write` op into a per-request channel; zero-copy `pipe` for upstream fetch bodies |
| 5D | Cancellation, stream hardening | 🟡 | Disconnect stops rendering ✅, backpressure (1 MiB high-water) ✅, host ops unreachable from app code ✅. `AbortSignal` into loaders ❌ |
| 5E | Suspense-aware streaming | ✅ | fallbacks stream first, crawlers get `allReady`, per-route `streaming = false` |
| 6 | Data model + resource cache | 🟡 | `forge:data` (`useData`, `getData`, `preloadData`) ✅, process cache `forge:cache` ✅. Request-scoped cache ❌ |
| 6B | Loader execution | ✅ | `loader(ctx)` with params, url, request, env |
| 6C | Serialization, redaction, ETag | 🟡 | ETag + 304 ✅, production error redaction ✅. Non-JSON values fail the request rather than a typed error; redaction hook ❌ |
| 6D | Loader cancellation + tests | 🟡 | Request timeout → 504 ✅, tests ✅. Cooperative cancellation ❌ |
| 7 | Sandbox, timeouts, isolate hardening | 🟡 | Heap limit, CPU interrupt, timeouts, recycle ✅ (tested). Internal-import and eval policy ✅. Dependency capability policy and structured audit log ❌ |
| 7B | Worker supervisor | 🟡 | Least-loaded dispatch, recycle on fatal error, liveness/readiness ✅. Crash-loop backoff is a fixed 1 s retry. Process-per-worker and cgroups ❌. Egress allowlist ✅ |
| 8A | Persistent cache + tags | 🟡 | In-memory TTL + tags ✅, page cache with SWR ✅, in-flight dedupe ✅. Disk/Redis backend ❌ |
| 8B | Rolldown pivot, targets, `forge check` | ✅ | server, static, single-html, `forge check`, AST security scan |
| 8C | Rolldown backend + target matrix | ⤳ ✅ | No `BundlerBackend` trait or generated JS config: one in-process backend. Security, alias and manifest "plugins" are one Rust plugin |
| 8D | Migration patch | ✅ | Every fixture module ported or superseded (below) |

## The 8D fixture, file by file

| Fixture file | Became |
|---|---|
| `build/config.rs` | `src/config.rs` (JSON config, targets, modes, env) |
| `build/bundler.rs` | `src/build/bundle.rs` (`BundleSpec` → `Bundle`, no trait needed) |
| `build/rolldown.rs` | `src/build/bundle.rs` (Rolldown crate + `ForgePlugin`; no subprocess, no generated `rolldown.config.mjs`) |
| `build/generated.rs` | `src/build/generated.rs` (real runtime, real server/client entries, route modules) |
| `build/manifest.rs` | `src/build/manifest.rs` (same capability matrix, plus routes, preload map, capsule, stats) |
| `build/security_scan.rs` | `src/build/analyze.rs` (AST-based: no false positives from strings/comments, finds indirect eval, reports positions) |
| `build/single_html.rs` | `src/build/html.rs` (`SingleFile`, prerendered body, optional compression, case-insensitive `</script` escaping) |

Fixture test list from the handoff, and where each is covered now:

| Fixture check | Test |
|---|---|
| graph emits server/client entries | `build::generated::tests::emits_server_and_client_entries` |
| relative import generation | `build::generated::tests::relative_imports` |
| security scan denies `eval` / `new Function` / internal imports | `build::analyze::tests::*`, `tests/build.rs::security_policy_is_enforced_at_build_time` |
| single-file HTML escapes `</script>` | `build::html::tests::inline_script_escaping`, `tests/build.rs::single_html_inlines_assets_and_escapes_script_terminators` |
| manifest capabilities per target | `build::manifest::tests::capabilities_per_target` |
| output file classifier | not needed: Rolldown reports chunks and assets as typed values |
| bundle manifest maps route entry to output | `tests/build.rs::server_build_writes_the_expected_layout` (preload map) |

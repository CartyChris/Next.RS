# Milestones Worked So Far

## Milestone 1 — V8 Interop Bridge

Designed the initial `FrameworkRenderer` concept:
- Rust backend runtime using `deno_core` / V8
- JS SSR function contract
- Request context serialization
- Async event-loop driving
- String extraction back into Rust
- V8 isolate/thread-safety discussion

Key architectural decision:
- Do not share one `JsRuntime` across Axum worker threads.
- Use renderer workers/pool where each worker owns its isolate.

## Milestone 2 — Async Server & File Router

Designed:
- Axum/Tokio server
- compile-time app route scan
- SSR fallback
- static serving
- request context construction
- renderer pool integration

## Milestone 3 — Native TS/SWC/Oxc Pipeline

Designed:
- native Rust-first build strategy
- Oxc/SWC transform boundaries
- dependency policy
- capsule verification
- client/server graph split

Later superseded for production by Rolldown/Oxc bundling in Milestone 8.

## Milestone 3B/3C/3D/3E — Build Hardening

Designed:
- dependency vendoring concepts
- capsule verification
- module loader confinement
- native graph policy
- CJS caveats

Later adversarial review concluded:
- keep capsule, manifest, dependency policy
- replace custom vendoring/CJS rewriting with Rolldown production backend

## Milestone 4 — Client Hydration Bridge

Designed:
- generated client runtime
- `hydrateRoot`
- hydration data schema
- modulepreload hints
- route-level error boundary
- no-JS fallback
- lazy route hydration

## Milestone 4B — Client Navigation, Prefetch, Metrics

Designed:
- same-origin link interception
- `history.pushState` / `popstate`
- route prefetch
- hover/focus/viewport prefetch
- performance marks/measures
- dev overlay hooks
- adaptive hydration strategy contract

## Milestone 4C — Route Data Protocol + Layout Graph Foundation

Designed:
- `/_forge/data`
- route-data JSON schema
- client navigation fetching fresh data
- route data cache
- search/params serialization
- first layout graph manifest

## Milestone 4D — Persistent Layout Rendering

Designed:
- `layout.tsx` compilation model
- route-to-layout chain mapping
- server layout composition
- client layout composition
- stable layout keys for preservation
- layout loading/error export contract

## Milestone 5A — Streaming SSR Transport

Designed:
- `RenderMode`
- chunked HTML transport streaming
- Axum `Body::from_stream`
- renderer pool stream jobs
- fallback non-streaming path

Adversarial clarification:
- 5A is transport streaming, not true React Suspense streaming.

## Milestone 5B — React Web Streams Capability Probe

Designed:
- runtime capability probe
- `ReactReadableStream` render mode
- fallback matrix:
  - ReactReadableStream
  - ChunkedHtml
  - String
- startup validation
- render-mode CLI flags

## Milestone 5C — deno_core Op-Backed Push Streaming

Designed:
- `deno_core` Extension with async ops
- JS reads React `ReadableStream`
- JS pushes chunks into Rust bounded channel
- backpressure propagation into React stream
- Axum streams receiver

## Milestone 5D — Cancellation, AbortSignal, Stream Hardening

Designed:
- no global stream write API
- token-protected stream ops
- per-stream cancellation flag
- body-drop cancellation
- JS cancellation checks
- React `AbortController.signal`
- first-chunk preflight
- stream error marker policy

## Milestone 5E — Suspense-Aware Streaming APIs

Designed:
- `Loading` export as Suspense fallback
- layout/page Suspense boundaries
- `StreamingReadiness::{ShellReady, AllReady}`
- crawler/static `allReady`
- route error boundary integration
- shell commit policy correction

## Milestone 6 — Data Fetching Model + Suspense Resource Cache

Designed:
- `forge:data`
- `loadData`, `useData`, `preloadData`, `forgeFetch`
- request-scoped cache
- data records in `__FORGE_DATA__`
- client data cache hydration
- streamed bootstrap wait fix

## Milestone 6B — Route Loader Execution for /_forge/data

Designed:
- `loader(ctx)` export
- generated `__forgeLoadRouteData`
- renderer route-data jobs
- Axum `/_forge/data` invoking renderer loader path
- `props.loader`
- `data.records`
- structured JSON errors

## Milestone 6C — Data Serialization, Redaction, Abort, Cache Hardening

Designed:
- JSON serializability guard
- reject BigInt/function/symbol/cyclic data
- safer serialization defaults
- redaction hook
- `forge:navigation` typed errors
- ETag for route data
- client SWR cache
- tag/key invalidation hooks
- production error sanitization

## Milestone 6D — Cooperative Loader Cancellation + Runtime Tests

Designed:
- request cancellation registry
- `op_forge_request_is_cancelled`
- JS request signal factory
- loader `AbortController.signal`
- Axum drop guard cancellation
- timeout cancellation
- e2e test shapes for data endpoint, notFound, redirect, JSON serializability, ETag, SWR

## Milestone 7 — Production Sandbox, Timeouts, Isolate Hardening

Designed:
- `SandboxConfig`
- V8 heap limit strategy
- isolate termination handle
- worker recycle policy
- max HTML/data/chunk/stream limits
- stream idle/total timeout
- dangerous global deny policy
- internal import validation
- dependency capability policy
- structured audit logs
- closed-worker detection

## Milestone 7B — Worker Supervisor + Process-Level Isolation Plan

Designed:
- generation-aware worker lifecycle
- supervisor-owned worker table
- health snapshots
- crash-loop backoff
- graceful drain/recycle
- liveness/readiness/detail endpoints
- process-per-renderer backend contract
- Linux cgroup plan
- network egress policy
- production deployment profile

## Milestone 8A — Persistent Server Cache + Tag Invalidation

Designed:
- persistent cache abstraction
- memory backend
- tag/path index
- TTL/SWR semantics
- stampede lock
- cache poisoning protections
- route-data cache key
- invalidation API policy

## Milestone 8B — Native Bundling Pipeline Pivot

Designed:
- switch production bundling to Rolldown/Oxc
- keep Forge manifest/capsule/runtime
- replace custom vendoring/CJS lowering
- secure app/game defaults
- single-file HTML target
- WASM-host target
- `forge check`
- AST security scanning policy
- feature-driven transform optimization

## Milestone 8C — Rolldown Backend Implementation + Target Matrix

Designed:
- `BundlerBackend` trait
- Rolldown CLI/config adapter
- generated Rolldown config
- server/browser entry grouping
- security plugin
- pre-bundle security scan
- bundle stats collection
- manifest update path
- capsule from bundle outputs
- targets:
  - server
  - static
  - single-html
  - wasm-host skeleton

## Milestone 8D — Rolldown Migration Patch Plan

Implemented as fixture:
- `build/bundler.rs`
- `build/rolldown.rs`
- `build/generated.rs`
- `build/manifest.rs`
- `build/security_scan.rs`
- `build/single_html.rs`
- `build/config.rs`
- non-Rolldown verification tests
- generated wrapper tests
- config-generation tests
- single-file escaping tests

Verified in sandbox:
- 50 non-Rolldown fixture checks passed
- Rolldown/cargo e2e were blocked by environment permissions/missing binary

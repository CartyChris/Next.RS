# Gap Register

This file lists what is still missing before this becomes a complete alpha/beta/production framework.

## Missing for integrated alpha

- Create or mount the real repository.
- Apply Milestone 8D patch files into the real crate.
- Wire the actual module tree.
- Run:
  - `cargo fmt`
  - `cargo check`
  - `cargo test`
  - `cargo clippy`
- Install/enable Rolldown.
- Run a real Rolldown build.
- Serve one route end-to-end:
  - `app/page.tsx`
  - SSR HTML response
  - browser hydration
- Verify `/ _forge/data` for one route loader.

## Missing for beta

- Complete V8 renderer implementation.
- Complete Axum server.
- Complete client runtime.
- Complete route/layout graph.
- Complete streaming SSR implementation.
- Complete route-data loader execution.
- Complete persistent cache integration.
- Complete worker supervisor implementation.
- Add starter templates.
- Add docs.
- Add CI.
- Add examples:
  - basic app
  - layout app
  - loader app
  - streaming/Suspense app
  - single-file HTML app
  - game-style app

## Missing for production

- Process-level isolation option.
- Resource limits in deployment.
- Full security audit.
- Dependency policy enforcement from bundle graph.
- E2E browser test matrix.
- Load/performance tests.
- Memory/timeout/fault-injection tests.
- Real Rolldown/Vite compatibility tests.
- Stable public APIs.
- Versioned manifest schema.
- Migration docs.
- Release packaging.

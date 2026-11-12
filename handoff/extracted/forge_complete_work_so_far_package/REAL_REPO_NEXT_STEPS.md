# Real Repository Next Steps

The actual real codebase was not mounted in the sandbox environment, so these patches were prepared and verified as a fixture.

## Apply Patch Fixture

Copy the files from `forge_8d_impl/src/build/` into the real repository and integrate them with the existing crate.

Core files:
```txt
src/build/bundler.rs
src/build/rolldown.rs
src/build/generated.rs
src/build/manifest.rs
src/build/security_scan.rs
src/build/single_html.rs
src/build/config.rs
```

## Required Integration Work

1. Add new modules to `src/build/mod.rs`.
2. Add `BuildConfig` fields:
   - `bundler`
   - `target`
   - `security`
3. Extend the real `AssetManifest`.
4. Wire `emit_generated_build_graph`.
5. Wire `RolldownBundler`.
6. Update runtime loading to use bundled ESM server entry.
7. Update capsule creation to include Rolldown output files.
8. Add `forge check`.
9. Move old vendoring/CJS rewriting under `build/legacy`.
10. Feature-gate real Rolldown e2e tests.

## Local Commands

```bash
cargo fmt --check
cargo check
cargo test
cargo clippy --all-targets --all-features -- -D warnings

forge check --production
forge build --bundler rolldown --target server --production
forge serve --production

curl /health/ready
curl /
curl '/_forge/data?path=/'

forge build --target single-html --production

cargo test --features e2e_rolldown
```

## Expected Current State

The fixture tests pass, but the production framework is not finalized until these commands pass in the real repository.

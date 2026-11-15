# Handoff package

This folder preserves, unmodified, the design and fixture package this repository was started from:

- `forge_complete_work_so_far_package_AUDITED.zip`: the original archive, byte for byte.
- `extracted/`: its contents. Start with `extracted/forge_complete_work_so_far_package/README.md` and `MILESTONES_WORKED_SO_FAR.md`.

It is kept for provenance. Nothing in it is built or executed. The working implementation lives in `src/`; how each milestone and each Milestone 8D fixture file maps onto it is in [docs/STATUS.md](../docs/STATUS.md), and the remaining work is in [docs/ROADMAP.md](../docs/ROADMAP.md).

Notable departures from the handoff design, with reasons in [docs/ARCHITECTURE.md](../docs/ARCHITECTURE.md):

- QuickJS (rquickjs) instead of deno_core/V8 for server rendering.
- Rolldown linked as a Rust library instead of a generated `rolldown.config.mjs` run through the Rolldown CLI, so neither Node.js nor a JS bundler install is needed to build.
- Regex security scanning replaced by an Oxc AST scan.

# Next.RS

Two kinds of work happen in this repository. Pick the section that fits.

## Building an app or game with Next.RS

Read [docs/AGENT_GUIDE.md](docs/AGENT_GUIDE.md): the work loop, file conventions, built-in modules, targets and pitfalls on one page. `forge new` copies it into every project as `AGENTS.md`, `forge docs` searches all the documentation offline, and `forge mcp` serves it to MCP clients.

- Interface: [docs/FORGE_UI.md](docs/FORGE_UI.md) for components and looks, [docs/DESIGN.md](docs/DESIGN.md) for the rules that keep a page from looking generated.
- Everything else: [docs/GUIDE.md](docs/GUIDE.md).
- Working apps to copy from: [starter](examples/starter) and [Relay](examples/ui-showcase), which shows every component at `/components`.

## Changing Next.RS itself

- Layout: `src/` is the `forge` binary (build, runtime, server, pack, mcp). `src/runtime/js/` is the JavaScript runtime embedded in it. `packages/forge-ui/src` is the source of `forge:ui`, also embedded. `examples/` are the `forge new` templates and the test fixtures. How the parts fit: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).
- Before finishing: `cargo fmt`, `cargo clippy --all-targets --features e2e -- -D warnings`, `cargo test`.
- When changing a public component in `packages/forge-ui`, check its types and the showcase under the server, static, and single-HTML targets:

  ```bash
  node packages/forge-ui/scripts/build-types.mjs   # regenerate forge-ui.d.ts (needs npm ci in examples/ui-showcase)
  cargo build && node tests/browser/forge-ui.mjs   # Relay on all three targets, with axe
  ```

- A new component needs: an export in `src/index.ts`, styles in `styles.css` that use only `--forge-ui-*` tokens, an entry in the gallery (`examples/ui-showcase/app/components/page.tsx`), and a line in [docs/FORGE_UI.md](docs/FORGE_UI.md).
- A new look is one entry in `packages/forge-ui/src/themes.ts`. Keep the block between the `themes:start` and `themes:end` markers valid JSON: the binary reads it for `forge themes` and the MCP theme tool.
- The documents embedded in the binary are listed in `DOCS` in `src/mcp.rs`. Keep headings descriptive; search ranks sections by their heading and text.
- Browser tests use the Playwright that is already installed. Do not add a local `playwright` to `tests/browser/node_modules`.

# Forge UI

Accessible React components and design tokens for [Next.RS](../../README.md). This folder is the source of the built-in `forge:ui` module: the `forge` binary embeds these files, so a Forge app imports the components with nothing to install.

```tsx
import { Button, Theme } from "forge:ui";
```

- [Component reference and patterns](../../docs/FORGE_UI.md)
- [Design rules](../../docs/DESIGN.md)
- Examples: [starter](../../examples/starter) and [Relay](../../examples/ui-showcase) (every component at `/components`)

## Layout of this package

| Path | Contents |
|---|---|
| `src/` | The components (TypeScript, React 19, no other dependencies). `index.ts` lists the public exports. |
| `styles.css` | Tokens and component styles, class prefix `forge-ui-`. |
| `forge-ui.d.ts` | Generated ambient types for `forge:ui`. Regenerate after changing `src/`. |
| `scripts/build-types.mjs` | Generates `forge-ui.d.ts` (`--check` verifies it is current). |

After changing a component:

```sh
(cd examples/ui-showcase && npm ci)
node packages/forge-ui/scripts/build-types.mjs
cargo build
node tests/browser/forge-ui.mjs
```

New source files must also be listed in `UI_FILES` in `src/build/generated.rs`; a test fails if one is missing.

## Using it as a package

The same sources work as the `@nextrs/ui` package in a React 19 app bundled by Forge. Install it from a checkout (`npm install --install-links ./packages/forge-ui`), import `@nextrs/ui/styles.css` once, and call `configure({ Link, href, usePathname, navigate })` to connect links to your router. Inside Forge, prefer `forge:ui`, which does all of that for you.

## Browser support

Current Chromium, Firefox and Safari. The palette uses `oklch()` and relative color syntax; overlays use the native `<dialog>` and popover APIs. Everything renders on the server and works in single-file builds.

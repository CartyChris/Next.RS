# Forge UI

Accessible React components and CSS tokens for [Next.RS](../../README.md). The package uses your app’s existing React 19 installation and Forge’s built-in TypeScript, JSX, and CSS bundling. The stylesheet and components require zero additional runtime dependencies.

Explore the [Relay showcase](../../examples/ui-showcase) or use the [agent guide](../../docs/FORGE_UI.md) to start an app.

## Install from a Next.RS checkout

From a Forge app inside the repository’s `examples/` directory, run:

```sh
npm install --install-links ../../packages/forge-ui
```

Add `install-links=true` to the app’s `.npmrc` so `npm ci` uses the same local package installation mode. For an app outside the checkout, adjust the path or make a tarball with `npm pack ./packages/forge-ui` from the checkout root and install the resulting tarball. When you change the local package, reinstall it in the consuming app.

Import the stylesheet once from the root layout and wrap your app:

```tsx
import type { ReactNode } from "react";
import { Theme } from "@nextrs/ui";
import "@nextrs/ui/styles.css";
import "./app.css";

export default function Layout({ children }: { children: ReactNode }) {
  return <Theme>{children}</Theme>;
}
```

An app can set `body { margin: 0; }` in `app.css`. The component stylesheet uses the `forge-ui-` class prefix and places document colors and typography on `Theme`. It applies no reset to the rest of the document.

## Components

| Import | Compose with |
| --- | --- |
| `Theme`, `Container`, `Stack`, `Grid` | `theme="system"` follows the device. `Container` sets page width. `Stack` gives vertical rhythm. `Grid` reflows from one column to the configured maximum. |
| `Button`, `ButtonLink` | Actions and links, respectively. Use `variant` and `size` to set hierarchy. Use `type="submit"` for a form submit. |
| `TextInput`, `Select`, `Textarea`, `Checkbox` | Pass `label` and normal HTML control props. Optional `description` and `error` associate with the field automatically. `Select` accepts native `option` and `optgroup` children. |
| `Card`, `CardHeader`, `CardTitle`, `CardDescription`, `CardContent`, `CardFooter` | Compose a content surface using the parts you need. Set `CardTitle as` to fit the page’s heading order. |
| `Nav`, `NavLink` | Set a descriptive `label` on each navigation landmark and use `current="page"` or `current="location"` for active links. |
| `Badge`, `Alert` | Label statuses with text. `Alert` can announce updates with `live="polite"` or use `live="assertive"` for urgent messages. |
| `Dialog` | Set `open`, `onOpenChange`, and `title`. Supply a short optional `description`, body content, and optional `footer` actions. |
| `Tabs` | Pass a `label` and items with unique `value`, `label`, and `content`. Use `defaultValue` or control the pair `value` and `onValueChange`. |
| `NotificationRegion` | Mount once per workspace, then pass an array of notifications with unique string IDs. Give it `onDismiss` for user dismissal. |

Named TypeScript exports include `ButtonProps`, `TextInputProps`, `DialogProps`, `TabItem`, `TabsProps`, `Notification`, and `ThemeMode`. Native element props and React 19 refs pass through on the primitives and fields. App code owns state, form submission, and notification lifetimes.

## Customize tokens

Override variables in your app stylesheet with a custom class on `Theme`. Pair foreground and background tokens to retain contrast in each palette.

```tsx
<Theme className="library-theme" theme={mode}>{children}</Theme>
```

```css
.forge-ui.library-theme {
  --forge-ui-accent: #145c4d;
  --forge-ui-accent-hover: #0f493d;
  --forge-ui-on-accent: #fff;
  --forge-ui-focus: #187460;
  --forge-ui-radius: 0.5rem;
}
.forge-ui.library-theme[data-theme="dark"] {
  --forge-ui-accent: #8ae1c9;
  --forge-ui-accent-hover: #acf0dc;
  --forge-ui-on-accent: #103c32;
  --forge-ui-focus: #9ce9d3;
}
@media (prefers-color-scheme: dark) {
  .forge-ui.library-theme[data-theme="system"] {
    --forge-ui-accent: #8ae1c9;
    --forge-ui-accent-hover: #acf0dc;
    --forge-ui-on-accent: #103c32;
    --forge-ui-focus: #9ce9d3;
  }
}
```

See [styles.css](styles.css) for the full palette, spacing, type scale, surface, radius, focus, and shadow tokens. Prefer the component’s props for variants and `className` for layout or local branding.

## Rendering and browser support

Forge renders standard markup through QuickJS for server and static builds. The single-file build bundles React, the styles, and the client interactions into its offline HTML. A modal opens after React mounts using the native `HTMLDialogElement.showModal()` API. Target modern Chromium, Firefox, and Safari with native modal dialog support. Your Forge output may have additional browser requirements. Mobile form controls keep native select and checkbox behavior.

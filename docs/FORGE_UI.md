# Forge UI

Forge UI is the component library built into the `forge` binary. Import from `forge:ui`; there is nothing to install, the stylesheet comes with the import, and links go through the Forge router. It needs React 19 in the app (`npm install react react-dom`).

- **Design rules** (what good looks like, and what to avoid): [DESIGN.md](DESIGN.md)
- **Working apps**: [starter](../examples/starter) (app shell, table, form) and [Relay](../examples/ui-showcase) (dashboard, plus a gallery of every component at `/components`)
- **Source and types**: [packages/forge-ui/src](../packages/forge-ui/src), bundled as `.forge/types/forge-ui.d.ts` in every project

```bash
forge new my-app --template starter
```

## Start

Wrap the app in `Theme` once, in `app/layout.tsx`:

```tsx
import { Theme, Toaster } from "forge:ui";
import type { ReactNode } from "react";

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <Theme accent="ember" radius="md" density="comfortable">
      {children}
      <Toaster />
    </Theme>
  );
}
```

```tsx
// app/page.tsx
import { Button, Card, CardContent, Container, PageHeader, Stack, Stat, toast } from "forge:ui";

export default function Page() {
  return (
    <main>
      <Container>
        <Stack gap="lg">
          <PageHeader title="Projects" description="Everything your team has shipped."
            actions={<Button onClick={() => toast("Created")}>New project</Button>} />
          <Card><CardContent><Stat label="Live" value="12" change="+2" trend="up" /></CardContent></Card>
        </Stack>
      </Container>
    </main>
  );
}
```

`forge pack Component.tsx` understands `forge:ui` too, so a single component file becomes one offline HTML file with the components and styles inside.

## Theme

| Prop | Values | Notes |
|---|---|---|
| `theme` | `"system"` (default), `"light"`, `"dark"`, `"inherit"` | `inherit` keeps the outer scheme, for a nested `Theme` that only changes accent or density. |
| `accent` | `"ember"` (default), `"violet"`, `"cobalt"`, `"verdigris"`, `"gold"`, `"rose"`, `"steel"`, any CSS color, or a hue `0–360` | One hue drives the accent and gives the neutrals a faint tint. Lightness is fixed per role, so text contrast stays AA for every accent. |
| `radius` | `"none"`, `"sm"`, `"md"` (default), `"lg"`, `"full"` | Corners of controls and panels. |
| `density` | `"compact"`, `"comfortable"` (default), `"spacious"` | Control height and spacing. Comfortable is 40px with a mouse and 44px for touch. |
| `tokens` | `{ font, "font-display", surface, text, radius, … }` | Sets `--forge-ui-<name>` on this `Theme`. |

`useThemeMode()` returns `[mode, setMode]` and remembers the choice in `localStorage`. All tokens are CSS variables named `--forge-ui-*` ([styles.css](../packages/forge-ui/styles.css)); use them in your own CSS (`color: var(--forge-ui-muted)`) so custom parts follow the theme.

## Components

Every component accepts `className` and the native props of its element.

**Layout**

| Component | Use |
|---|---|
| `Container size="narrow \| default \| wide \| full"` | Page width and side padding. |
| `Stack gap` | Vertical rhythm. `Row gap align justify wrap` for horizontal groups. |
| `Grid columns={1–4} gap` | Reflows to fewer columns on small screens. |
| `Divider`, `PageHeader title description actions eyebrow` | `PageHeader` renders the page's `h1`. |
| `AppShell brand nav header sidebarFooter` | Sidebar + top bar. The sidebar becomes a drawer on small screens. |
| `Card variant="outline \| raised \| ghost \| spotlight" interactive` | With `CardHeader`, `CardTitle as`, `CardDescription`, `CardContent`, `CardFooter`. |

**Text**: `Heading level size`, `Text tone size truncate`, `Code`, `Kbd`, `Icon name size label` (31 outline icons: `iconNames`).

**Actions**

| Component | Use |
|---|---|
| `Button variant="primary \| secondary \| soft \| ghost \| danger \| link" size loading iconStart iconEnd` | One primary button per view. |
| `ButtonLink href`, `NavLink href matchPrefix`, `AppLink href` | In-app paths use the router; `NavLink` sets `aria-current` itself. |
| `IconButton label`, `ButtonGroup label` | `label` is the accessible name. |
| `Menu label items`, `Popover label`, `Tooltip content` | Menu items: `{ label, onSelect, href, icon, shortcut, disabled, danger }` or `{ separator: true }`. |
| `CommandPalette open onOpenChange commands` | Pair with `useHotkey("mod+k", …)` and `useDisclosure()`. |

**Fields**: `TextInput`, `Textarea`, `Select`, `Checkbox`, `Switch`, `RadioGroup name options`, `SegmentedControl name options`, `Slider showValue`. Each takes `label` (required), `description`, `error`.

**Feedback and status**: `Alert tone title actions live`, `Badge tone dot`, `toast(message | { title, description, tone, duration })` with `<Toaster />` mounted once, `Dialog` / `Drawer` (`open`, `onOpenChange`, `title`, `footer`, `placement`, `size`), `Spinner`, `Skeleton lines`, `Progress label value`, `EmptyState title description action`.

**Data**: `Table caption columns rows rowKey` (columns: `{ key, header, cell, sortable, sortValue, align }`), `Stat label value change trend hint`, `Avatar name src`, `Pagination page pageCount onPageChange | hrefFor`, `Breadcrumbs items`, `Tabs items label`, `Accordion items multiple`, `Image src alt width height priority placeholder`.

**Games**: `Hud` over a canvas with `HudSlot position interactive panel`, and `Bar label value max tone` for health or energy.

**Hooks and helpers**: `useDisclosure()`, `useHotkey(combo, handler)`, `useThemeMode()`, `cx(...classes)`.

## Patterns

```tsx
// A form: native validation plus a specific message. Focus the first invalid field.
<form onSubmit={save} noValidate>
  <Stack>
    <TextInput name="email" type="email" label="Email" required error={error} />
    <Button type="submit" loading={saving}>Save</Button>
  </Stack>
</form>

// A dialog: state lives in the page; focus returns to the opener on close.
const confirm = useDisclosure();
<Button variant="danger" onClick={confirm.onOpen}>Delete</Button>
<Dialog open={confirm.open} onOpenChange={confirm.setOpen} title="Delete project?"
  footer={<><Button variant="secondary" onClick={confirm.onClose}>Cancel</Button><Button variant="danger">Delete</Button></>}>
  This cannot be undone.
</Dialog>

// A table from loader data.
<Table caption="Projects" rows={projects} rowKey={(p) => p.id} columns={[
  { key: "name", header: "Name", sortable: true },
  { key: "visitors", header: "Visitors", sortable: true, align: "end" },
]} />

// A game HUD over a canvas.
<div style={{ position: "relative" }}>
  <canvas ref={canvas} />
  <Hud>
    <HudSlot position="top-left" panel><Bar label="Health" value={hp} /></HudSlot>
    <HudSlot position="bottom" interactive><Button onClick={pause}>Pause</Button></HudSlot>
  </Hud>
</div>
```

Server and browser must render the same text. Numbers and dates formatted with `toLocaleString()` or `Intl` match for `en-US`; format dates with `timeZone: "UTC"` (the server has no local time zone), or format them in an effect.

## Verify

```bash
forge check                        # routes, security scan, writes .forge/types for tsc
npx tsc -p .
forge build --target single-html   # then open dist/index.html with networking off
```

Check a 320px-wide viewport, light and dark, keyboard-only use (Tab, arrows, Escape, focus returning to the opener), and form errors. The repository's own checks are `node tests/browser/forge-ui.mjs` (Relay on all three targets, with axe) and `tests/browser/smoke.mjs`.

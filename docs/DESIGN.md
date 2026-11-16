# Design rules

Rules for building interfaces with Next.RS and `forge:ui` that look considered rather than generated. They are written for AI agents and apply to people too. The component reference is [FORGE_UI.md](FORGE_UI.md).

## Use the system

1. **Import components from `forge:ui`.** Do not hand-build a button, input, dialog, menu, table or toast: the library versions handle focus, keyboard, contrast, dark mode and small screens.
2. **Wrap the app in one `Theme`** in `app/layout.tsx`. Choose `accent`, `radius` and `density` there, once.
3. **Style custom parts with tokens**, never with hard-coded colors: `var(--forge-ui-surface)`, `--forge-ui-text`, `--forge-ui-muted`, `--forge-ui-edge`, `--forge-ui-accent`, `--forge-ui-accent-soft`, `--forge-ui-space-1…7`, `--forge-ui-radius`, `--forge-ui-radius-panel`. Hard-coded hex breaks dark mode.
4. **Lay out with `Container`, `Stack`, `Row` and `Grid`.** Reach for custom CSS only for what they cannot express, in a `.css` file imported by the page.
5. **Icons come from `Icon`.** Do not use emoji as icons.

## What makes a page look professional

**Hierarchy.** One `h1` per page (`PageHeader` renders it). One primary button per view; everything else is `secondary`, `soft` or `ghost`. Destructive actions are `danger` and sit away from the primary action.

**Density and alignment.** Align to the grid the layout components give you. Numbers in tables are right-aligned (`align: "end"`). Related controls share a `Row`; unrelated groups get space, not borders.

**Restraint.** Color carries meaning: accent for the main action and the current item, tones (`success`, `warning`, `danger`, `info`) for status only. Most of the page is neutral. Borders are 1px and quiet; shadows are for things that float (menus, dialogs, toasts).

**Words.** Labels name the thing ("Workspace name"), buttons name the action ("Save changes", not "Submit" or "OK"), errors say how to fix the problem. Use sentence case. No exclamation marks, no filler ("Welcome to your dashboard!").

**Real content.** Use plausible data in examples: names, dates, numbers with units. Empty lists get an `EmptyState` with one next step.

**States.** Every async action shows progress (`Button loading`, `Skeleton`, `Progress`) and an outcome (`toast`, `Alert`). Every list has an empty state. Every form field has an error state.

## What to avoid

These are the marks of generated UI. Do not produce them.

- Purple-to-blue gradients, gradient text, glowing borders, and glass panels over busy backgrounds.
- Emoji in headings, buttons or navigation.
- Everything centered in a narrow column with oversized rounded cards.
- A hero section, three feature cards and a call to action when the request was an application.
- Decorative badges and pills that carry no state.
- More than one accent color, or tinting every card a different color.
- Heavy drop shadows on cards that sit in the page.
- Uppercase, letter-spaced labels everywhere.
- Inventing a new visual style per page. Reuse the same components and spacing.

## Structure by kind of app

**Application (dashboard, admin, tool).** `AppShell` with a vertical `Nav`; each page is `<main>` → `Container` → `Stack gap="lg">` → `PageHeader` then content. Summaries are `Stat`s in a `Grid`; collections are a `Table`; details open in a `Drawer`; settings are a narrow `Container` of `Card`s with forms. Add a `CommandPalette` on `mod+k` when there are more than a few destinations.

**Content site.** A horizontal `Nav` in a header, `Container size="narrow"` for reading width, `Heading`/`Text` for type. Use loaders and `staticParams()` so pages prerender.

**Single-file tool.** One page, no navigation chrome. State in `usePersistentState` (`forge:store`) or a self-saving document (`forge:file`). Build with `forge build --target single-html`.

**Game.** The canvas fills its box (`fit()` from `forge:game`). UI over it goes in `Hud` slots; menus and pause screens are `Dialog`s. Keep HUD text short and use `Bar` for gauges. Run the simulation in a fixed-step `loop()`; never tie game speed to frame rate.

## Accessibility is part of the design

- Every input has a visible `label`. Icon-only buttons use `IconButton label`.
- Meaning never depends on color alone: status has text (`Badge` with a word), errors have a message.
- Everything works with a keyboard. The library components already do; custom ones must too.
- Headings go in order. Landmarks (`Nav label`, `<main>`) are named.
- Respect `prefers-reduced-motion` (the library does) and keep motion under 200 ms.

## Performance

- Fetch data in `loader.ts`, not in effects, so the first paint has content.
- Import images from code (`import photo from "./photo.jpg"`) and render them with `Image width height`, so space is reserved and they load lazily.
- Keep client-only code (`three`, charts) in pages with `export const ssr = false`, or load it in an effect.
- Do not add a CSS framework or a component library on top of `forge:ui`; each one adds weight to every page and to single-file builds.

## Checklist before finishing

1. `forge check` passes and `npx tsc -p .` is clean.
2. The page works at 320px wide and at desktop width, in light and dark.
3. Tab through it: focus is always visible, dialogs trap and return focus, Escape closes overlays.
4. No hard-coded colors, no emoji icons, one primary action per view.
5. Loading, empty and error states exist for anything that loads or submits.
6. For offline output: `forge build --target single-html` and open the file with networking off.

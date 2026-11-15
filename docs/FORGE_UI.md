# Build an interface with Forge UI

Start with [the package README](../packages/forge-ui/README.md) and the [Relay showcase](../examples/ui-showcase). The package is `@nextrs/ui`. Its [public exports](../packages/forge-ui/src/index.ts) are the import reference for generated apps.

1. Import `@nextrs/ui/styles.css` once in `app/layout.tsx` and wrap `children` with `Theme`. Keep the initial theme the same in both environments. For system mode, the stylesheet honors the OS preference before React starts.
2. Build a real page structure with `main`, one `h1`, named navigation, and headings in order. Use `Container`, `Stack`, and `Grid` for sizing and spacing. Wrap related content in `Card` parts.
3. Use `Button` for actions and `ButtonLink` or `NavLink` for navigation. Use `href()` from `forge:navigation` for routes that must also work in a single HTML file. Normal anchors support browser navigation. Use Forge’s `Link` for client route transitions and prefetching.
4. Use natural HTML control props. Pass `label`, `name`, a value or default value, and any constraints such as `required`. Use `error` for a specific corrective message and focus the first invalid field on custom validation.
5. Keep each dialog and its trigger under the same `Theme`. Set `open` from React state and connect `onOpenChange` to it. Give icon-only buttons an accessible name. Use `Tabs` for related views of the same page and navigation for distinct routes. Keep tab content mounted light enough for immediate arrow-key activation.
6. Put informational feedback near the relevant control. Mount `NotificationRegion` before user actions so new notifications can be announced. Use stable IDs per notification and remove them on dismissal.

```tsx
import { useState } from "react";
import {
  Button, Card, CardContent, CardHeader, CardTitle, Container,
  Dialog, Select, Stack, TextInput,
} from "@nextrs/ui";

export default function Page() {
  const [open, setOpen] = useState(false);
  const [project, setProject] = useState("");
  return (
    <main>
      <Container size="narrow">
        <Stack gap="lg">
          <h1>Project settings</h1>
          <Card>
            <CardHeader><CardTitle>{project || "Your project"}</CardTitle></CardHeader>
            <CardContent>
              <Button onClick={(event) => {
                event.currentTarget.focus();
                setOpen(true);
              }}>Rename project</Button>
            </CardContent>
          </Card>
          <Dialog open={open} onOpenChange={setOpen} title="Rename project">
            <form onSubmit={(event) => {
              event.preventDefault();
              setProject(String(new FormData(event.currentTarget).get("name")));
              setOpen(false);
            }}>
              <Stack>
                <TextInput name="name" label="Project name" required />
                <Select name="visibility" label="Visibility" disabled>
                  <option>Private</option>
                </Select>
                <Button type="submit">Save name</Button>
              </Stack>
            </form>
          </Dialog>
        </Stack>
      </Container>
    </main>
  );
}
```

Prefer system fonts and local assets to keep offline output self-contained. Use classes for app styling so they work under the Forge server’s Content Security Policy.

## Verify an app

Run these from the app directory:

```sh
forge build
forge build --target static
forge build --target single-html
```

Open `dist/index.html` directly with networking disabled. Try narrow and desktop layouts in light and dark modes, keyboard focus, dialog Escape and focus return, tab arrows and wrapping, invalid form recovery, and status announcements. Check SSR markup and run your TypeScript check alongside the build.

## Design conventions

Use a restrained developer-tool layout. Give the page title and its main action a shared row, organize related metrics together, and use dividers to separate dense information. Forge UI uses a system sans stack, warm neutral surfaces, a violet action accent, and distinct semantic status colors. Compact rounding and quiet borders define surfaces. Reserve pronounced elevation for overlays. Hover, selection, and theme changes are immediate.

For Figma handoff, use the semantic names in [styles.css](../packages/forge-ui/styles.css) as variable names. Give color variables Light and Dark modes. Keep spacing, radius, and typography independent of the palette. Draw components using the variants available in [the package reference](../packages/forge-ui/README.md) so the design can map to React props.

References for these conventions include [Geist materials](https://vercel.com/geist/materials) and [button hierarchy](https://vercel.com/geist/button), [daisyUI semantic colors](https://daisyui.com/docs/colors/), and [Figma variable modes](https://help.figma.com/hc/en-us/articles/15339657135383-Guide-to-variables-in-Figma). The [WAI-ARIA dialog](https://www.w3.org/WAI/ARIA/apg/patterns/dialog-modal/) and [tabs](https://www.w3.org/WAI/ARIA/apg/patterns/tabs/) patterns describe the keyboard behaviors to verify.

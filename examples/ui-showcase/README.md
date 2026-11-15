# Relay

Relay is a sample release workspace built with [Forge UI](../../packages/forge-ui/README.md). It uses fixture data so the dashboard, environment filters, activity tabs, schedule form, notifications, and theme control work without a backend. Scheduling adds an entry to the page's queue and activity. Changes reset when you leave the overview.

From this directory, with `forge` on your PATH:

```sh
npm ci
npm run typecheck
npm run dev
```

For streaming SSR, run `npm run build` and `npm start`. To prerender for a static host, run `forge build --target static`. For an offline copy, run `npm run offline` and open `dist/index.html` in your browser. The Component guide route works in each target.

See the [agent guide](../../docs/FORGE_UI.md) for import, theming, and composition examples.

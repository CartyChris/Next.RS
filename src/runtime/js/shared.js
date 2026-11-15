// Registry shared by every forge:* module. It imports nothing, so apps that
// never render React do not bundle React. The React adapter and the client
// router fill in the late-bound slots.
export const ROOT_ID = "__forge_root";
export const DATA_ID = "__FORGE_DATA__";
export const STATE_ID = "__FORGE_STATE__";
export const APP_SCRIPT_ID = "__forge_app";

export const rt = {
  mode: "history", // "history" | "hash" (single-file HTML)
  server: typeof document === "undefined",
  state: null, // current route state on the client
  React: null,
  useRoute: null,
  Link: null,
  navigate: null,
  prefetch: null,
  href: (to) => (rt.mode === "hash" ? `#${to}` : to),
  // single-html: the document as parsed ({ head, root, state }), for forge:file saveSelf.
  selfDocument: null,
};

export function useRoute() {
  if (!rt.useRoute) throw new Error("Forge hooks need a React page: this route was not rendered by the React adapter");
  return rt.useRoute();
}

// Later modules win: layouts first, page last. `meta` may be an object or a function of the route state.
export function resolveMeta(mods, state) {
  const out = {};
  const info = { params: state.params, data: state.data, pathname: state.pathname };
  for (const mod of mods) Object.assign(out, typeof mod.meta === "function" ? mod.meta(info) : mod.meta);
  return out;
}

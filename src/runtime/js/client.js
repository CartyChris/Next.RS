// Client runtime: route matching, data fetching, history/hash navigation, and
// mounting through a renderer adapter (React, or the built-in vanilla adapter).
import { rt, resolveMeta, ROOT_ID, DATA_ID } from "./shared.js";

let routes = [];
let notFoundRoute = null;
let adapters = {};
let dataMode = "server"; // "server" | "static" | "inline"
let staticData = {};
let state = null;
let active = null; // { kind, handle }
let navToken = 0;
const listeners = new Set();
const store = {
  get: () => state,
  subscribe(fn) {
    listeners.add(fn);
    return () => listeners.delete(fn);
  },
};

const normalize = (p) => (p.length > 1 && p.endsWith("/") ? p.replace(/\/+$/, "") || "/" : p || "/");
const safeDecode = (s) => {
  try {
    return decodeURIComponent(s);
  } catch {
    return s;
  }
};

function matchSegments(segs, parts) {
  const params = {};
  for (let i = 0; i < segs.length; i++) {
    const [type, value] = segs[i];
    if (type === 0) {
      if (parts[i] !== value) return null;
    } else if (type === 1) {
      if (i >= parts.length) return null;
      params[value] = parts[i];
    } else {
      if (parts.length <= i) return null;
      params[value] = parts.slice(i).join("/");
      return params;
    }
  }
  return parts.length === segs.length ? params : null;
}

export function match(pathname) {
  const parts = pathname.split("/").filter(Boolean).map(safeDecode);
  for (const route of routes) {
    const params = matchSegments(route.segs, parts);
    if (params) return { route, params };
  }
  return null;
}

// Where are we? In hash mode the app path lives after `#`.
function here() {
  if (rt.mode !== "hash") return { pathname: normalize(location.pathname), search: location.search };
  const u = new URL(location.hash.slice(1) || "/", "http://x");
  return { pathname: normalize(u.pathname), search: u.search };
}

function parse(to) {
  if (rt.mode === "hash") {
    const u = new URL(String(to).replace(/^#/, ""), `http://x${here().pathname}`);
    return { pathname: normalize(u.pathname), search: u.search, hash: "", external: false };
  }
  const u = new URL(to, location.href);
  return { pathname: normalize(u.pathname), search: u.search, hash: u.hash, external: u.origin !== location.origin };
}

const dataCache = new Map();
function loadData(route, pathname, search) {
  if (!route.loader) return Promise.resolve({ data: null });
  if (dataMode === "inline") return Promise.resolve(pathname in staticData ? { data: staticData[pathname] } : { notFound: true });
  const key = pathname + search;
  const hit = dataCache.get(key);
  if (hit && Date.now() - hit.at < 5000) return hit.promise;
  const url =
    dataMode === "static"
      ? `/_forge/data${pathname === "/" ? "/index" : pathname}.json`
      : `/_forge/data?path=${encodeURIComponent(key)}`;
  const promise = fetch(url, { headers: { accept: "application/json" } }).then((r) => {
    if (r.status === 404) return { notFound: true };
    if (!r.ok) throw new Error(`data request failed: ${r.status}`);
    return r.json();
  });
  dataCache.set(key, { at: Date.now(), promise });
  promise.catch(() => dataCache.delete(key));
  return promise;
}

function applyMeta(s) {
  const meta = resolveMeta([...s.mod.layouts, s.mod.page], s);
  if (meta.title) document.title = meta.title;
  if (meta.description != null) {
    let tag = document.querySelector('meta[name="description"]');
    if (!tag) {
      tag = document.createElement("meta");
      tag.name = "description";
      document.head.append(tag);
    }
    tag.content = meta.description;
  }
}

// Pages without a framework: `export function mount(el, ctx)` returning an optional cleanup,
// and optionally `export function render(ctx)` returning HTML (also used for SSR).
const vanilla = {
  mount(el, _store, hydrate) {
    let cleanup;
    let run = 0;
    const show = async (isHydrating) => {
      const id = ++run;
      const s = state;
      if (typeof cleanup === "function") cleanup();
      cleanup = undefined;
      const { page } = s.mod;
      const ctx = { params: s.params, data: s.data, pathname: s.pathname, search: s.search, hydrate: isHydrating, navigate: go };
      if (!isHydrating) {
        const html = page.render ? await page.render(ctx) : "";
        if (id !== run) return;
        el.innerHTML = html ?? "";
      }
      cleanup = page.mount?.(el, ctx);
    };
    show(hydrate);
    return {
      update: () => show(false),
      unmount() {
        run++;
        if (typeof cleanup === "function") cleanup();
      },
    };
  },
};

function render(next, hydrate = false) {
  const el = document.getElementById(ROOT_ID);
  if (active && active.kind !== next.kind) {
    active.handle.unmount();
    active = null;
    el.textContent = "";
  }
  state = rt.state = next;
  if (!active) active = { kind: next.kind, handle: adapters[next.kind].mount(el, store, hydrate) };
  else active.handle.update?.();
  for (const fn of listeners) fn();
  applyMeta(next);
}

const hard = (to) => (rt.mode === "hash" ? undefined : location.assign(to));

async function go(to, { replace = false, scroll = true, history: push = true } = {}) {
  const loc = parse(to);
  if (loc.external) return void location.assign(to);
  const token = ++navToken;
  const m = match(loc.pathname);
  let route = m ? m.route : notFoundRoute;
  let params = m ? m.params : {};
  if (!route) return hard(to);
  let mod, res;
  try {
    [mod, res] = await Promise.all([route.load(), m ? loadData(route, loc.pathname, loc.search) : { data: null }]);
  } catch (e) {
    // Most often a stale chunk after a deploy: let the server answer.
    console.error("[forge] navigation failed:", e);
    return hard(to);
  }
  if (token !== navToken) return;
  if (res.redirect) return go(res.redirect, { replace: true });
  if (res.notFound) {
    if (!notFoundRoute) return hard(to);
    route = notFoundRoute;
    params = {};
    res = { data: null };
    mod = await route.load();
    if (token !== navToken) return;
  }
  if (push) {
    const url = rt.href(loc.pathname + loc.search) + loc.hash;
    try {
      history[replace ? "replaceState" : "pushState"](null, "", url);
    } catch {
      location.hash = loc.pathname + loc.search; // some file:// contexts refuse pushState
    }
  }
  render({ route: route.id, kind: route.kind, params, data: res.data ?? null, pathname: loc.pathname, search: loc.search, mod });
  if (loc.hash) document.getElementById(loc.hash.slice(1))?.scrollIntoView();
  else if (scroll) scrollTo(0, 0);
}

function prefetch(to) {
  const loc = parse(to);
  const m = !loc.external && match(loc.pathname);
  if (!m) return;
  m.route.load().catch(() => {});
  loadData(m.route, loc.pathname, loc.search).catch(() => {});
}

function onClick(e) {
  if (e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
  const a = e.target.closest?.("a[href]");
  if (!a || (a.target && a.target !== "_self") || a.hasAttribute("download") || a.dataset.reload != null) return;
  const raw = a.getAttribute("href");
  let to;
  if (rt.mode === "hash") {
    if (raw.startsWith("#/")) to = raw.slice(1);
    else if (raw.startsWith("/") && !raw.startsWith("//")) to = raw;
    else return;
  } else {
    const u = new URL(a.href, location.href);
    if (u.origin !== location.origin) return;
    if (u.pathname === location.pathname && u.search === location.search && u.hash) return; // in-page anchor
    if (!match(normalize(u.pathname))) return; // public files, API routes: let the browser load them
    to = u.pathname + u.search + u.hash;
  }
  e.preventDefault();
  go(to);
}

export async function start(config) {
  routes = config.routes;
  notFoundRoute = config.notFound ?? null;
  adapters = { vanilla, ...config.adapters };
  dataMode = config.data;
  staticData = config.staticData ?? {};
  rt.mode = config.mode;
  rt.navigate = go;
  rt.prefetch = prefetch;

  addEventListener("click", onClick);
  const onPop = () => {
    const loc = here();
    if (state && loc.pathname === state.pathname && loc.search === state.search) return;
    go(loc.pathname + loc.search, { history: false, scroll: false });
  };
  addEventListener(rt.mode === "hash" ? "hashchange" : "popstate", onPop);

  // Server-rendered pages carry their state; everything else resolves like a navigation.
  let boot = null;
  try {
    boot = JSON.parse(document.getElementById(DATA_ID)?.textContent || "null");
  } catch {}
  const route = boot?.route && (routes.find((r) => r.id === boot.route) ?? (notFoundRoute?.id === boot.route ? notFoundRoute : null));
  const loc = here();
  // No boot state, or (hash mode) the document was prerendered for another path: resolve like a navigation.
  if (!route || (boot.pathname && boot.pathname !== loc.pathname)) {
    return go(rt.mode === "hash" ? loc.pathname + loc.search : location.pathname + location.search, { history: false, scroll: false });
  }
  const mod = await route.load();
  render({ route: route.id, kind: route.kind, params: boot.params ?? {}, data: boot.data ?? null, pathname: loc.pathname, search: loc.search, mod }, boot.ssr !== false);
}

// Server runtime: bundled with the app, driven by the Rust host one request at a time.
// Protocol (per request id): head(json) → write(chunk)* | pipe(handle) → end().
import { host } from "./host-ref.js";
import { rt, resolveMeta } from "./shared.js";

rt.server = true;

const escapeHtml = (s) => String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
// JSON that is safe inside a <script> element.
const scriptJson = (value) => JSON.stringify(value).replace(/</g, "\\u003c");
const env = new Proxy({}, { get: (_, key) => (typeof key === "string" ? host.env(key) : undefined) });

export function createServer({ routes, apis, notFound, renderers, dev }) {
  const pages = new Map(routes.map((r) => [r.id, r]));
  const apiById = new Map(apis.map((a) => [a.id, a]));
  renderers = {
    vanilla: async (state) => (await state.mod.page.render?.({ params: state.params, data: state.data, pathname: state.pathname })) ?? "",
    ...renderers,
  };

  const context = (req) => {
    const url = new URL(req.url);
    return { params: req.params, url, searchParams: url.searchParams, request: new Request(req.url, { method: req.method, headers: req.headers }), env };
  };

  // → { data } | { redirect, status } | { notFound }
  async function load(route, req) {
    if (!route.loader?.loader) return { data: null };
    try {
      return { data: (await route.loader.loader(context(req))) ?? null };
    } catch (e) {
      if (e?.__forge === "redirect") return { redirect: e.location, status: e.status };
      if (e?.__forge === "not-found") return { notFound: true };
      throw e;
    }
  }

  async function page(req, out) {
    let route = pages.get(req.route);
    let status = 200;
    let params = req.params;
    let res = route ? await load(route, req) : { notFound: true };
    if (res.redirect) return out.head({ status: res.status, headers: [["location", res.redirect]] });
    if (res.notFound) {
      if (!notFound) return out.head({ status: 404, builtin: "not-found" });
      [route, status, params, res] = [notFound, 404, {}, { data: null }];
    }
    const state = { route: route.id, kind: route.kind, params, data: res.data, pathname: req.pathname, search: "", mod: route.mod };
    const boot = scriptJson({ schemaVersion: 1, route: route.id, params, data: res.data });
    const meta = resolveMeta([...route.mod.layouts, route.mod.page], state);
    // Render the shell first: if it throws we can still answer with a clean 500.
    const body = await renderers[route.kind](state, { nonce: req.nonce, stream: req.stream });
    out.head({ status, page: { meta, boot } });
    if (typeof body === "string") out.write(body);
    else for await (const chunk of body) out.write(chunk);
  }

  async function data(req, out) {
    const res = await load(pages.get(req.route), req);
    const payload = res.redirect ? { redirect: res.redirect } : res.notFound ? { notFound: true } : { data: res.data };
    out.head({ status: 200, headers: [["content-type", "application/json; charset=utf-8"]] });
    out.write(JSON.stringify(payload));
  }

  async function api(req, out, body) {
    const mod = apiById.get(req.route).mod;
    const handler = mod[req.method] ?? (req.method === "HEAD" ? mod.GET : undefined);
    if (typeof handler !== "function") {
      const allow = Object.keys(mod).filter((k) => /^[A-Z]+$/.test(k)).join(", ");
      return out.head({ status: 405, headers: [["allow", allow]] });
    }
    const ctx = context(req);
    ctx.request = new Request(req.url, { method: req.method, headers: req.headers, body: body ?? null });
    let res = await handler(ctx.request, ctx);
    if (!(res instanceof Response)) res = Response.json(res ?? null);
    out.head({ status: res.status, headers: [...res.headers] });
    const b = res._body;
    if (b == null) return;
    if (typeof b === "string" || b instanceof Uint8Array) out.write(b);
    else if (b._handle != null && !b._locked) out.pipe(b._handle); // untouched upstream body: Rust streams it directly
    else for await (const chunk of b) out.write(chunk);
  }

  // Prerender support: the concrete params a dynamic route should be built for.
  async function paths(req, out) {
    const list = (await pages.get(req.route)?.loader?.staticParams?.()) ?? [];
    out.head({ status: 200, headers: [["content-type", "application/json"]] });
    out.write(JSON.stringify(list));
  }

  const kinds = { page, data, api, paths };

  globalThis.__forge_dispatch = (id, json, body) => {
    let sent = false;
    const out = {
      head: (o) => ((sent = true), host.head(id, JSON.stringify(o))),
      write: (chunk) => host.write(id, chunk),
      pipe: (handle) => host.pipe(id, handle),
    };
    (async () => {
      try {
        const req = JSON.parse(json);
        await kinds[req.kind](req, out, body);
      } catch (e) {
        console.error(e);
        if (!sent) {
          const detail = dev ? `${e?.name ?? "Error"}: ${e?.message ?? e}\n${e?.stack ?? ""}` : "Internal Server Error";
          out.head({ status: 500, headers: [["content-type", "text/html; charset=utf-8"]] });
          out.write(`<!doctype html><title>Server error</title><pre>${escapeHtml(detail)}</pre>`);
        }
      } finally {
        host.end(id);
      }
    })();
  };
}

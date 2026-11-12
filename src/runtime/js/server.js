// Server runtime: bundled with the app, driven by the Rust host.
//
// Requests (per id): head(json) → write(chunk)* | pipe(handle) → end().
// Aborts: Rust calls __forge_abort(id) when the client left or the request timed out;
//         the request's AbortSignal (ctx.signal, request.signal, React's render) fires.
// Sockets: Rust calls __forge_socket("open" | "message" | "close", sid, a, b).
//          Every socket for one URL path lives on the same worker, so a route's
//          module-level state is a consistent room.
import { host } from "./host-ref.js";
import { rt, resolveMeta } from "./shared.js";

rt.server = true;

const escapeHtml = (s) => String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
// JSON that is safe inside a <script> element.
const scriptJson = (value) => JSON.stringify(value).replace(/</g, "\\u003c");
const env = new Proxy({}, { get: (_, key) => (typeof key === "string" ? host.env(key) : undefined) });
const closed = () => Object.assign(new Error("client disconnected"), { __forge: "closed" });
const encode = (m) => (typeof m === "string" || m instanceof Uint8Array ? m : JSON.stringify(m));

export function createServer({ routes, apis, notFound, renderers, dev, links = "history" }) {
  rt.mode = links;
  const pages = new Map(routes.map((r) => [r.id, r]));
  if (notFound) pages.set(notFound.id, notFound);
  const apiById = new Map(apis.map((a) => [a.id, a]));
  renderers = {
    vanilla: async (state) => (await state.mod.page.render?.({ params: state.params, data: state.data, pathname: state.pathname })) ?? "",
    ...renderers,
  };

  const context = (req, signal, body) => {
    const url = new URL(req.url);
    const request = new Request(req.url, { method: req.method, headers: req.headers, body: body ?? null, signal });
    return { params: req.params, url, searchParams: url.searchParams, request, signal, env };
  };

  // → { data } | { redirect, status } | { notFound }
  async function load(route, req, signal) {
    if (!route.loader?.loader) return { data: null };
    try {
      return { data: (await route.loader.loader(context(req, signal))) ?? null };
    } catch (e) {
      if (e?.__forge === "redirect") return { redirect: e.location, status: e.status };
      if (e?.__forge === "not-found") return { notFound: true };
      throw e;
    }
  }

  async function page(req, out, signal) {
    let route = pages.get(req.route);
    let status = 200;
    let params = req.params;
    let res = route ? await load(route, req, signal) : { notFound: true };
    if (res.redirect) return out.head({ status: res.status, headers: [["location", res.redirect]] });
    if (res.notFound) {
      if (!notFound) return out.head({ status: 404, builtin: "not-found" });
      [route, status, params, res] = [notFound, 404, {}, { data: null }];
    }
    const ssr = !!route.mod;
    const boot = scriptJson({ schemaVersion: 1, route: route.id, params, data: res.data, pathname: req.pathname, ssr });
    if (!ssr) return out.head({ status, page: { meta: {}, boot, route: route.id, ssr } });
    const state = { route: route.id, kind: route.kind, params, data: res.data, pathname: req.pathname, search: "", mod: route.mod };
    const meta = resolveMeta([...route.mod.layouts, route.mod.page], state);
    // Render the shell first: if it throws we can still answer with a clean 500.
    const body = await renderers[route.kind](state, { nonce: req.nonce, stream: req.stream, signal });
    out.head({ status, page: { meta, boot, route: route.id, ssr } });
    if (typeof body === "string") await out.write(body);
    else for await (const chunk of body) await out.write(chunk);
  }

  async function data(req, out, signal) {
    const res = await load(pages.get(req.route), req, signal);
    const payload = res.redirect ? { redirect: res.redirect } : res.notFound ? { notFound: true } : { data: res.data };
    out.head({ status: 200, headers: [["content-type", "application/json; charset=utf-8"]] });
    await out.write(JSON.stringify(payload));
  }

  async function api(req, out, signal, body) {
    const mod = apiById.get(req.route).mod;
    const handler = mod[req.method] ?? (req.method === "HEAD" ? mod.GET : undefined);
    if (typeof handler !== "function") {
      const allow = Object.keys(mod).filter((k) => /^[A-Z]+$/.test(k)).join(", ");
      return out.head({ status: 405, headers: [["allow", allow]] });
    }
    const ctx = context(req, signal, body);
    let res = await handler(ctx.request, ctx);
    if (!(res instanceof Response)) res = Response.json(res ?? null);
    out.head({ status: res.status, headers: [...res.headers] });
    const b = res._body;
    if (b == null) return;
    if (typeof b === "string" || b instanceof Uint8Array) await out.write(b);
    else if (b._handle != null && !b._locked) out.pipe(b._handle); // untouched upstream body: Rust streams it directly
    else for await (const chunk of b) await out.write(chunk);
  }

  // Prerender support: the concrete params a dynamic route should be built for.
  async function paths(req, out) {
    const list = (await pages.get(req.route)?.loader?.staticParams?.()) ?? [];
    out.head({ status: 200, headers: [["content-type", "application/json"]] });
    await out.write(JSON.stringify(list));
  }

  const kinds = { page, data, api, paths };
  const controllers = new Map();

  globalThis.__forge_abort = (id) => {
    const c = controllers.get(id);
    controllers.delete(id);
    c?.abort(Object.assign(new Error("request aborted: client disconnected or timed out"), { name: "AbortError" }));
  };

  globalThis.__forge_dispatch = (id, json, body) => {
    let sent = false;
    const controller = new AbortController();
    controllers.set(id, controller);
    const out = {
      head: (o) => ((sent = true), host.head(id, JSON.stringify(o))),
      // Resolves when the client can take more; throws if it went away (stops rendering).
      async write(chunk) {
        const r = host.write(id, chunk);
        if (r === 2) throw closed();
        if (r === 1) await host.op((o) => host.drain(o, id));
      },
      pipe: (handle) => host.pipe(id, handle),
    };
    (async () => {
      try {
        const req = JSON.parse(json);
        await kinds[req.kind](req, out, controller.signal, body);
      } catch (e) {
        if (e?.__forge === "closed" || controller.signal.aborted) return;
        console.error(e);
        if (!sent) {
          const detail = dev ? `${e?.name ?? "Error"}: ${e?.message ?? e}\n${e?.stack ?? ""}` : "Internal Server Error";
          out.head({ status: 500, headers: [["content-type", "text/html; charset=utf-8"]] });
          await out.write(`<!doctype html><title>Server error</title><pre>${escapeHtml(detail)}</pre>`).catch(() => {});
        }
      } finally {
        controllers.delete(id);
        host.end(id);
      }
    })();
  };

  // ---- WebSockets ----
  const sockets = new Map();
  const decode = (data, binary) => {
    if (binary) return data;
    try {
      return JSON.parse(data);
    } catch {
      return data;
    }
  };
  const guard = (ws, fn) => {
    try {
      const r = fn();
      if (r && typeof r.then === "function") r.catch((e) => fail(ws, e));
    } catch (e) {
      fail(ws, e);
    }
  };
  const fail = (ws, e) => {
    console.error(e);
    host.wsClose(ws.id, 1011, dev ? String(e?.message ?? e).slice(0, 120) : "server error");
  };

  globalThis.__forge_socket = (kind, sid, a, b) => {
    if (kind === "open") {
      const req = JSON.parse(a);
      const handlers = apiById.get(req.route)?.mod.socket;
      if (!handlers) return host.wsClose(sid, 1011, "route has no socket handler");
      const ctx = context(req);
      const ws = {
        id: sid,
        params: req.params,
        url: ctx.url,
        /** Per-connection state for your handlers. */
        data: {},
        send: (msg) => host.wsSend(sid, encode(msg)),
        close: (code = 1000, reason = "") => host.wsClose(sid, code, reason),
        subscribe: (topic) => host.subscribe(sid, topic),
        unsubscribe: (topic) => host.unsubscribe(sid, topic),
        /** Send to every subscriber of `topic` except this socket. */
        publish: (topic, msg) => host.publish(topic, encode(msg), sid),
      };
      sockets.set(sid, { ws, handlers, ctx });
      guard(ws, () => handlers.open?.(ws, ctx));
    } else if (kind === "message") {
      const s = sockets.get(sid);
      if (s) guard(s.ws, () => s.handlers.message?.(s.ws, decode(a, b), s.ctx));
    } else {
      const s = sockets.get(sid);
      sockets.delete(sid);
      if (s) guard(s.ws, () => s.handlers.close?.(s.ws, a, b));
    }
  };
}

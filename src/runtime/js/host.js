// Host prelude for the embedded engine. Runs once per worker, before the server bundle.
// It turns the raw `__host` ops into the web-standard globals server code expects:
// timers, console, TextEncoder/Decoder, URL, Headers/Request/Response, fetch, streams.
(() => {
  "use strict";
  const g = globalThis;
  const host = g.__host;
  delete g.__host;
  // The server runtime claims the raw ops exactly once; app code never sees them.
  g.__forge_take_host = () => {
    delete g.__forge_take_host;
    host.op = op;
    return host;
  };

  // ---- async ops: Rust resolves them by id --------------------------------
  const pending = new Map();
  let opSeq = 0;
  const op = (start) =>
    new Promise((resolve, reject) => {
      const id = ++opSeq;
      pending.set(id, [resolve, reject]);
      start(id);
    });
  // Give up on an in-flight op: its promise rejects now and Rust drops the late result.
  const abortOp = (id, reason) => {
    const p = pending.get(id);
    if (!p) return;
    pending.delete(id);
    host.cancel(id);
    p[1](reason);
  };
  g.__forge_resolve = (id, ok, value) => {
    const p = pending.get(id);
    if (!p) return;
    pending.delete(id);
    if (ok) p[0](value);
    else p[1](new Error(value));
  };

  // ---- macrotasks + timers -------------------------------------------------
  let tasks = [];
  g.__forge_tick = () => {
    if (!tasks.length) return false;
    const run = tasks;
    tasks = [];
    for (const task of run) {
      try {
        task();
      } catch (e) {
        g.console.error(e);
      }
    }
    return true;
  };
  const timers = new Set();
  let timerSeq = 0;
  g.setTimeout = (fn, ms = 0, ...args) => {
    const id = ++timerSeq;
    timers.add(id);
    const fire = () => timers.delete(id) && fn(...args);
    if (ms > 0) op((o) => host.timer(o, ms)).then(fire);
    else tasks.push(fire);
    return id;
  };
  g.setInterval = (fn, ms = 0, ...args) => {
    const id = ++timerSeq;
    timers.add(id);
    const tick = () => {
      if (!timers.has(id)) return;
      fn(...args);
      op((o) => host.timer(o, Math.max(1, ms))).then(tick);
    };
    op((o) => host.timer(o, Math.max(1, ms))).then(tick);
    return id;
  };
  g.clearTimeout = g.clearInterval = (id) => void timers.delete(id);
  g.queueMicrotask ??= (fn) => void Promise.resolve().then(fn);
  g.MessageChannel = class MessageChannel {
    constructor() {
      const port = (other) => ({ onmessage: null, postMessage: (data) => tasks.push(() => other().onmessage?.({ data })) });
      this.port1 = port(() => this.port2);
      this.port2 = port(() => this.port1);
    }
  };

  // ---- console / performance / crypto -------------------------------------
  const show = (v) => {
    if (typeof v === "string") return v;
    if (v instanceof Error) return `${v.name}: ${v.message}\n${v.stack ?? ""}`.trimEnd();
    try {
      return JSON.stringify(v) ?? String(v);
    } catch {
      return String(v);
    }
  };
  const log = (level) => (...args) => host.log(level, args.map(show).join(" "));
  g.console = { log: log(0), info: log(0), debug: log(0), warn: log(1), error: log(2), trace: log(0) };
  g.performance = { now: () => host.now(), timeOrigin: Date.now() };
  g.crypto = {
    getRandomValues(a) {
      new Uint8Array(a.buffer, a.byteOffset, a.byteLength).set(host.random(a.byteLength));
      return a;
    },
    randomUUID() {
      const b = host.random(16);
      b[6] = (b[6] & 15) | 64;
      b[8] = (b[8] & 63) | 128;
      const h = Array.from(b, (x) => x.toString(16).padStart(2, "0")).join("");
      return `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20)}`;
    },
  };

  // ---- text encoding --------------------------------------------------------
  const view = (v) =>
    v instanceof Uint8Array ? v : ArrayBuffer.isView(v) ? new Uint8Array(v.buffer, v.byteOffset, v.byteLength) : new Uint8Array(v);
  // Bytes at the end of `b` that start a UTF-8 sequence which is not complete yet.
  const incompleteTail = (b) => {
    for (let i = 1; i <= Math.min(3, b.length); i++) {
      const c = b[b.length - i];
      if ((c & 0xc0) === 0x80) continue;
      const need = c >= 0xf0 ? 4 : c >= 0xe0 ? 3 : c >= 0xc0 ? 2 : 1;
      return i < need ? i : 0;
    }
    return 0;
  };
  g.TextEncoder = class TextEncoder {
    get encoding() {
      return "utf-8";
    }
    encode(s = "") {
      return host.encode(String(s));
    }
    // Whole characters only; `read` counts UTF-16 code units, lone surrogates become U+FFFD.
    encodeInto(s, dest) {
      s = String(s);
      let read = 0;
      let written = 0;
      for (const ch of s) {
        let c = ch.codePointAt(0);
        if (c >= 0xd800 && c <= 0xdfff) c = 0xfffd;
        const n = c < 0x80 ? 1 : c < 0x800 ? 2 : c < 0x10000 ? 3 : 4;
        if (written + n > dest.length) break;
        if (n === 1) dest[written] = c;
        else if (n === 2) (dest[written] = 0xc0 | (c >> 6)), (dest[written + 1] = 0x80 | (c & 63));
        else if (n === 3) (dest[written] = 0xe0 | (c >> 12)), (dest[written + 1] = 0x80 | ((c >> 6) & 63)), (dest[written + 2] = 0x80 | (c & 63));
        else (dest[written] = 0xf0 | (c >> 18)), (dest[written + 1] = 0x80 | ((c >> 12) & 63)), (dest[written + 2] = 0x80 | ((c >> 6) & 63)), (dest[written + 3] = 0x80 | (c & 63));
        written += n;
        read += ch.length;
      }
      return { read, written };
    }
  };
  g.TextDecoder = class TextDecoder {
    #rest = null;
    get encoding() {
      return "utf-8";
    }
    decode(input, opts) {
      let b = input == null ? new Uint8Array(0) : view(input);
      if (this.#rest) {
        const joined = new Uint8Array(this.#rest.length + b.length);
        joined.set(this.#rest);
        joined.set(b, this.#rest.length);
        b = joined;
        this.#rest = null;
      }
      if (opts?.stream) {
        const cut = incompleteTail(b);
        if (cut) {
          this.#rest = b.slice(b.length - cut);
          b = b.subarray(0, b.length - cut);
        }
      }
      return host.decode(b);
    }
  };

  const B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  g.btoa = (s) => {
    s = String(s);
    let out = "";
    for (let i = 0; i < s.length; i += 3) {
      const a = s.charCodeAt(i), b = s.charCodeAt(i + 1), c = s.charCodeAt(i + 2);
      if (a > 255 || b > 255 || c > 255) throw new Error("btoa: character out of Latin1 range");
      const n = (a << 16) | ((b || 0) << 8) | (c || 0);
      out += B64[n >> 18] + B64[(n >> 12) & 63] + (i + 1 < s.length ? B64[(n >> 6) & 63] : "=") + (i + 2 < s.length ? B64[n & 63] : "=");
    }
    return out;
  };
  g.atob = (s) => {
    s = String(s).replace(/[\s=]/g, "");
    let out = "";
    for (let i = 0; i < s.length; i += 4) {
      const n = (B64.indexOf(s[i]) << 18) | (B64.indexOf(s[i + 1]) << 12) | ((B64.indexOf(s[i + 2]) & 63) << 6) | (B64.indexOf(s[i + 3]) & 63);
      out += String.fromCharCode(n >> 16);
      if (i + 2 < s.length) out += String.fromCharCode((n >> 8) & 255);
      if (i + 3 < s.length) out += String.fromCharCode(n & 255);
    }
    return out;
  };

  // ---- URL ------------------------------------------------------------------
  const dec = (s) => {
    try {
      return decodeURIComponent(s.replace(/\+/g, " "));
    } catch {
      return s;
    }
  };
  const enc = (s) => encodeURIComponent(s).replace(/%20/g, "+");
  class URLSearchParams {
    #l = [];
    constructor(init = "") {
      if (typeof init === "string") {
        for (const part of init.replace(/^\?/, "").split("&")) {
          if (!part) continue;
          const i = part.indexOf("=");
          this.#l.push(i < 0 ? [dec(part), ""] : [dec(part.slice(0, i)), dec(part.slice(i + 1))]);
        }
      } else if (init) {
        for (const [k, v] of Symbol.iterator in init ? init : Object.entries(init)) this.#l.push([String(k), String(v)]);
      }
    }
    get size() {
      return this.#l.length;
    }
    append(k, v) {
      this.#l.push([String(k), String(v)]);
    }
    delete(k) {
      this.#l = this.#l.filter((e) => e[0] !== k);
    }
    get(k) {
      return this.#l.find((e) => e[0] === k)?.[1] ?? null;
    }
    getAll(k) {
      return this.#l.filter((e) => e[0] === k).map((e) => e[1]);
    }
    has(k) {
      return this.#l.some((e) => e[0] === k);
    }
    set(k, v) {
      const i = this.#l.findIndex((e) => e[0] === k);
      if (i < 0) return this.append(k, v);
      this.#l = this.#l.filter((e, j) => j <= i || e[0] !== k);
      this.#l[i][1] = String(v);
    }
    sort() {
      this.#l.sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0));
    }
    forEach(fn, self) {
      for (const [k, v] of this.#l) fn.call(self, v, k, this);
    }
    *entries() {
      yield* this.#l.map((e) => [...e]);
    }
    *keys() {
      for (const e of this.#l) yield e[0];
    }
    *values() {
      for (const e of this.#l) yield e[1];
    }
    [Symbol.iterator]() {
      return this.entries();
    }
    toString() {
      return this.#l.map(([k, v]) => `${enc(k)}=${enc(v)}`).join("&");
    }
  }
  class URL {
    #p;
    #ser; // serialization of the untouched query: the raw query is kept until searchParams changes
    constructor(input, base) {
      const p = host.url(String(input), base == null ? undefined : String(base));
      if (!p) throw new TypeError(`Invalid URL: ${input}`);
      this.#p = p;
      this.searchParams = new URLSearchParams(p[8]);
      this.#ser = this.searchParams.toString();
    }
    static canParse(input, base) {
      return !!host.url(String(input), base == null ? undefined : String(base));
    }
    get protocol() {
      return this.#p[1];
    }
    get username() {
      return this.#p[2];
    }
    get password() {
      return this.#p[3];
    }
    get host() {
      return this.#p[4];
    }
    get hostname() {
      return this.#p[5];
    }
    get port() {
      return this.#p[6];
    }
    get pathname() {
      return this.#p[7];
    }
    set pathname(v) {
      this.#p[7] = String(v).startsWith("/") ? String(v) : `/${v}`;
    }
    get search() {
      const s = this.searchParams.toString();
      if (s === this.#ser) return this.#p[8] === "?" ? "" : this.#p[8];
      return s ? `?${s}` : "";
    }
    set search(v) {
      v = String(v);
      this.#p[8] = v && !v.startsWith("?") ? `?${v}` : v;
      this.searchParams = new URLSearchParams(v);
      this.#ser = this.searchParams.toString();
    }
    get hash() {
      return this.#p[9];
    }
    set hash(v) {
      this.#p[9] = v && !String(v).startsWith("#") ? `#${v}` : String(v);
    }
    get origin() {
      return this.#p[10];
    }
    get href() {
      const p = this.#p;
      const auth = p[2] ? `${p[2]}${p[3] ? `:${p[3]}` : ""}@` : "";
      return p[4] ? `${p[1]}//${auth}${p[4]}${p[7]}${this.search}${p[9]}` : `${p[1]}${p[7]}${this.search}${p[9]}`;
    }
    toString() {
      return this.href;
    }
    toJSON() {
      return this.href;
    }
  }
  g.URL = URL;
  g.URLSearchParams = URLSearchParams;

  // ---- abort ----------------------------------------------------------------
  class AbortSignal {
    aborted = false;
    reason = undefined;
    onabort = null;
    #l = new Set();
    addEventListener(type, fn) {
      if (type === "abort") this.#l.add(fn);
    }
    removeEventListener(type, fn) {
      this.#l.delete(fn);
    }
    throwIfAborted() {
      if (this.aborted) throw this.reason;
    }
    _fire(reason) {
      if (this.aborted) return;
      this.aborted = true;
      this.reason = reason;
      for (const fn of [this.onabort, ...this.#l]) fn?.call(this, { type: "abort", target: this });
    }
    static abort(reason) {
      const s = new AbortSignal();
      s._fire(reason ?? new Error("AbortError"));
      return s;
    }
    static timeout(ms) {
      const s = new AbortSignal();
      g.setTimeout(() => s._fire(new Error("TimeoutError")), ms);
      return s;
    }
  }
  g.AbortSignal = AbortSignal;
  g.AbortController = class AbortController {
    signal = new AbortSignal();
    abort(reason) {
      this.signal._fire(reason ?? new Error("AbortError"));
    }
  };

  // ---- streams (the subset React, fetch and SSE code use) -------------------
  class ReadableStream {
    #src;
    #q = [];
    #waiters = [];
    #state = 0; // 0 readable, 1 closed, 2 errored
    #err;
    #hwm;
    #started = false;
    #pulling = false;
    #again = false;
    #ctrl;
    _locked = false;
    _handle = null; // native upstream body, if this stream came from fetch()

    constructor(src = {}, strategy = {}) {
      this.#src = src;
      this.#hwm = strategy.highWaterMark ?? 1;
      const self = this;
      this.#ctrl = {
        enqueue: (chunk) => {
          if (self.#state) return;
          const w = self.#waiters.shift();
          if (w) w.resolve({ done: false, value: chunk });
          else self.#q.push(chunk);
          self.#pull();
        },
        close: () => {
          if (self.#state) return;
          self.#state = 1;
          if (!self.#q.length) for (const w of self.#waiters.splice(0)) w.resolve({ done: true, value: undefined });
        },
        error: (e) => {
          if (self.#state) return;
          self.#state = 2;
          self.#err = e;
          self.#q = [];
          for (const w of self.#waiters.splice(0)) w.reject(e);
        },
        get desiredSize() {
          return self.#hwm - self.#q.length;
        },
      };
      Promise.resolve()
        .then(() => src.start?.(this.#ctrl))
        .then(() => {
          this.#started = true;
          this.#pull();
        }, this.#ctrl.error);
    }

    #pull() {
      if (this.#state || !this.#started || !this.#src.pull) return;
      if (!(this.#waiters.length || this.#hwm - this.#q.length > 0)) return;
      if (this.#pulling) {
        this.#again = true;
        return;
      }
      this.#pulling = true;
      Promise.resolve()
        .then(() => this.#src.pull(this.#ctrl))
        .then(() => {
          this.#pulling = false;
          if (this.#again) {
            this.#again = false;
            this.#pull();
          }
        }, this.#ctrl.error);
    }

    #read() {
      if (this.#q.length) {
        const value = this.#q.shift();
        if (this.#state === 1 && !this.#q.length) for (const w of this.#waiters.splice(0)) w.resolve({ done: true, value: undefined });
        this.#pull();
        return Promise.resolve({ done: false, value });
      }
      if (this.#state === 1) return Promise.resolve({ done: true, value: undefined });
      if (this.#state === 2) return Promise.reject(this.#err);
      return new Promise((resolve, reject) => {
        this.#waiters.push({ resolve, reject });
        this.#pull();
      });
    }

    get locked() {
      return this._locked;
    }
    cancel(reason) {
      if (this.#state) return Promise.resolve();
      this.#state = 1;
      this.#q = [];
      for (const w of this.#waiters.splice(0)) w.resolve({ done: true, value: undefined });
      return Promise.resolve(this.#src.cancel?.(reason));
    }
    getReader() {
      if (this._locked) throw new TypeError("ReadableStream is locked");
      this._locked = true;
      return {
        read: () => this.#read(),
        cancel: (r) => this.cancel(r),
        releaseLock: () => {
          this._locked = false;
        },
      };
    }
    pipeThrough(transform) {
      (async () => {
        try {
          for await (const chunk of this) await transform.writable._write(chunk);
          await transform.writable._close();
        } catch (e) {
          transform.writable._abort(e);
        }
      })();
      return transform.readable;
    }
    async pipeTo(dest) {
      for await (const chunk of this) await dest._write(chunk);
      await dest._close();
    }
    [Symbol.asyncIterator]() {
      const reader = this.getReader();
      return {
        next: () => reader.read(),
        return: async (value) => {
          await reader.cancel();
          return { done: true, value };
        },
        [Symbol.asyncIterator]() {
          return this;
        },
      };
    }
    static from(iterable) {
      const it = iterable[Symbol.asyncIterator]?.() ?? iterable[Symbol.iterator]();
      return new ReadableStream({
        async pull(c) {
          const { done, value } = await it.next();
          if (done) c.close();
          else c.enqueue(value);
        },
        cancel: (r) => it.return?.(r),
      });
    }
  }
  class TransformStream {
    constructor(t = {}) {
      let out;
      this.readable = new ReadableStream({ start: (c) => void (out = c) });
      const ctl = { enqueue: (v) => out.enqueue(v), error: (e) => out.error(e), terminate: () => out.close() };
      const w = {
        _write: async (chunk) => (t.transform ? t.transform(chunk, ctl) : ctl.enqueue(chunk)),
        _close: async () => {
          await t.flush?.(ctl);
          out.close();
        },
        _abort: (e) => out.error(e),
        getWriter: () => ({ write: w._write, close: w._close, abort: w._abort, releaseLock() {} }),
      };
      this.writable = w;
    }
  }
  g.ReadableStream = ReadableStream;
  g.TransformStream = TransformStream;
  g.TextDecoderStream = class TextDecoderStream extends TransformStream {
    constructor() {
      const d = new g.TextDecoder();
      const push = (c, s) => s && c.enqueue(s);
      super({ transform: (chunk, c) => push(c, d.decode(chunk, { stream: true })), flush: (c) => push(c, d.decode()) });
    }
  };
  g.TextEncoderStream = class TextEncoderStream extends TransformStream {
    constructor() {
      super({ transform: (chunk, c) => c.enqueue(host.encode(String(chunk))) });
    }
  };

  // ---- fetch API ------------------------------------------------------------
  class Headers {
    #l = [];
    constructor(init) {
      if (!init) return;
      for (const [k, v] of Symbol.iterator in init ? init : Object.entries(init)) this.append(k, v);
    }
    append(k, v) {
      this.#l.push([String(k).toLowerCase(), String(v)]);
    }
    delete(k) {
      k = String(k).toLowerCase();
      this.#l = this.#l.filter((e) => e[0] !== k);
    }
    set(k, v) {
      this.delete(k);
      this.append(k, v);
    }
    get(k) {
      k = String(k).toLowerCase();
      const v = this.#l.filter((e) => e[0] === k).map((e) => e[1]);
      return v.length ? v.join(", ") : null;
    }
    has(k) {
      return this.get(k) !== null;
    }
    getSetCookie() {
      return this.#l.filter((e) => e[0] === "set-cookie").map((e) => e[1]);
    }
    forEach(fn, self) {
      for (const [k, v] of this.#l) fn.call(self, v, k, this);
    }
    *entries() {
      yield* this.#l.map((e) => [...e]);
    }
    *keys() {
      for (const e of this.#l) yield e[0];
    }
    *values() {
      for (const e of this.#l) yield e[1];
    }
    [Symbol.iterator]() {
      return this.entries();
    }
  }

  // Internal body forms: null | string | Uint8Array | ReadableStream.
  const toBody = (body, headers) => {
    const type = (t) => headers.has("content-type") || headers.set("content-type", t);
    if (body == null) return null;
    if (typeof body === "string") return type("text/plain;charset=UTF-8"), body;
    if (body instanceof URLSearchParams) return type("application/x-www-form-urlencoded;charset=UTF-8"), body.toString();
    if (body instanceof ReadableStream) return body;
    if (body instanceof ArrayBuffer || ArrayBuffer.isView(body)) return view(body);
    if (body[Symbol.asyncIterator] || body[Symbol.iterator]) return ReadableStream.from(body);
    throw new TypeError("unsupported body type");
  };
  const collect = async (body) => {
    if (body == null) return new Uint8Array(0);
    if (typeof body === "string") return host.encode(body);
    if (body instanceof Uint8Array) return body;
    const parts = [];
    let size = 0;
    for await (const chunk of body) {
      const b = typeof chunk === "string" ? host.encode(chunk) : view(chunk);
      parts.push(b);
      size += b.length;
    }
    const out = new Uint8Array(size);
    let at = 0;
    for (const p of parts) out.set(p, at), (at += p.length);
    return out;
  };
  class Body {
    _body = null;
    get body() {
      const b = this._body;
      if (b == null || b instanceof ReadableStream) return b;
      return (this._body = ReadableStream.from([typeof b === "string" ? host.encode(b) : b]));
    }
    get bodyUsed() {
      return this._body instanceof ReadableStream && this._body.locked;
    }
    async bytes() {
      return collect(this._body);
    }
    async arrayBuffer() {
      const b = await collect(this._body);
      return b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength);
    }
    async text() {
      return typeof this._body === "string" ? this._body : host.decode(await collect(this._body));
    }
    async json() {
      return JSON.parse(await this.text());
    }
  }
  class Response extends Body {
    constructor(body = null, init = {}) {
      super();
      this.status = init.status ?? 200;
      this.statusText = init.statusText ?? "";
      this.headers = new Headers(init.headers);
      this.url = "";
      this._body = toBody(body, this.headers);
    }
    get ok() {
      return this.status >= 200 && this.status < 300;
    }
    clone() {
      if (this._body instanceof ReadableStream) throw new TypeError("cannot clone a streaming Response");
      return new Response(this._body, this);
    }
    static json(data, init = {}) {
      const headers = new Headers(init.headers);
      if (!headers.has("content-type")) headers.set("content-type", "application/json; charset=utf-8");
      return new Response(JSON.stringify(data), { ...init, headers });
    }
    static redirect(url, status = 302) {
      return new Response(null, { status, headers: { location: String(url) } });
    }
  }
  class Request extends Body {
    constructor(input, init = {}) {
      super();
      const from = input instanceof Request ? input : null;
      this.url = from ? from.url : String(input);
      this.method = String(init.method ?? from?.method ?? "GET").toUpperCase();
      this.headers = new Headers(init.headers ?? from?.headers);
      this.signal = init.signal ?? from?.signal ?? null;
      this._body = toBody(init.body ?? from?._body ?? null, this.headers);
    }
  }
  g.Headers = Headers;
  g.Request = Request;
  g.Response = Response;

  const dropper = g.FinalizationRegistry ? new g.FinalizationRegistry((handle) => host.drop(handle)) : null;
  g.fetch = async (input, init = {}) => {
    const req = new Request(input, init);
    const signal = req.signal;
    signal?.throwIfAborted();
    const body = req._body == null ? null : await collect(req._body);
    const meta = JSON.stringify({ url: new URL(req.url).href, method: req.method, headers: [...req.headers] });
    let opId;
    const pendingResponse = op((o) => {
      opId = o;
      host.fetch(o, meta, body);
    });
    const onAbort = () => abortOp(opId, signal.reason);
    signal?.addEventListener("abort", onAbort);
    let parts;
    try {
      parts = JSON.parse(await pendingResponse);
    } finally {
      signal?.removeEventListener("abort", onAbort);
    }
    const [status, statusText, headers, handle, url] = parts;
    const stream = new ReadableStream({
      async pull(c) {
        const chunk = await op((o) => host.read(o, handle));
        if (chunk == null) c.close();
        else c.enqueue(chunk);
      },
      cancel: () => host.drop(handle),
    });
    stream._handle = handle;
    dropper?.register(stream, handle);
    signal?.addEventListener("abort", () => stream.cancel(signal.reason));
    const res = new Response(stream, { status, statusText, headers });
    res.url = url;
    return res;
  };
})();

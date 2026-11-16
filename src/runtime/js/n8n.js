// forge:n8n: connect a Next.RS app to n8n (or any webhook-driven automation tool).
//
//   trigger(url, data)        call a workflow's Webhook node and get its response
//   webhook(handler, options) an API route handler that n8n's HTTP Request node can call
//
// Keep webhook URLs and secrets on the server: read them from `ctx.env` (listed in
// `server.env`) and allow the n8n host in `server.allowFetch`.

/** A failed call to a workflow: `status` and the parsed `body` say why. */
export class WorkflowError extends Error {
  constructor(message, status, body) {
    super(message);
    this.name = "WorkflowError";
    this.status = status;
    this.body = body;
  }
}

async function readBody(source) {
  const type = (source.headers.get("content-type") ?? "").toLowerCase();
  const text = await source.text();
  if (!text) return null;
  if (type.includes("json")) {
    try {
      return JSON.parse(text);
    } catch {
      throw new WorkflowError("Body is not valid JSON", 400, text);
    }
  }
  if (type.includes("application/x-www-form-urlencoded")) return Object.fromEntries(new URLSearchParams(text));
  // n8n sometimes sends JSON without the header.
  try {
    return JSON.parse(text);
  } catch {
    return text;
  }
}

/**
 * Call a workflow through its Webhook node (use the Production URL once the workflow is
 * active). Resolves with what the workflow responds: parsed JSON, or text.
 *
 * `secret` is sent as the `secretHeader` header (default `x-n8n-secret`), matching the node's
 * "Header Auth" credential; `basicAuth: { user, password }` matches "Basic Auth".
 */
export async function trigger(url, data, options = {}) {
  const { method = "POST", headers = {}, secret, secretHeader = "x-n8n-secret", basicAuth, query, signal, timeoutMs = 30_000 } = options;
  const target = new URL(url);
  for (const [key, value] of Object.entries(query ?? {})) target.searchParams.set(key, String(value));
  const send = { ...headers };
  if (secret) send[secretHeader] = secret;
  if (basicAuth) send.authorization = `Basic ${btoa(`${basicAuth.user}:${basicAuth.password}`)}`;
  const hasBody = data !== undefined && method !== "GET" && method !== "HEAD";
  if (hasBody) send["content-type"] ??= "application/json";
  if (!hasBody && data && typeof data === "object") {
    for (const [key, value] of Object.entries(data)) target.searchParams.set(key, String(value));
  }
  const timeout = new AbortController();
  const timer = setTimeout(() => timeout.abort(new WorkflowError(`No response from the workflow within ${timeoutMs} ms`, 504)), timeoutMs);
  signal?.addEventListener("abort", () => timeout.abort(signal.reason), { once: true });
  let response;
  try {
    response = await fetch(target, {
      method,
      headers: send,
      body: hasBody ? (typeof data === "string" ? data : JSON.stringify(data)) : undefined,
      signal: timeout.signal,
    });
  } catch (cause) {
    if (cause instanceof WorkflowError) throw cause;
    throw new WorkflowError(`Could not reach the workflow: ${cause?.message ?? cause}`, 502);
  } finally {
    clearTimeout(timer);
  }
  const body = await readBody(response).catch(() => null);
  if (!response.ok) {
    const hint = response.status === 404 ? " (is the workflow active, and is this the Production URL?)" : response.status === 401 || response.status === 403 ? " (check the webhook's authentication)" : "";
    throw new WorkflowError(`The workflow answered ${response.status}${hint}`, response.status, body);
  }
  return body;
}

// Compare without leaking how many leading characters match.
function sameSecret(a, b) {
  if (typeof a !== "string" || typeof b !== "string" || a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return diff === 0;
}

/**
 * An API route handler for calls from n8n (HTTP Request node) or any webhook sender:
 *
 *   export const POST = webhook(async ({ body }) => ({ received: body.id }), { secret: (ctx) => ctx.env.N8N_SECRET });
 *
 * It checks the shared secret (the `secretHeader` header or `Authorization: Bearer …`),
 * parses a JSON or form body, and answers with JSON: what the handler returns, or
 * `{ ok: true }`. A thrown `WorkflowError` sets the status; other errors answer 500.
 */
export function webhook(handler, options = {}) {
  const { secret, secretHeader = "x-n8n-secret" } = options;
  return async (request, ctx) => {
    const expected = typeof secret === "function" ? secret(ctx) : secret;
    if (secret !== undefined) {
      if (!expected) return Response.json({ ok: false, error: "This webhook has no secret configured on the server" }, { status: 500 });
      const bearer = request.headers.get("authorization")?.replace(/^Bearer\s+/i, "");
      if (!sameSecret(request.headers.get(secretHeader), expected) && !sameSecret(bearer, expected)) {
        return Response.json({ ok: false, error: `Missing or wrong secret: send it in the ${secretHeader} header` }, { status: 401 });
      }
    }
    try {
      const body = request.method === "GET" || request.method === "HEAD" ? null : await readBody(request);
      const query = Object.fromEntries(new URL(request.url).searchParams);
      const result = await handler({ body, query, headers: request.headers, request, ctx });
      if (result instanceof Response) return result;
      return Response.json(result ?? { ok: true });
    } catch (error) {
      if (error instanceof WorkflowError) return Response.json({ ok: false, error: error.message }, { status: error.status ?? 400 });
      console.error(error);
      return Response.json({ ok: false, error: "The handler failed" }, { status: 500 });
    }
  };
}

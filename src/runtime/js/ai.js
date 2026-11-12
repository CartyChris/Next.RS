// forge:ai: streaming helpers for LLM apps. Works in the browser (single-file,
// bring-your-own-key) and inside server API routes (keys stay server-side).

import { rt } from "./shared.js";

// Body chunks via a reader: async iteration of ReadableStream is not universal in browsers.
async function* chunks(body) {
  const reader = body.getReader();
  let done = false;
  try {
    for (;;) {
      const r = await reader.read();
      if ((done = r.done)) return;
      yield r.value;
    }
  } finally {
    if (!done) await reader.cancel().catch(() => {});
  }
}

/** Parse a `text/event-stream` body into `{ event, data, id }` records. */
export async function* sse(response) {
  if (!response.ok) throw new Error(`HTTP ${response.status}: ${(await response.text()).slice(0, 500)}`);
  const decoder = new TextDecoder();
  let buffer = "";
  const parse = (block) => {
    const ev = { event: "message", data: "", id: undefined };
    const data = [];
    for (const line of block.split("\n")) {
      if (!line || line[0] === ":") continue;
      const i = line.indexOf(":");
      const field = i < 0 ? line : line.slice(0, i);
      const value = i < 0 ? "" : line.slice(i + 1).replace(/^ /, "");
      if (field === "data") data.push(value);
      else if (field === "event") ev.event = value;
      else if (field === "id") ev.id = value;
    }
    ev.data = data.join("\n");
    return data.length ? ev : null;
  };
  for await (const chunk of chunks(response.body)) {
    buffer += decoder.decode(chunk, { stream: true }).replace(/\r\n?/g, "\n");
    let end;
    while ((end = buffer.indexOf("\n\n")) >= 0) {
      const ev = parse(buffer.slice(0, end));
      buffer = buffer.slice(end + 2);
      if (ev) yield ev;
    }
  }
  const last = parse(buffer + decoder.decode());
  if (last) yield last;
}

// Text delta from the common provider stream shapes.
const delta = (j) =>
  j?.choices?.[0]?.delta?.content ?? // OpenAI-compatible chat completions
  (j?.type === "content_block_delta" ? j.delta?.text : undefined) ?? // Anthropic Messages
  (j?.type === "response.output_text.delta" ? j.delta : undefined) ?? // OpenAI Responses
  j?.candidates?.[0]?.content?.parts?.[0]?.text ?? // Gemini
  j?.message?.content ?? // Ollama chat
  j?.response ?? // Ollama generate
  (typeof j?.text === "string" ? j.text : undefined);

/**
 * Stream text from an endpoint: `for await (const piece of streamText(url, init))`.
 * Understands SSE and NDJSON bodies in the provider formats above, and plain text streams.
 */
export async function* streamText(url, init) {
  const response = await fetch(url, init);
  const type = response.headers.get("content-type") ?? "";
  if (type.includes("text/event-stream")) {
    for await (const ev of sse(response)) {
      if (ev.data === "[DONE]") return;
      let json;
      try {
        json = JSON.parse(ev.data);
      } catch {
        yield ev.data;
        continue;
      }
      if (json?.type === "error" || json?.error) throw new Error(json.error?.message ?? JSON.stringify(json.error ?? json));
      const text = delta(json);
      if (text) yield text;
    }
    return;
  }
  if (!response.ok) throw new Error(`HTTP ${response.status}: ${(await response.text()).slice(0, 500)}`);
  const decoder = new TextDecoder();
  const ndjson = type.includes("ndjson") || type.includes("jsonl");
  let buffer = "";
  for await (const chunk of chunks(response.body)) {
    const text = decoder.decode(chunk, { stream: true });
    if (!ndjson) {
      if (text) yield text;
      continue;
    }
    buffer += text;
    const lines = buffer.split("\n");
    buffer = lines.pop();
    for (const line of lines) {
      const piece = line.trim() && delta(JSON.parse(line));
      if (piece) yield piece;
    }
  }
}

/** Server helper: turn an async iterable of strings into an SSE `Response`. */
export function sseResponse(iterable, init = {}) {
  const encoder = new TextEncoder();
  const body = (async function* () {
    try {
      for await (const piece of iterable) yield encoder.encode(`data: ${JSON.stringify({ text: piece })}\n\n`);
      yield encoder.encode("data: [DONE]\n\n");
    } catch (e) {
      yield encoder.encode(`data: ${JSON.stringify({ error: { message: String(e?.message ?? e) } })}\n\n`);
    }
  })();
  const stream = new ReadableStream({
    async pull(c) {
      const { done, value } = await body.next();
      if (done) c.close();
      else c.enqueue(value);
    },
  });
  return new Response(stream, {
    ...init,
    headers: { "content-type": "text/event-stream", "cache-control": "no-cache", ...init.headers },
  });
}

/**
 * React hook for rendering a text stream:
 * `const { text, streaming, error, start, stop } = useStream(); start(streamText(url, init))`.
 */
export function useStream() {
  const { useState, useRef, useCallback } = rt.React;
  const [text, setText] = useState("");
  const [streaming, setStreaming] = useState(false);
  const [error, setError] = useState(null);
  const run = useRef(0);
  const start = useCallback(async (source) => {
    const id = ++run.current;
    let acc = "";
    setText("");
    setError(null);
    setStreaming(true);
    try {
      for await (const piece of source) {
        if (run.current !== id) break;
        setText((acc += piece));
      }
    } catch (e) {
      if (run.current === id) setError(e);
    } finally {
      if (run.current === id) setStreaming(false);
    }
    return acc;
  }, []);
  const stop = useCallback(() => {
    run.current++;
    setStreaming(false);
  }, []);
  return { text, streaming, error, start, stop };
}

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

// ---------------------------------------------------------------------------
// chat(): one message format, four wire protocols, normalized streaming events.
//
// Messages: { role: "system" | "user" | "assistant" | "tool", content: string,
//             toolCalls?: [{ id, name, arguments }], toolCallId?: string, name?: string }
// Tools:    [{ name, description, parameters: <JSON Schema> }]
// Events:   { type: "text", text } | { type: "tool_call", id, name, arguments }
//           | { type: "usage", input, output } | { type: "done", stop }
// ---------------------------------------------------------------------------

const DEFAULT_BASE = {
  openai: "https://api.openai.com/v1",
  anthropic: "https://api.anthropic.com/v1",
  gemini: "https://generativelanguage.googleapis.com/v1beta",
  ollama: "http://localhost:11434",
};

const parseArgs = (raw) => {
  if (raw == null || raw === "") return {};
  if (typeof raw === "object") return raw;
  try {
    return JSON.parse(raw);
  } catch {
    return { _raw: raw };
  }
};

async function* lines(response) {
  if (!response.ok) throw new Error(`HTTP ${response.status}: ${(await response.text()).slice(0, 500)}`);
  const decoder = new TextDecoder();
  let buffer = "";
  for await (const chunk of chunks(response.body)) {
    buffer += decoder.decode(chunk, { stream: true });
    const parts = buffer.split("\n");
    buffer = parts.pop();
    for (const line of parts) if (line.trim()) yield line;
  }
  if (buffer.trim()) yield buffer;
}

const providers = {
  openai: {
    request(o) {
      const messages = [];
      if (o.system) messages.push({ role: "system", content: o.system });
      for (const m of o.messages) {
        if (m.role === "tool") messages.push({ role: "tool", tool_call_id: m.toolCallId, content: m.content });
        else if (m.toolCalls?.length)
          messages.push({
            role: "assistant",
            content: m.content || null,
            tool_calls: m.toolCalls.map((c) => ({ id: c.id, type: "function", function: { name: c.name, arguments: JSON.stringify(c.arguments ?? {}) } })),
          });
        else messages.push({ role: m.role, content: m.content });
      }
      const body = { model: o.model, messages, stream: true, stream_options: { include_usage: true } };
      if (o.tools?.length) body.tools = o.tools.map((t) => ({ type: "function", function: { name: t.name, description: t.description, parameters: t.parameters ?? { type: "object", properties: {} } } }));
      if (o.temperature != null) body.temperature = o.temperature;
      if (o.maxTokens != null) body.max_tokens = o.maxTokens;
      const headers = { "content-type": "application/json" };
      if (o.apiKey) headers.authorization = `Bearer ${o.apiKey}`;
      return [`${o.baseUrl}/chat/completions`, headers, body];
    },
    async *parse(response) {
      const calls = new Map();
      let stop = "stop";
      for await (const ev of sse(response)) {
        if (ev.data === "[DONE]") break;
        const j = JSON.parse(ev.data);
        if (j.error) throw new Error(j.error.message ?? JSON.stringify(j.error));
        if (j.usage) yield { type: "usage", input: j.usage.prompt_tokens ?? 0, output: j.usage.completion_tokens ?? 0 };
        const choice = j.choices?.[0];
        if (!choice) continue;
        if (choice.delta?.content) yield { type: "text", text: choice.delta.content };
        for (const tc of choice.delta?.tool_calls ?? []) {
          const c = calls.get(tc.index ?? 0) ?? { id: "", name: "", args: "" };
          if (tc.id) c.id = tc.id;
          if (tc.function?.name) c.name += tc.function.name;
          if (tc.function?.arguments) c.args += tc.function.arguments;
          calls.set(tc.index ?? 0, c);
        }
        if (choice.finish_reason) stop = choice.finish_reason === "tool_calls" ? "tool_use" : choice.finish_reason;
      }
      for (const c of calls.values()) yield { type: "tool_call", id: c.id, name: c.name, arguments: parseArgs(c.args) };
      yield { type: "done", stop };
    },
  },

  anthropic: {
    request(o) {
      const messages = [];
      for (const m of o.messages) {
        if (m.role === "system") continue;
        if (m.role === "tool") {
          const block = { type: "tool_result", tool_use_id: m.toolCallId, content: m.content };
          const last = messages.at(-1);
          if (last?.role === "user" && Array.isArray(last.content) && last.content[0]?.type === "tool_result") last.content.push(block);
          else messages.push({ role: "user", content: [block] });
        } else if (m.toolCalls?.length) {
          const content = m.content ? [{ type: "text", text: m.content }] : [];
          for (const c of m.toolCalls) content.push({ type: "tool_use", id: c.id, name: c.name, input: c.arguments ?? {} });
          messages.push({ role: "assistant", content });
        } else messages.push({ role: m.role, content: m.content });
      }
      const system = [o.system, ...o.messages.filter((m) => m.role === "system").map((m) => m.content)].filter(Boolean).join("\n\n");
      const body = { model: o.model, messages, max_tokens: o.maxTokens ?? 1024, stream: true };
      if (system) body.system = system;
      if (o.tools?.length) body.tools = o.tools.map((t) => ({ name: t.name, description: t.description, input_schema: t.parameters ?? { type: "object", properties: {} } }));
      if (o.temperature != null) body.temperature = o.temperature;
      const headers = { "content-type": "application/json", "anthropic-version": "2023-06-01" };
      if (o.apiKey) headers["x-api-key"] = o.apiKey;
      if (typeof document !== "undefined") headers["anthropic-dangerous-direct-browser-access"] = "true";
      return [`${o.baseUrl}/messages`, headers, body];
    },
    async *parse(response) {
      const blocks = new Map();
      let stop = "stop";
      let input = 0;
      for await (const ev of sse(response)) {
        const j = JSON.parse(ev.data);
        if (j.type === "error") throw new Error(j.error?.message ?? "anthropic stream error");
        if (j.type === "message_start") input = j.message?.usage?.input_tokens ?? 0;
        else if (j.type === "content_block_start" && j.content_block?.type === "tool_use") blocks.set(j.index, { id: j.content_block.id, name: j.content_block.name, args: "" });
        else if (j.type === "content_block_delta") {
          if (j.delta?.type === "text_delta") yield { type: "text", text: j.delta.text };
          else if (j.delta?.type === "input_json_delta") blocks.get(j.index).args += j.delta.partial_json;
        } else if (j.type === "content_block_stop" && blocks.has(j.index)) {
          const b = blocks.get(j.index);
          yield { type: "tool_call", id: b.id, name: b.name, arguments: parseArgs(b.args) };
        } else if (j.type === "message_delta") {
          if (j.delta?.stop_reason) stop = j.delta.stop_reason === "end_turn" ? "stop" : j.delta.stop_reason;
          if (j.usage) yield { type: "usage", input, output: j.usage.output_tokens ?? 0 };
        }
      }
      yield { type: "done", stop };
    },
  },

  gemini: {
    request(o) {
      const contents = [];
      for (const m of o.messages) {
        if (m.role === "system") continue;
        if (m.role === "tool") contents.push({ role: "user", parts: [{ functionResponse: { name: m.name, response: { content: parseArgs(m.content) } } }] });
        else if (m.toolCalls?.length) {
          const parts = m.content ? [{ text: m.content }] : [];
          for (const c of m.toolCalls) parts.push({ functionCall: { name: c.name, args: c.arguments ?? {} } });
          contents.push({ role: "model", parts });
        } else contents.push({ role: m.role === "assistant" ? "model" : "user", parts: [{ text: m.content }] });
      }
      const body = { contents };
      const system = [o.system, ...o.messages.filter((m) => m.role === "system").map((m) => m.content)].filter(Boolean).join("\n\n");
      if (system) body.systemInstruction = { parts: [{ text: system }] };
      if (o.tools?.length) body.tools = [{ functionDeclarations: o.tools.map((t) => ({ name: t.name, description: t.description, parameters: t.parameters })) }];
      const config = {};
      if (o.temperature != null) config.temperature = o.temperature;
      if (o.maxTokens != null) config.maxOutputTokens = o.maxTokens;
      if (Object.keys(config).length) body.generationConfig = config;
      const headers = { "content-type": "application/json" };
      if (o.apiKey) headers["x-goog-api-key"] = o.apiKey;
      return [`${o.baseUrl}/models/${o.model}:streamGenerateContent?alt=sse`, headers, body];
    },
    async *parse(response) {
      let stop = "stop";
      let n = 0;
      for await (const ev of sse(response)) {
        const j = JSON.parse(ev.data);
        if (j.error) throw new Error(j.error.message ?? "gemini error");
        const cand = j.candidates?.[0];
        for (const part of cand?.content?.parts ?? []) {
          if (part.text) yield { type: "text", text: part.text };
          if (part.functionCall) yield { type: "tool_call", id: `call_${n++}`, name: part.functionCall.name, arguments: part.functionCall.args ?? {} };
        }
        if (cand?.finishReason) stop = cand.finishReason === "STOP" ? (n ? "tool_use" : "stop") : cand.finishReason.toLowerCase();
        if (j.usageMetadata?.candidatesTokenCount != null) yield { type: "usage", input: j.usageMetadata.promptTokenCount ?? 0, output: j.usageMetadata.candidatesTokenCount };
      }
      yield { type: "done", stop };
    },
  },

  ollama: {
    request(o) {
      const messages = [];
      if (o.system) messages.push({ role: "system", content: o.system });
      for (const m of o.messages) {
        if (m.role === "tool") messages.push({ role: "tool", content: m.content, tool_name: m.name });
        else if (m.toolCalls?.length) messages.push({ role: "assistant", content: m.content ?? "", tool_calls: m.toolCalls.map((c) => ({ function: { name: c.name, arguments: c.arguments ?? {} } })) });
        else messages.push({ role: m.role, content: m.content });
      }
      const body = { model: o.model, messages, stream: true };
      if (o.tools?.length) body.tools = o.tools.map((t) => ({ type: "function", function: { name: t.name, description: t.description, parameters: t.parameters } }));
      const options = {};
      if (o.temperature != null) options.temperature = o.temperature;
      if (o.maxTokens != null) options.num_predict = o.maxTokens;
      if (Object.keys(options).length) body.options = options;
      return [`${o.baseUrl}/api/chat`, { "content-type": "application/json" }, body];
    },
    async *parse(response) {
      let n = 0;
      let stop = "stop";
      for await (const line of lines(response)) {
        const j = JSON.parse(line);
        if (j.error) throw new Error(j.error);
        if (j.message?.content) yield { type: "text", text: j.message.content };
        for (const c of j.message?.tool_calls ?? []) yield { type: "tool_call", id: `call_${n++}`, name: c.function.name, arguments: parseArgs(c.function.arguments) };
        if (j.done) {
          if (j.eval_count != null) yield { type: "usage", input: j.prompt_eval_count ?? 0, output: j.eval_count };
          stop = n ? "tool_use" : j.done_reason ?? "stop";
        }
      }
      yield { type: "done", stop };
    },
  },
};

// Offline stand-in: echoes, and calls a tool when the last user message names one.
async function* mockModel(o) {
  const last = [...o.messages].reverse().find((m) => m.role === "user" || m.role === "tool");
  const wanted = last?.role === "user" ? o.tools?.find((t) => last.content.toLowerCase().includes(t.name.toLowerCase())) : null;
  if (wanted) {
    yield { type: "tool_call", id: "call_mock_0", name: wanted.name, arguments: {} };
    yield { type: "done", stop: "tool_use" };
    return;
  }
  const reply = last?.role === "tool" ? `The ${last.name ?? "tool"} returned: ${last.content}` : `(mock model) You said: ${last?.content ?? ""}`;
  for (const word of reply.split(/(?<= )/)) {
    if (o.delay) await new Promise((r) => setTimeout(r, o.delay));
    o.signal?.throwIfAborted();
    yield { type: "text", text: word };
  }
  yield { type: "usage", input: o.messages.reduce((n, m) => n + String(m.content ?? "").length, 0), output: reply.length };
  yield { type: "done", stop: "stop" };
}

/**
 * Stream a chat completion from any supported provider.
 * `chat({ provider, model, messages, tools?, system?, apiKey?, baseUrl?, temperature?, maxTokens?, signal?, headers? })`
 * Returns an async iterable of events with `.text()` and `.result()` helpers.
 */
export function chat(options) {
  const o = { provider: "openai", messages: [], ...options };
  async function* run() {
    if (o.provider === "mock") return yield* mockModel(o);
    const provider = providers[o.provider];
    if (!provider) throw new Error(`forge:ai: unknown provider "${o.provider}" (openai, anthropic, gemini, ollama, mock)`);
    o.baseUrl = (o.baseUrl ?? DEFAULT_BASE[o.provider]).replace(/\/+$/, "");
    const [url, headers, body] = provider.request(o);
    const response = await fetch(url, { method: "POST", headers: { ...headers, ...o.headers }, body: JSON.stringify(body), signal: o.signal });
    yield* provider.parse(response);
  }
  const iterable = run();
  let cached;
  iterable.result = () =>
    (cached ??= (async () => {
      const out = { text: "", toolCalls: [], usage: null, stop: null };
      for await (const ev of iterable) {
        if (ev.type === "text") out.text += ev.text;
        else if (ev.type === "tool_call") out.toolCalls.push(ev);
        else if (ev.type === "usage") out.usage = { input: ev.input, output: ev.output };
        else if (ev.type === "done") out.stop = ev.stop;
      }
      return out;
    })());
  iterable.text = async () => (await iterable.result()).text;
  return iterable;
}

/**
 * Tool-calling agent loop. `tools` maps names to `{ description, parameters, run(args, ctx) }`.
 * Yields every chat event plus `{ type: "tool_result", id, name, result }` and a final
 * `{ type: "final", text, messages, steps }`.
 */
export async function* runAgent({ tools = {}, maxSteps = 8, messages = [], ...options }) {
  const history = [...messages];
  const specs = Object.entries(tools).map(([name, t]) => ({ name, description: t.description ?? "", parameters: t.parameters ?? { type: "object", properties: {} } }));
  for (let step = 1; step <= maxSteps; step++) {
    let text = "";
    const calls = [];
    for await (const ev of chat({ ...options, messages: history, tools: specs })) {
      if (ev.type === "text") text += ev.text;
      if (ev.type === "tool_call") calls.push(ev);
      yield ev;
    }
    history.push({ role: "assistant", content: text, toolCalls: calls.map(({ id, name, arguments: a }) => ({ id, name, arguments: a })) });
    if (!calls.length) {
      yield { type: "final", text, messages: history, steps: step };
      return;
    }
    for (const call of calls) {
      let result;
      try {
        const tool = tools[call.name];
        if (!tool) throw new Error(`unknown tool ${call.name}`);
        result = await tool.run(call.arguments, { signal: options.signal });
      } catch (e) {
        result = { error: String(e?.message ?? e) };
      }
      yield { type: "tool_result", id: call.id, name: call.name, result };
      history.push({ role: "tool", toolCallId: call.id, name: call.name, content: typeof result === "string" ? result : JSON.stringify(result) });
    }
  }
  yield { type: "final", text: "", messages: history, steps: maxSteps, stop: "max_steps" };
}

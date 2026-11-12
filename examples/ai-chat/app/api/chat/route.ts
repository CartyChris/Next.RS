import { sseResponse } from "forge:ai";

type Message = { role: "system" | "user" | "assistant"; content: string };

// POST /api/chat → text/event-stream.
// With OPENAI_API_KEY and/or OPENAI_BASE_URL set (in the environment or .env), proxies any
// OpenAI-compatible chat API: OpenAI, OpenRouter, Groq, Ollama (http://localhost:11434/v1),
// LM Studio, vLLM, llama.cpp server. Without them, streams a local echo so the app runs offline.
export async function POST(request: Request, { env }: ForgeContext) {
  const { messages } = (await request.json()) as { messages: Message[] };
  const key = env.OPENAI_API_KEY;
  const base = env.OPENAI_BASE_URL;
  if (!key && !base) return sseResponse(echo(messages.at(-1)?.content ?? ""));

  const upstream = await fetch(`${base ?? "https://api.openai.com/v1"}/chat/completions`, {
    method: "POST",
    headers: { "content-type": "application/json", ...(key ? { authorization: `Bearer ${key}` } : {}) },
    body: JSON.stringify({ model: env.OPENAI_MODEL ?? "gpt-4o-mini", messages, stream: true }),
  });
  // Returning the upstream body untouched lets Forge pipe it to the browser in Rust,
  // chunk by chunk, without the bytes passing through JavaScript.
  return new Response(upstream.body, {
    status: upstream.status,
    headers: { "content-type": upstream.headers.get("content-type") ?? "text/event-stream" },
  });
}

async function* echo(prompt: string) {
  const reply = `(local echo: set OPENAI_API_KEY or OPENAI_BASE_URL to use a model) You said: ${prompt}`;
  for (const word of reply.split(/(?<= )/)) {
    await new Promise((r) => setTimeout(r, 20));
    yield word;
  }
}

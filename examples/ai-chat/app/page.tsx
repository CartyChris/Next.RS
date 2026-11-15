import { useState, type FormEvent } from "react";
import { streamText, useStream } from "forge:ai";
import "./chat.css";

type Message = { role: "user" | "assistant"; content: string };

export const meta = { title: "Forge Chat" };

export default function Chat() {
  const [messages, setMessages] = useState<Message[]>([]);
  const [input, setInput] = useState("");
  const { text, streaming, error, start, stop } = useStream();

  async function send(e: FormEvent) {
    e.preventDefault();
    if (!input.trim() || streaming) return;
    const next: Message[] = [...messages, { role: "user", content: input }];
    setMessages(next);
    setInput("");
    const reply = await start(
      streamText("/api/chat", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ messages: next }),
      }),
    );
    if (reply) setMessages((m) => [...m, { role: "assistant", content: reply }]);
  }

  return (
    <main>
      <h1>Forge Chat</h1>
      <div className="log">
        {messages.map((m, i) => (
          <div key={i} className={`msg ${m.role}`}>
            {m.content}
          </div>
        ))}
        {streaming && <div className="msg assistant" id="streaming">{text || "…"}</div>}
        {error != null && <p className="error">{String((error as Error).message ?? error)}</p>}
      </div>
      <form onSubmit={send}>
        <input value={input} onChange={(e) => setInput(e.target.value)} placeholder="Ask anything" autoFocus />
        {streaming ? <button type="button" onClick={stop}>Stop</button> : <button>Send</button>}
      </form>
    </main>
  );
}

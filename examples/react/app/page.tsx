import { useState } from "react";

export const meta = { title: "Forge React", description: "Rust-served React with streaming SSR" };

export default function Home() {
  const [count, setCount] = useState(0);
  return (
    <main>
      <h1>Hello Forge</h1>
      <p>Server-rendered by QuickJS inside a Rust binary, hydrated by React.</p>
      <button id="inc" onClick={() => setCount((c) => c + 1)}>
        Clicked {count} times
      </button>
    </main>
  );
}

import { Suspense, use } from "react";

export const meta = { title: "Streaming · Forge" };

const wait = (ms: number, value: string) => new Promise<string>((r) => setTimeout(() => r(value), ms));
let cached: Promise<string> | null = null;

function Slow() {
  // On the server this suspends; React streams the fallback first, then the content.
  const text = use((cached ??= wait(300, "Loaded after 300 ms, streamed into the same response.")));
  return <p id="slow">{text}</p>;
}

export default function Page() {
  return (
    <main>
      <h1>Streaming</h1>
      <Suspense fallback={<p id="fallback">Loading…</p>}>
        <Slow />
      </Suspense>
    </main>
  );
}

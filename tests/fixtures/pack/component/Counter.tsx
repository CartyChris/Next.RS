// An artifact-style component: React, Tailwind classes, an icon library. No package.json,
// no node_modules: `forge pack Counter.tsx` installs what it imports and mounts it.
import { useState } from "react";
import { Minus, Plus } from "lucide-react";

export default function Counter() {
  const [count, setCount] = useState(0);
  return (
    <main className="min-h-screen flex items-center justify-center bg-slate-950 text-slate-100">
      <div className="flex flex-col items-center gap-4 p-8 rounded-2xl bg-slate-900 shadow-xl">
        <h1 className="text-2xl font-bold tracking-tight">Counter</h1>
        <p id="count" className="text-6xl font-mono tabular-nums">
          {count}
        </p>
        <div className="flex gap-2">
          <button id="dec" onClick={() => setCount((c) => c - 1)} className="p-2 rounded-lg bg-slate-800 hover:bg-slate-700" aria-label="decrement">
            <Minus size={20} />
          </button>
          <button id="inc" onClick={() => setCount((c) => c + 1)} className="p-2 rounded-lg bg-sky-600 hover:bg-sky-500" aria-label="increment">
            <Plus size={20} />
          </button>
        </div>
      </div>
    </main>
  );
}

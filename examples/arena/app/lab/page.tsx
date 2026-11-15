import { useEffect, useRef, useState } from "react";
import { Link } from "forge:navigation";
import { usePersistentState } from "forge:store";
import { SEED } from "../_game/brain";
import { startPreview } from "../_game/preview";

export const meta = { title: "Neuroevolution Lab · Forge Arena", description: "Train arena bots with a genetic algorithm in a Web Worker." };

type Point = { generation: number; best: number; avg: number };
type Saved = { weights: number[]; generation: number; fitness: number } | null;

function drawChart(canvas: HTMLCanvasElement, history: Point[]) {
  const g = canvas.getContext("2d")!;
  const { width: w, height: h } = canvas;
  g.clearRect(0, 0, w, h);
  if (history.length < 2) return;
  const values = history.flatMap((p) => [p.best, p.avg]);
  const lo = Math.min(...values, 0);
  const hi = Math.max(...values, 1);
  const x = (i: number) => (i / (history.length - 1)) * (w - 20) + 10;
  const y = (v: number) => h - 10 - ((v - lo) / (hi - lo || 1)) * (h - 20);
  for (const [key, color] of [
    ["avg", "#4dabf7"],
    ["best", "#ffd43b"],
  ] as const) {
    g.strokeStyle = color;
    g.lineWidth = 2;
    g.shadowColor = color;
    g.shadowBlur = 8;
    g.beginPath();
    history.forEach((p, i) => (i ? g.lineTo(x(i), y(p[key])) : g.moveTo(x(i), y(p[key]))));
    g.stroke();
  }
}

export default function Lab() {
  const [saved, setSaved] = usePersistentState<Saved>("arena:lab-best", null);
  const [running, setRunning] = useState(false);
  const [history, setHistory] = useState<Point[]>([]);
  const [last, setLast] = useState<{ generation: number; best: number; avg: number; ms: number } | null>(null);
  const [population, setPopulation] = useState(40);
  const [mutation, setMutation] = useState(0.12);
  const [deploy, setDeploy] = useState("");
  const worker = useRef<Worker | null>(null);
  const champion = useRef<number[]>(saved?.weights ?? SEED);
  const chart = useRef<HTMLCanvasElement>(null);
  const preview = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const w = new Worker(new URL("../_game/evolve.worker.ts", import.meta.url), { type: "module" });
    w.onmessage = (e) => {
      const m = e.data;
      if (m.t !== "gen") return;
      champion.current = m.champion;
      setLast({ generation: m.generation, best: m.best, avg: m.avg, ms: m.ms });
      setHistory((h) => [...h.slice(-199), { generation: m.generation, best: m.best, avg: m.avg }]);
      // Each generation plays different random matches, so scores are not comparable across
      // generations: the latest champion (selected under the same conditions as its rivals) is kept.
      setSaved({ weights: m.champion, generation: m.generation, fitness: m.best });
    };
    if (saved) w.postMessage({ t: "reset", from: saved.weights });
    worker.current = w;
    const stop = startPreview(preview.current!, () => champion.current);
    return () => {
      stop();
      w.terminate();
    };
  }, []);

  useEffect(() => {
    if (chart.current) drawChart(chart.current, history);
  }, [history]);

  const toggle = () => {
    worker.current?.postMessage({ t: running ? "pause" : "start" });
    setRunning(!running);
  };
  const reset = () => {
    worker.current?.postMessage({ t: "reset" });
    setHistory([]);
    setSaved(null);
    champion.current = SEED;
  };
  const configure = (next: { population?: number; mutation?: number }) => {
    worker.current?.postMessage({ t: "config", ...next });
    if (next.population) setPopulation(next.population);
    if (next.mutation) setMutation(next.mutation);
  };
  const offline = typeof location !== "undefined" && location.protocol === "file:";
  const publish = async () => {
    if (!saved) return;
    setDeploy("deploying…");
    try {
      const res = await fetch("/api/champion", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ ...saved, by: "lab" }),
      });
      const body = await res.json();
      setDeploy(res.ok ? `Deployed generation ${body.generation}. Rooms switch within 2 seconds.` : body.error);
    } catch (e) {
      setDeploy(`Could not reach the server: ${(e as Error).message}`);
    }
  };

  return (
    <main className="lab">
      <section className="hero small">
        <h1>Neuroevolution Lab</h1>
        <p className="lede">
          A genetic algorithm evolves the bots' 10 → 12 → 3 neural network in a Web Worker, scoring each brain by playing headless
          matches on the exact physics the server runs. Nothing leaves your browser until you deploy.
        </p>
      </section>
      <section className="grid lab-grid">
        <div className="card">
          <h2>Training</h2>
          <div className="row">
            <button className="primary" id="train" onClick={toggle}>
              {running ? "Pause" : "Train"}
            </button>
            <button onClick={reset}>Reset</button>
          </div>
          <dl className="numbers">
            <dt>Generation</dt>
            <dd id="generation">{last?.generation ?? 0}</dd>
            <dt>Best fitness</dt>
            <dd id="best">{last ? last.best.toFixed(2) : "–"}</dd>
            <dt>Average</dt>
            <dd>{last ? last.avg.toFixed(2) : "–"}</dd>
            <dt>Time / generation</dt>
            <dd>{last ? `${last.ms.toFixed(0)} ms` : "–"}</dd>
            <dt>Saved champion</dt>
            <dd>{saved ? `gen ${saved.generation}, ${saved.fitness.toFixed(2)}` : "none"}</dd>
          </dl>
          <label>
            Population {population}
            <input type="range" min={10} max={120} step={2} value={population} onChange={(e) => configure({ population: +e.target.value })} />
          </label>
          <label>
            Mutation rate {mutation.toFixed(2)}
            <input type="range" min={0.02} max={0.4} step={0.01} value={mutation} onChange={(e) => configure({ mutation: +e.target.value })} />
          </label>
          <canvas ref={chart} width={420} height={140} className="chart" aria-label="Fitness over generations" />
          <small className="muted">
            <span style={{ color: "#ffd43b" }}>best</span> · <span style={{ color: "#4dabf7" }}>average</span> fitness (orbs vs two seed bots)
          </small>
        </div>
        <div className="card">
          <h2>Champion vs seed bots</h2>
          <canvas ref={preview} width={420} height={420} className="preview" aria-label="Champion preview" />
          <div className="row">
            <button className="primary" disabled={!saved || offline} onClick={publish}>
              Deploy to every room
            </button>
            <Link to="/play/neon">Play against it →</Link>
          </div>
          <p className="muted">{offline ? "Deploying needs the server; this is the offline single-file build." : deploy}</p>
        </div>
      </section>
    </main>
  );
}

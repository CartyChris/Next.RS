// Neuroevolution in a Web Worker: a genetic algorithm over bot brains, scored by playing
// headless matches with the same physics the server runs. Nothing here touches the network.

import { SEED, crossover, mutate, randomBrain, think } from "./brain";
import { TICK, addPlayer, createWorld, rng, step } from "./sim";

type Command =
  | { t: "start" }
  | { t: "pause" }
  | { t: "reset"; from?: number[] }
  | { t: "config"; population?: number; mutation?: number };

const EPISODE_STEPS = 400; // 20 simulated seconds
const ELITE = 4;

let size = 40;
let mutation = 0.12;
let population: number[][] = [];
let generation = 0;
let running = false;
const rand = rng(Date.now() & 0x7fffffff);

function reset(from: number[] = SEED) {
  population = Array.from({ length: size }, (_, i) => (i === 0 ? from.slice() : i < size / 2 ? mutate(from, rand, 0.3, 0.6) : randomBrain(rand, 0.9)));
  generation = 0;
}

/** One match against two seed bots; fitness is our orbs minus half the opponents' average. */
function episode(brain: number[], seed: number): number {
  const w = createWorld(seed);
  const me = addPlayer(w, 1, "candidate", "", true);
  addPlayer(w, 2, "a", "", true);
  addPlayer(w, 3, "b", "", true);
  for (let i = 0; i < EPISODE_STEPS; i++) {
    for (const p of w.players.values()) p.input = think(p.id === 1 ? brain : SEED, w, p);
    step(w, TICK);
  }
  const others = (w.players.get(2)!.score + w.players.get(3)!.score) / 2;
  return me.score - others * 0.5;
}

function fitness(brain: number[]): number {
  const base = generation * 7919;
  return (episode(brain, base + 1) + episode(brain, base + 2) + episode(brain, base + 3)) / 3;
}

function evolve() {
  const started = performance.now();
  const scored = population.map((g) => ({ g, f: fitness(g) })).sort((a, b) => b.f - a.f);
  const avg = scored.reduce((n, s) => n + s.f, 0) / scored.length;
  self.postMessage({ t: "gen", generation, best: scored[0].f, avg, champion: scored[0].g, ms: performance.now() - started });
  const tournament = () => {
    let best = scored[Math.floor(rand() * scored.length)];
    for (let k = 0; k < 2; k++) {
      const c = scored[Math.floor(rand() * scored.length)];
      if (c.f > best.f) best = c;
    }
    return best.g;
  };
  const next = scored.slice(0, ELITE).map((s) => s.g);
  while (next.length < size) next.push(mutate(crossover(tournament(), tournament(), rand), rand, mutation));
  population = next;
  generation++;
}

async function run() {
  while (running) {
    evolve();
    // Yield so pause/config messages are handled between generations.
    await new Promise((r) => setTimeout(r, 0));
  }
}

self.onmessage = (e: MessageEvent<Command>) => {
  const m = e.data;
  if (m.t === "start" && !running) {
    running = true;
    void run();
  } else if (m.t === "pause") running = false;
  else if (m.t === "reset") reset(m.from);
  else if (m.t === "config") {
    if (m.population) size = Math.max(8, Math.min(200, m.population | 0));
    if (m.mutation) mutation = Math.max(0.01, Math.min(0.6, m.mutation));
  }
};

reset();

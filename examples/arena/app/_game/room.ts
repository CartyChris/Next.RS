// One arena match: world + bots + rounds + commentary. Transport-agnostic: the server route
// feeds it socket messages, the browser's offline mode feeds it local input.

import { SEED, think } from "./brain";
import { lines } from "./commentary";
import { TICK, addPlayer, createWorld, hash, resetRound, step, type Input, type World } from "./sim";

export const BOT_NAMES = ["Byte", "Neura", "Pixel", "Quanta", "Torch", "Vector"];
export const COLORS = ["#ff6b6b", "#4dabf7", "#69db7c", "#ffd43b", "#da77f2", "#ff922b", "#38d9a9", "#f783ac"];

export type RosterEntry = { id: number; name: string; color: string; bot: boolean };
export type Result = { name: string; score: number; bot: boolean };
/** [id, x, z, vx, vz, score, energy, stunned] */
export type PlayerState = [number, number, number, number, number, number, number, number];
export type Snapshot = { t: "s"; k: number; r: number; p: PlayerState[]; o: [number, number][] };

export type Advance = { snapshot: Snapshot; lines: string[]; results: Result[] | null };

const r2 = (v: number) => Math.round(v * 100) / 100;

export class Room {
  readonly name: string;
  readonly world: World;
  brain: number[];
  generation = 0;
  private tick = 0;
  private roundSeconds: number;
  private roundLeft: number;
  private leader: number | null = null;
  private streaks = new Map<number, { n: number; at: number }>();

  constructor(name: string, { bots = 3, roundSeconds = 60, brain = SEED }: { bots?: number; roundSeconds?: number; brain?: number[] } = {}) {
    this.name = name;
    this.world = createWorld(hash(name) ^ Date.now());
    this.brain = brain;
    this.roundLeft = roundSeconds;
    this.roundSeconds = roundSeconds;
    for (let i = 0; i < bots; i++) addPlayer(this.world, -(i + 1), BOT_NAMES[i % BOT_NAMES.length], COLORS[(i + 3) % COLORS.length], true);
  }

  get humans() {
    return [...this.world.players.values()].filter((p) => !p.bot).length;
  }

  join(id: number, name: string, color?: string): string {
    const clean = name.replace(/[^\p{L}\p{N} _.-]/gu, "").trim().slice(0, 16) || `Player ${id}`;
    const used = new Set([...this.world.players.values()].map((p) => p.color));
    const hue = color && /^#[0-9a-f]{6}$/i.test(color) ? color : (COLORS.find((c) => !used.has(c)) ?? COLORS[id % COLORS.length]);
    addPlayer(this.world, id, clean, hue, false);
    return lines.join(clean);
  }

  leave(id: number): string | null {
    const p = this.world.players.get(id);
    this.world.players.delete(id);
    return p ? lines.leave(p.name) : null;
  }

  input(id: number, input: Partial<Input>) {
    const p = this.world.players.get(id);
    if (!p || p.bot) return;
    p.input = { x: Number(input.x) || 0, z: Number(input.z) || 0, boost: !!input.boost };
  }

  setBrain(weights: number[], generation: number): string {
    this.brain = weights;
    this.generation = generation;
    return lines.brain(generation);
  }

  roster(): RosterEntry[] {
    return [...this.world.players.values()].map(({ id, name, color, bot }) => ({ id, name, color, bot }));
  }

  /** One fixed step: bots think, physics runs, commentary and round bookkeeping happen. */
  advance(dt = TICK): Advance {
    const w = this.world;
    for (const p of w.players.values()) if (p.bot) p.input = think(this.brain, w, p);
    const said: string[] = [];
    for (const e of step(w, dt)) {
      if (e.type === "knock") said.push(lines.knock(w.players.get(e.by)!.name, w.players.get(e.victim)!.name));
      if (e.type === "collect") {
        const s = this.streaks.get(e.id);
        const n = s && w.time - s.at < 3 ? s.n + 1 : 1;
        this.streaks.set(e.id, { n, at: w.time });
        if (n === 4 || n === 7) said.push(lines.streak(w.players.get(e.id)!.name, n));
      }
    }
    const top = [...w.players.values()].sort((a, b) => b.score - a.score)[0];
    if (top && top.score > 0 && top.id !== this.leader && top.score >= 3) {
      this.leader = top.id;
      said.push(lines.lead(top.name, top.score));
    }
    this.roundLeft -= dt;
    let results: Result[] | null = null;
    if (this.roundLeft <= 0) {
      results = [...w.players.values()].sort((a, b) => b.score - a.score).map((p) => ({ name: p.name, score: p.score, bot: p.bot }));
      if (results[0]) said.push(lines.round(results[0].name, results[0].score, results[0].bot));
      resetRound(w);
      this.roundLeft = this.roundSeconds;
      this.leader = null;
    }
    this.tick++;
    return { snapshot: this.snapshot(), lines: said, results };
  }

  snapshot(): Snapshot {
    const p: PlayerState[] = [];
    for (const q of this.world.players.values()) p.push([q.id, r2(q.x), r2(q.z), r2(q.vx), r2(q.vz), q.score, r2(q.energy), q.stunned > 0 ? 1 : 0]);
    return { t: "s", k: this.tick, r: Math.max(0, Math.round(this.roundLeft * 10) / 10), p, o: this.world.orbs.map((o) => [r2(o.x), r2(o.z)]) };
  }
}

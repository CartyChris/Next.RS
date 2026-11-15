// Bot brains: a tiny multilayer perceptron (10 senses → 12 hidden → 3 actions).
// Weights are a flat number[] so they serialize, mutate and cross over trivially.

import { ARENA, type Input, type Player, type World } from "./sim";

export const LAYERS = [10, 12, 3] as const;
/** Total weights, biases included. */
export const SIZE = LAYERS.slice(1).reduce((n, out, i) => n + out * (LAYERS[i] + 1), 0);

/** What a bot perceives, all roughly in [-1, 1]. */
export function senses(w: World, p: Player): number[] {
  let orb = null as null | { x: number; z: number };
  let best = Infinity;
  for (const o of w.orbs) {
    const d = (o.x - p.x) ** 2 + (o.z - p.z) ** 2;
    if (d < best) [best, orb] = [d, o];
  }
  let foe = null as null | Player;
  let foeD = Infinity;
  for (const q of w.players.values()) {
    if (q === p) continue;
    const d = (q.x - p.x) ** 2 + (q.z - p.z) ** 2;
    if (d < foeD) [foeD, foe] = [d, q];
  }
  const unit = (dx: number, dz: number) => {
    const l = Math.hypot(dx, dz) || 1;
    return [dx / l, dz / l, Math.min(1, l / ARENA)];
  };
  const [ox, oz, od] = orb ? unit(orb.x - p.x, orb.z - p.z) : [0, 0, 1];
  const [fx, fz, fd] = foe ? unit(foe.x - p.x, foe.z - p.z) : [0, 0, 1];
  return [ox, oz, od, fx, fz, fd, p.vx / 11, p.vz / 11, p.x / ARENA, p.z / ARENA];
}

/** Run the network: weights are laid out per neuron as [bias, w0, w1, …]. */
export function forward(weights: ArrayLike<number>, input: number[]): number[] {
  let layer = input;
  let k = 0;
  for (let l = 1; l < LAYERS.length; l++) {
    const next: number[] = new Array(LAYERS[l]);
    for (let j = 0; j < LAYERS[l]; j++) {
      let sum = weights[k++];
      for (let i = 0; i < layer.length; i++) sum += weights[k++] * layer[i];
      next[j] = Math.tanh(sum);
    }
    layer = next;
  }
  return layer;
}

export function think(weights: ArrayLike<number>, w: World, p: Player): Input {
  const [x, z, boost] = forward(weights, senses(w, p));
  return { x, z, boost: boost > 0.4 };
}

/** A hand-made starting brain: steer toward the nearest orb, never boost. Evolution does the rest. */
export const SEED: number[] = (() => {
  const wts = new Array(SIZE).fill(0);
  const hiddenStride = LAYERS[0] + 1;
  const set = (neuron: number, input: number, value: number) => (wts[neuron * hiddenStride + 1 + input] = value);
  set(0, 0, 2.5); // h0 ≈ orb dx
  set(1, 1, 2.5); // h1 ≈ orb dz
  const out = LAYERS[1] * hiddenStride;
  const outStride = LAYERS[1] + 1;
  wts[out + 0 * outStride + 1 + 0] = 2.5; // x ← h0
  wts[out + 1 * outStride + 1 + 1] = 2.5; // z ← h1
  wts[out + 2 * outStride] = -2; // boost bias: off
  return wts;
})();

/** Gaussian-ish noise from a uniform source (sum of three uniforms). */
const noise = (rand: () => number) => (rand() + rand() + rand() - 1.5) * 1.15;

export function mutate(weights: number[], rand: () => number, rate = 0.1, sigma = 0.35): number[] {
  return weights.map((v) => (rand() < rate ? Math.max(-8, Math.min(8, v + noise(rand) * sigma)) : v));
}

export function crossover(a: number[], b: number[], rand: () => number): number[] {
  // Neuron-aligned: keep each neuron's weights together, which preserves learned features.
  const out: number[] = [];
  let k = 0;
  for (let l = 1; l < LAYERS.length; l++) {
    for (let j = 0; j < LAYERS[l]; j++) {
      const from = rand() < 0.5 ? a : b;
      for (let i = 0; i <= LAYERS[l - 1]; i++, k++) out.push(from[k]);
    }
  }
  return out;
}

export function randomBrain(rand: () => number, scale = 1): number[] {
  return Array.from({ length: SIZE }, () => noise(rand) * scale);
}

export function isBrain(value: unknown): value is number[] {
  return Array.isArray(value) && value.length === SIZE && value.every((v) => typeof v === "number" && Number.isFinite(v) && Math.abs(v) <= 8);
}

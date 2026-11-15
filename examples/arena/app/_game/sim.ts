// Arena physics. Pure TypeScript with no DOM or Forge imports, so the exact same code runs
// in the server room (QuickJS), in the browser (offline mode) and in the training worker.

export const ARENA = 12; // half-extent of the square arena
export const PLAYER_R = 0.7;
export const ORB_R = 0.35;
export const ORBS = 10;
export const TICK = 1 / 20;

const ACCEL = 30;
const DRAG = 2.6;
const MAX_SPEED = 11;
const BOOST = 1.9;
const HARD_HIT = 7.5;
const MAX_LOOSE_ORBS = 6;

export type Input = { x: number; z: number; boost: boolean };

export type Player = {
  id: number;
  name: string;
  color: string;
  bot: boolean;
  x: number;
  z: number;
  vx: number;
  vz: number;
  score: number;
  energy: number;
  stunned: number;
  input: Input;
};

export type Orb = { x: number; z: number };

export type GameEvent =
  | { type: "collect"; id: number; x: number; z: number }
  | { type: "knock"; by: number; victim: number; x: number; z: number };

export type World = { players: Map<number, Player>; orbs: Orb[]; rng: () => number; time: number };

/** mulberry32: small, fast, deterministic. */
export function rng(seed: number): () => number {
  let s = seed | 0;
  return () => {
    s = (s + 0x6d2b79f5) | 0;
    let t = Math.imul(s ^ (s >>> 15), 1 | s);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

export const hash = (text: string) => [...text].reduce((h, c) => (Math.imul(h, 31) + c.charCodeAt(0)) | 0, 7);

const clamp = (v: number, lo: number, hi: number) => (v < lo ? lo : v > hi ? hi : v);

function randomSpot(w: World, margin = 1.5): Orb {
  return { x: (w.rng() * 2 - 1) * (ARENA - margin), z: (w.rng() * 2 - 1) * (ARENA - margin) };
}

export function createWorld(seed: number): World {
  const w: World = { players: new Map(), orbs: [], rng: rng(seed), time: 0 };
  for (let i = 0; i < ORBS; i++) w.orbs.push(randomSpot(w));
  return w;
}

export function addPlayer(w: World, id: number, name: string, color: string, bot: boolean): Player {
  let spot = randomSpot(w, 2);
  for (let tries = 0; tries < 20; tries++) {
    const clear = [...w.players.values()].every((p) => (p.x - spot.x) ** 2 + (p.z - spot.z) ** 2 > 9);
    if (clear) break;
    spot = randomSpot(w, 2);
  }
  const p: Player = { id, name, color, bot, x: spot.x, z: spot.z, vx: 0, vz: 0, score: 0, energy: 1, stunned: 0, input: { x: 0, z: 0, boost: false } };
  w.players.set(id, p);
  return p;
}

export function resetRound(w: World) {
  for (const p of w.players.values()) {
    p.score = 0;
    p.energy = 1;
  }
  w.orbs = [];
  for (let i = 0; i < ORBS; i++) w.orbs.push(randomSpot(w));
}

/** Advance the world by `dt` seconds. */
export function step(w: World, dt: number): GameEvent[] {
  const events: GameEvent[] = [];
  const players = [...w.players.values()];

  for (const p of players) {
    let ax = clamp(p.input.x, -1, 1);
    let az = clamp(p.input.z, -1, 1);
    const len = Math.hypot(ax, az);
    if (len > 1) {
      ax /= len;
      az /= len;
    }
    const boosting = p.input.boost && p.energy > 0.05;
    p.energy = clamp(p.energy + (boosting ? -0.9 : 0.25) * dt, 0, 1);
    const accel = ACCEL * (boosting ? BOOST : 1) * (p.stunned > 0 ? 0.3 : 1);
    p.vx += ax * accel * dt;
    p.vz += az * accel * dt;
    const damp = Math.exp(-DRAG * dt);
    p.vx *= damp;
    p.vz *= damp;
    const cap = MAX_SPEED * (boosting ? 1.35 : 1);
    const speed = Math.hypot(p.vx, p.vz);
    if (speed > cap) {
      p.vx *= cap / speed;
      p.vz *= cap / speed;
    }
    p.x += p.vx * dt;
    p.z += p.vz * dt;
    const edge = ARENA - PLAYER_R;
    if (Math.abs(p.x) > edge) {
      p.x = Math.sign(p.x) * edge;
      p.vx *= -0.6;
    }
    if (Math.abs(p.z) > edge) {
      p.z = Math.sign(p.z) * edge;
      p.vz *= -0.6;
    }
    p.stunned = Math.max(0, p.stunned - dt);
  }

  // Player collisions: equal-mass elastic bounce; a hard hit knocks an orb loose.
  for (let i = 0; i < players.length; i++) {
    for (let j = i + 1; j < players.length; j++) {
      const a = players[i];
      const b = players[j];
      const dx = b.x - a.x;
      const dz = b.z - a.z;
      const d = Math.hypot(dx, dz);
      if (d >= 2 * PLAYER_R || d === 0) continue;
      const nx = dx / d;
      const nz = dz / d;
      const push = (2 * PLAYER_R - d) / 2;
      a.x -= nx * push;
      a.z -= nz * push;
      b.x += nx * push;
      b.z += nz * push;
      const towardB = a.vx * nx + a.vz * nz;
      const towardA = -(b.vx * nx + b.vz * nz);
      const vn = towardB + towardA;
      if (vn <= 0) continue;
      a.vx -= vn * nx;
      a.vz -= vn * nz;
      b.vx += vn * nx;
      b.vz += vn * nz;
      if (vn > HARD_HIT) {
        const [by, victim] = towardB >= towardA ? [a, b] : [b, a];
        victim.stunned = 0.6;
        if (victim.score > 0 && w.orbs.length < ORBS + MAX_LOOSE_ORBS) {
          victim.score--;
          const angle = w.rng() * Math.PI * 2;
          const spot = { x: clamp(victim.x + Math.cos(angle) * 2.2, -ARENA + 1, ARENA - 1), z: clamp(victim.z + Math.sin(angle) * 2.2, -ARENA + 1, ARENA - 1) };
          w.orbs.push(spot);
          events.push({ type: "knock", by: by.id, victim: victim.id, x: spot.x, z: spot.z });
        }
      }
    }
  }

  const reach = (PLAYER_R + ORB_R) ** 2;
  for (const p of players) {
    for (let k = 0; k < w.orbs.length; k++) {
      const o = w.orbs[k];
      if ((o.x - p.x) ** 2 + (o.z - p.z) ** 2 > reach) continue;
      p.score++;
      events.push({ type: "collect", id: p.id, x: o.x, z: o.z });
      if (w.orbs.length > ORBS) w.orbs.splice(k--, 1);
      else w.orbs[k] = randomSpot(w);
    }
  }
  w.time += dt;
  return events;
}

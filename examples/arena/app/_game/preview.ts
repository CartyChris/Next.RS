// A 2D top-down match between a brain and two seed bots, for the lab. Canvas 2D only.

import { SEED, think } from "./brain";
import { ARENA, ORB_R, PLAYER_R, TICK, addPlayer, createWorld, step, type World } from "./sim";

export function startPreview(canvas: HTMLCanvasElement, getBrain: () => number[]) {
  const g = canvas.getContext("2d")!;
  let world: World;
  let elapsed = 0;
  const restart = () => {
    world = createWorld(Date.now() & 0xffff);
    addPlayer(world, 1, "champion", "#ffd43b", true);
    addPlayer(world, 2, "seed", "#4dabf7", true);
    addPlayer(world, 3, "seed", "#da77f2", true);
    elapsed = 0;
  };
  restart();
  let raf = 0;
  let acc = 0;
  let last = performance.now();
  const frame = (now: number) => {
    raf = requestAnimationFrame(frame);
    acc += Math.min(0.1, (now - last) / 1000);
    last = now;
    const brain = getBrain();
    while (acc >= TICK) {
      for (const p of world.players.values()) p.input = think(p.id === 1 ? brain : SEED, world, p);
      step(world, TICK);
      acc -= TICK;
      elapsed += TICK;
    }
    if (elapsed > 20) restart();

    const { width: w, height: h } = canvas;
    const k = Math.min(w, h) / (ARENA * 2 + 2);
    g.fillStyle = "#070912";
    g.fillRect(0, 0, w, h);
    g.save();
    g.translate(w / 2, h / 2);
    g.strokeStyle = "#3bc9db";
    g.lineWidth = 2;
    g.strokeRect(-ARENA * k, -ARENA * k, ARENA * 2 * k, ARENA * 2 * k);
    g.fillStyle = "#ffd43b";
    g.shadowColor = "#ffb300";
    g.shadowBlur = 10;
    for (const o of world.orbs) {
      g.beginPath();
      g.arc(o.x * k, o.z * k, ORB_R * k, 0, Math.PI * 2);
      g.fill();
    }
    for (const p of world.players.values()) {
      g.fillStyle = p.color;
      g.shadowColor = p.color;
      g.shadowBlur = p.id === 1 ? 18 : 8;
      g.beginPath();
      g.arc(p.x * k, p.z * k, PLAYER_R * k, 0, Math.PI * 2);
      g.fill();
      g.shadowBlur = 0;
      g.fillStyle = "#f8f9fa";
      g.font = "12px system-ui";
      g.textAlign = "center";
      g.fillText(String(p.score), p.x * k, p.z * k - PLAYER_R * k - 4);
    }
    g.restore();
  };
  raf = requestAnimationFrame(frame);
  return () => cancelAnimationFrame(raf);
}

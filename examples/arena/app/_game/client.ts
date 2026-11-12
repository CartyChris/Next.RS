// Game controller: networking, input, interpolation, HUD state. Falls back to a local
// room (same engine as the server) when there is no server: file:// or static hosting.

import { input as keyboard } from "forge:game";
import { connect, type SocketHandle } from "forge:realtime";
import { Room, type RosterEntry, type Snapshot } from "./room";
import { createView, type Drawn } from "./view";

export type FeedLine = { text: string; ai: boolean; at: number };
export type Hud = {
  status: string;
  offline: boolean;
  you: number | null;
  roundLeft: number;
  generation: number;
  players: (RosterEntry & { score: number; energy: number; you: boolean })[];
  feed: FeedLine[];
};

const INTERPOLATION_MS = 100;

export function startGame(canvas: HTMLCanvasElement, opts: { room: string; name: string; color: string; onHud: (hud: Hud) => void }) {
  const view = createView(canvas);
  const roster = new Map<number, RosterEntry>();
  const snaps: { at: number; s: Snapshot }[] = [];
  const feed: FeedLine[] = [];
  let you: number | null = null;
  let status = "connecting";
  let generation = 0;
  let local: Room | null = null;
  // Newest snapshot: drives the HUD even when rendering is slow.
  let latest: Snapshot | null = null;
  let localTimer: ReturnType<typeof setInterval> | undefined;

  const say = (text: string, ai = false) => {
    feed.unshift({ text, ai, at: Date.now() });
    feed.length = Math.min(feed.length, 6);
  };

  const onMessage = (m: any) => {
    if (m.t === "s") {
      latest = m;
      snaps.push({ at: performance.now(), s: m });
      if (snaps.length > 40) snaps.shift();
    } else if (m.t === "hi") {
      you = m.id;
      generation = m.generation;
    } else if (m.t === "roster") {
      roster.clear();
      for (const p of m.players) roster.set(p.id, p);
    } else if (m.t === "say") say(m.text, m.ai);
    else if (m.t === "brain") generation = m.generation;
  };

  // `connect` may report "offline" synchronously (file://), before it has returned a handle.
  let socket: SocketHandle | null = null;
  const goOffline = () => {
    if (local) return;
    socket?.close();
    status = "offline";
    local = new Room(opts.room, { bots: 4 });
    you = 1;
    say("Offline mode: you vs the bots, simulated in this tab.");
    say(local.join(1, opts.name, opts.color));
    onMessage({ t: "roster", players: local.roster() });
    localTimer = setInterval(() => {
      const out = local!.advance();
      onMessage(out.snapshot);
      for (const line of out.lines) say(line);
    }, 50);
  };

  socket = connect(`/api/room/${encodeURIComponent(opts.room)}`, {
    onMessage,
    onOpen: (s) => s.send({ t: "join", name: opts.name, color: opts.color }),
    onStatus: (s) => {
      if (local) return;
      status = s;
      if (s === "offline") goOffline();
    },
  });
  // Static hosting has no socket server: play locally rather than wait forever.
  const fallback = setTimeout(() => status !== "open" && goOffline(), 4000);

  const keys = keyboard(window);
  let pointer: { x: number; z: number } | null = null;
  const steer = (e: PointerEvent) => {
    const r = canvas.getBoundingClientRect();
    const x = ((e.clientX - r.left) / r.width) * 2 - 1;
    const z = ((e.clientY - r.top) / r.height) * 2 - 1;
    pointer = { x: Math.max(-1, Math.min(1, x * 1.6)), z: Math.max(-1, Math.min(1, z * 1.6)) };
  };
  const down = (e: PointerEvent) => {
    canvas.setPointerCapture(e.pointerId);
    steer(e);
  };
  const move = (e: PointerEvent) => pointer && steer(e);
  const up = () => (pointer = null);
  canvas.addEventListener("pointerdown", down);
  canvas.addEventListener("pointermove", move);
  canvas.addEventListener("pointerup", up);
  canvas.addEventListener("pointercancel", up);

  const sender = setInterval(() => {
    let x: number = keys.axis("KeyA", "KeyD") || keys.axis("ArrowLeft", "ArrowRight");
    let z: number = keys.axis("KeyW", "KeyS") || keys.axis("ArrowUp", "ArrowDown");
    let boost = keys.down("Space") || keys.down("ShiftLeft") || keys.down("ShiftRight");
    if (pointer) {
      ({ x, z } = pointer);
      boost ||= Math.hypot(x, z) > 1.3;
    }
    const input = { x, z, boost };
    if (local) local.input(1, input);
    else socket?.send({ t: "in", ...input });
    keys.endFrame();
  }, 50);

  let raf = 0;
  let last = performance.now();
  const frame = (now: number) => {
    raf = requestAnimationFrame(frame);
    const dt = Math.min(0.1, (now - last) / 1000);
    last = now;
    if (!snaps.length) return view.update([], [], now / 1000, dt);
    const t = now - INTERPOLATION_MS;
    let a = snaps[0];
    let b = snaps[snaps.length - 1];
    for (let i = snaps.length - 1; i > 0; i--) {
      if (snaps[i - 1].at <= t) {
        a = snaps[i - 1];
        b = snaps[i];
        break;
      }
    }
    const alpha = b.at === a.at ? 1 : Math.min(1, Math.max(0, (t - a.at) / (b.at - a.at)));
    const prev = new Map(a.s.p.map((p) => [p[0], p]));
    const drawn: Drawn[] = b.s.p.map((p) => {
      const q = prev.get(p[0]) ?? p;
      const who = roster.get(p[0]);
      return {
        id: p[0],
        x: q[1] + (p[1] - q[1]) * alpha,
        z: q[2] + (p[2] - q[2]) * alpha,
        vx: p[3],
        vz: p[4],
        score: p[5],
        energy: p[6],
        stunned: p[7] === 1,
        name: who?.name ?? "…",
        color: who?.color ?? "#ced4da",
        bot: who?.bot ?? false,
        you: p[0] === you,
      };
    });
    view.update(drawn, b.s.o, now / 1000, dt);
  };
  raf = requestAnimationFrame(frame);

  const hud = setInterval(() => {
    const players = (latest?.p ?? [])
      .map((p) => ({ ...(roster.get(p[0]) ?? { id: p[0], name: "…", color: "#ced4da", bot: false }), score: p[5], energy: p[6], you: p[0] === you }))
      .sort((x, y) => y.score - x.score);
    opts.onHud({ status, offline: !!local, you, roundLeft: latest?.r ?? 0, generation: local ? local.generation : generation, players, feed: [...feed] });
  }, 150);

  return () => {
    cancelAnimationFrame(raf);
    clearInterval(sender);
    clearInterval(hud);
    clearInterval(localTimer);
    clearTimeout(fallback);
    canvas.removeEventListener("pointerdown", down);
    canvas.removeEventListener("pointermove", move);
    canvas.removeEventListener("pointerup", up);
    canvas.removeEventListener("pointercancel", up);
    socket?.close();
    keys.dispose();
    view.dispose();
  };
}

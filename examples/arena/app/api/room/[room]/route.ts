// The authoritative game server for one room.
//
// Every socket (and HTTP request) for /api/room/<name> is pinned to the same JS worker, so
// `rooms` below is the single source of truth for that room. Snapshots go out 20×/s through
// publish(), which fans out in Rust to every subscriber on any worker.

import { chat } from "forge:ai";
import { invalidate, set } from "forge:cache";
import { publish, subscribers } from "forge:realtime";
import { Room } from "../../../_game/room";
import { ROOM_PREFIX, champion, recordRound, type RoomInfo } from "../../../_game/registry";

type Live = {
  room: Room;
  members: Set<ForgeSocket>;
  timer: ReturnType<typeof setInterval>;
  ticks: number;
  emptySince: number | null;
  lastAi: number;
  /** Deploy time of the brain the bots use (0: the seed brain). */
  brainAt: number;
};

const rooms = new Map<string, Live>();
const topic = (name: string) => `arena:${name}`;
let env: ForgeContext["env"] | null = null;

const say = (name: string, text: string, ai = false) => publish(topic(name), { t: "say", text, ai });

function info(name: string, room: Room): RoomInfo {
  const top = [...room.world.players.values()].sort((a, b) => b.score - a.score)[0];
  return { name, humans: room.humans, bots: room.world.players.size - room.humans, leader: top && top.score > 0 ? { name: top.name, score: top.score } : null };
}

function openRoom(name: string): Live {
  const existing = rooms.get(name);
  if (existing) return existing;
  const best = champion();
  const room = new Room(name, { bots: 3, brain: best.weights });
  room.generation = best.generation;
  const live: Live = { room, members: new Set(), ticks: 0, emptySince: null, lastAi: 0, brainAt: best.at, timer: 0 as never };
  live.timer = setInterval(() => tick(name, live), 50);
  rooms.set(name, live);
  return live;
}

function tick(name: string, live: Live) {
  const { room } = live;
  const out = room.advance();
  publish(topic(name), out.snapshot);
  for (const line of out.lines) say(name, line);
  if (out.results) {
    publish(topic(name), { t: "round", results: out.results });
    recordRound(name, out.results[0]);
    void aiCaster(name, live, out.results);
  }
  if (++live.ticks % 40 === 0) {
    // Every 2 s: pick up a newly deployed brain, refresh the lobby.
    const best = champion();
    if (best.at !== live.brainAt) {
      live.brainAt = best.at;
      say(name, room.setBrain(best.weights, best.generation));
      publish(topic(name), { t: "brain", generation: best.generation });
    }
    const summary = info(name, room);
    set(ROOM_PREFIX + name, summary, { ttl: 10 });
    publish("lobby", { t: "room", room: summary });
  }
  if (live.members.size > 0) live.emptySince = null;
  else if ((live.emptySince ??= Date.now()) < Date.now() - 15_000) {
    clearInterval(live.timer);
    rooms.delete(name);
    invalidate(ROOM_PREFIX + name);
    publish("lobby", { t: "closed", name });
  }
}

/** With OPENAI_API_KEY / OPENAI_BASE_URL set, a model adds a line after each round. */
async function aiCaster(name: string, live: Live, results: { name: string; score: number; bot: boolean }[]) {
  if (!env || !(env.OPENAI_API_KEY || env.OPENAI_BASE_URL) || Date.now() - live.lastAi < 8000) return;
  live.lastAi = Date.now();
  const scores = results.slice(0, 5).map((r) => `${r.name}${r.bot ? " (bot)" : ""} ${r.score}`).join(", ");
  try {
    const text = await chat({
      provider: (env.ARENA_AI_PROVIDER as never) ?? "openai",
      baseUrl: env.OPENAI_BASE_URL,
      apiKey: env.OPENAI_API_KEY,
      model: env.OPENAI_MODEL ?? "gpt-4o-mini",
      maxTokens: 60,
      system: "You are an excitable esports caster for a neon arena where humans and neural-network bots collect glowing orbs. One punchy sentence, at most 20 words, no emojis.",
      messages: [{ role: "user", content: `Round over in room "${name}". Final scores: ${scores}. Call it.` }],
      signal: AbortSignal.timeout(6000),
    }).text();
    if (text.trim()) say(name, text.trim(), true);
  } catch (e) {
    console.warn("ai caster:", (e as Error).message);
  }
}

export const socket: ForgeSocketHandlers<{ room: string }> = {
  open(ws, ctx) {
    env = ctx.env;
    const name = ws.params.room;
    const live = openRoom(name);
    live.members.add(ws);
    ws.subscribe(topic(name));
    ws.send({ t: "hi", id: ws.id, room: name, generation: live.room.generation });
    ws.send({ t: "roster", players: live.room.roster() });
  },
  message(ws, msg) {
    const name = ws.params.room;
    const live = rooms.get(name);
    if (!live || typeof msg !== "object" || msg === null) return;
    if (msg.t === "in") live.room.input(ws.id, msg);
    else if (msg.t === "join" && !ws.data.joined) {
      ws.data.joined = true;
      say(name, live.room.join(ws.id, String(msg.name ?? ""), msg.color));
      publish(topic(name), { t: "roster", players: live.room.roster() });
    }
  },
  close(ws) {
    const name = ws.params.room;
    const live = rooms.get(name);
    if (!live) return;
    live.members.delete(ws);
    if (ws.data.joined) {
      const line = live.room.leave(ws.id);
      if (line) say(name, line);
      publish(topic(name), { t: "roster", players: live.room.roster() });
    }
  },
};

/** Room status over HTTP (runs on the room's worker, so it sees the live state). */
export function GET(_request: Request, { params }: ForgeContext<{ room: string }>) {
  const live = rooms.get(params.room);
  if (!live) return { name: params.room, open: false };
  return { ...info(params.room, live.room), open: true, watching: subscribers(topic(params.room)), roster: live.room.roster() };
}

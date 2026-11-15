// Server-side shared state in forge:cache (visible to every worker): open rooms,
// the leaderboard, and the champion brain trained in the lab.

import { get, keys, set } from "forge:cache";
import { SEED } from "./brain";
import type { Result } from "./room";

export const ROOM_PREFIX = "arena:room:";
const BOARD_PREFIX = "arena:board:";
const CHAMPION = "arena:champion";

export type RoomInfo = { name: string; humans: number; bots: number; leader: { name: string; score: number } | null };
export type Champion = { weights: number[]; generation: number; fitness: number; by: string; at: number };
export type BoardEntry = Result & { room: string; at: number };

export function champion(): Champion {
  return get<Champion>(CHAMPION) ?? { weights: SEED, generation: 0, fitness: 0, by: "seed", at: 0 };
}

export function saveChampion(next: Champion) {
  set(CHAMPION, next, { ttl: 30 * 86400 });
}

export function listRooms(): RoomInfo[] {
  return keys(ROOM_PREFIX)
    .map((k) => get<RoomInfo>(k))
    .filter((r): r is RoomInfo => !!r)
    .sort((a, b) => b.humans - a.humans || a.name.localeCompare(b.name));
}

export function recordRound(room: string, winner: Result | undefined) {
  if (!winner || winner.score === 0) return;
  const at = Date.now();
  set(`${BOARD_PREFIX}${String(at).padStart(15, "0")}:${room}`, { ...winner, room, at }, { ttl: 86400 });
}

export function leaderboard(limit = 10): BoardEntry[] {
  return keys(BOARD_PREFIX)
    .map((k) => get<BoardEntry>(k))
    .filter((e): e is BoardEntry => !!e)
    .sort((a, b) => b.score - a.score || b.at - a.at)
    .slice(0, limit);
}

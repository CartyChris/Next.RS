// The brain every bot in every room uses. The lab trains one in a Web Worker and POSTs it here;
// rooms notice within 2 seconds and swap it in live.
import { publish } from "forge:realtime";
import { isBrain } from "../../_game/brain";
import { champion, saveChampion } from "../../_game/registry";

export function GET() {
  return champion();
}

export async function POST(request: Request) {
  const body = (await request.json().catch(() => null)) as Record<string, unknown> | null;
  if (!body || !isBrain(body.weights)) {
    return Response.json({ error: "weights must be the network's numbers, each finite and within ±8" }, { status: 400 });
  }
  const previous = champion().generation;
  const next = {
    weights: body.weights,
    generation: Math.max(0, Math.floor(Number(body.generation) || 0)),
    fitness: Number(body.fitness) || 0,
    by: String(body.by ?? "lab").replace(/[^\p{L}\p{N} _.-]/gu, "").slice(0, 24) || "lab",
    at: Date.now(),
  };
  saveChampion(next);
  publish("lobby", { t: "champion", champion: { generation: next.generation, fitness: next.fitness, by: next.by } });
  return { ok: true, previous, generation: next.generation };
}

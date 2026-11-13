import { champion, leaderboard, listRooms } from "./_game/registry";

// Runs per request on the server, and once at build time for static / single-html builds.
export function loader() {
  const best = champion();
  return {
    rooms: listRooms(),
    leaderboard: leaderboard(),
    champion: { generation: best.generation, fitness: best.fitness, by: best.by },
  };
}

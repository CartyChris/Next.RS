import { useState, type FormEvent } from "react";
import { useData } from "forge:data";
import { Link, navigate } from "forge:navigation";
import { useSocket } from "forge:realtime";
import { usePersistentState } from "forge:store";
import { COLORS } from "./_game/room";
import type { BoardEntry, RoomInfo } from "./_game/registry";

type Data = { rooms: RoomInfo[]; leaderboard: BoardEntry[]; champion: { generation: number; fitness: number; by: string } };

export const meta = {
  title: "Forge Arena",
  description: "Real-time multiplayer arena with neural-network bots, built on Next.RS.",
  themeColor: "#05060d",
};

const SUGGESTED = ["neon", "grid", "orbit"];

const FEATURES: [string, string][] = [
  ["Streaming SSR + loader", "This lobby is server-rendered by QuickJS inside the Rust binary, then hydrated."],
  ["Stateful socket rooms", "Every socket for /api/room/<name> lands on one JS worker, so the room's memory is the game state."],
  ["Rust pub/sub", "20 snapshots/s per room fan out to every player in Rust, across workers."],
  ["Neural nets in a Web Worker", "The lab evolves bot brains in your browser; deploy one and every room hot-swaps it."],
  ["forge:ai caster", "With an OpenAI-compatible key set, a model calls each round; offline, templates do."],
  ["One HTML file", "forge build --target single-html: the whole arena, offline, vs bots, from a USB stick."],
];

export default function Lobby() {
  const data = useData<Data>();
  const [rooms, setRooms] = useState(data.rooms);
  const [champion, setChampion] = useState(data.champion);
  const [name, setName] = usePersistentState("arena:name", "");
  const [color, setColor] = usePersistentState("arena:color", COLORS[1]);
  const [custom, setCustom] = useState("");
  const { status } = useSocket("/api/lobby", (m) => {
    if (m.t === "rooms") setRooms(m.rooms);
    else if (m.t === "room") setRooms((list) => [...list.filter((r) => r.name !== m.room.name), m.room].sort((a, b) => b.humans - a.humans || a.name.localeCompare(b.name)));
    else if (m.t === "closed") setRooms((list) => list.filter((r) => r.name !== m.name));
    else if (m.t === "champion") setChampion(m.champion);
  });

  const live = new Map(rooms.map((r) => [r.name, r]));
  const listed = [...rooms.map((r) => r.name), ...SUGGESTED.filter((s) => !live.has(s))];
  const online = rooms.reduce((n, r) => n + r.humans, 0);

  const create = (e: FormEvent) => {
    e.preventDefault();
    const slug = custom.toLowerCase().replace(/[^a-z0-9-]+/g, "-").replace(/^-|-$/g, "").slice(0, 24);
    if (slug) navigate(`/play/${slug}`);
  };

  return (
    <main className="lobby">
      <section className="hero">
        <h1>
          Collect the light.
          <br />
          <span>Outplay the machines.</span>
        </h1>
        <p className="lede">
          A real-time 3D arena where people and neural-network bots race for glowing orbs. Ram rivals to knock orbs loose. Train better
          bots in the lab and deploy them to every room.
        </p>
        <div className="stats">
          <div>
            <b>{online}</b> playing now
          </div>
          <div>
            <b>{rooms.length}</b> live rooms
          </div>
          <div>
            <b>gen {champion.generation}</b> bot brain
          </div>
          <div className={`pill ${status}`}>{status === "open" ? "live" : status}</div>
        </div>
      </section>

      <section className="grid">
        <div className="card">
          <h2>You</h2>
          <label>
            Name
            <input value={name} maxLength={16} placeholder="Guest" onChange={(e) => setName(e.target.value)} />
          </label>
          <div className="swatches" role="radiogroup" aria-label="Color">
            {COLORS.map((c) => (
              <button key={c} type="button" aria-label={c} aria-checked={c === color} role="radio" className={c === color ? "swatch on" : "swatch"} style={{ background: c }} onClick={() => setColor(c)} />
            ))}
          </div>
          <form onSubmit={create} className="row">
            <input value={custom} onChange={(e) => setCustom(e.target.value)} placeholder="new-room-name" aria-label="Room name" />
            <button className="primary">Create</button>
          </form>
        </div>

        <div className="card">
          <h2>Rooms</h2>
          <ul className="rooms">
            {listed.map((room) => {
              const r = live.get(room);
              return (
                <li key={room}>
                  <div>
                    <b>{room}</b>
                    <small>{r ? `${r.humans} human${r.humans === 1 ? "" : "s"} · ${r.bots} bots${r.leader ? ` · ${r.leader.name} leads (${r.leader.score})` : ""}` : "empty: start it"}</small>
                  </div>
                  <Link to={`/play/${room}`} className="primary">
                    Join
                  </Link>
                </li>
              );
            })}
          </ul>
        </div>

        <div className="card">
          <h2>Leaderboard</h2>
          {data.leaderboard.length === 0 ? (
            <p className="muted">No rounds finished yet. Rounds last 60 seconds.</p>
          ) : (
            <ol className="board">
              {data.leaderboard.map((e) => (
                <li key={`${e.at}${e.room}`}>
                  <span>
                    {e.bot ? "🤖 " : ""}
                    {e.name} <small>in {e.room}</small>
                  </span>
                  <b>{e.score}</b>
                </li>
              ))}
            </ol>
          )}
        </div>

        <div className="card">
          <h2>Bot brain</h2>
          <p>
            Generation <b>{champion.generation}</b>
            {champion.generation > 0 ? ` (fitness ${champion.fitness.toFixed(2)}, trained by ${champion.by})` : ": the hand-made seed brain."}
          </p>
          <p className="muted">A 10 → 12 → 3 neural network sees the nearest orb, the nearest rival, its velocity and the walls.</p>
          <Link to="/lab" className="primary">
            Train a better one →
          </Link>
        </div>
      </section>

      <section className="proof">
        <h2>Proof of life</h2>
        <p className="muted">Everything on this site runs on one Next.RS binary. What each part exercises:</p>
        <div className="features">
          {FEATURES.map(([title, text]) => (
            <div key={title}>
              <b>{title}</b>
              <p>{text}</p>
            </div>
          ))}
        </div>
      </section>
    </main>
  );
}

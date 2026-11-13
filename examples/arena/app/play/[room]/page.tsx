import { useEffect, useRef, useState } from "react";
import { Link, useParams } from "forge:navigation";
import { usePersistentState } from "forge:store";
import { startGame, type Hud } from "../../_game/client";

// WebGL and three.js only make sense in the browser: never evaluated on the server.
export const ssr = false;

export const meta = ({ params }: { params: { room: string } }) => ({ title: `${params.room} · Forge Arena` });

const fmt = (s: number) => `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, "0")}`;

export default function Play() {
  const { room } = useParams<{ room: string }>();
  const [name] = usePersistentState("arena:name", "");
  const [color] = usePersistentState("arena:color", "");
  const canvas = useRef<HTMLCanvasElement>(null);
  const [hud, setHud] = useState<Hud | null>(null);

  useEffect(() => startGame(canvas.current!, { room, name: name || "Guest", color, onHud: setHud }), [room]);

  return (
    <div className="play">
      <canvas ref={canvas} className="stage" aria-label="Arena" />
      <div className="hud tl">
        <div className="room">
          <span>#{room}</span>
          <span className={`pill ${hud?.status ?? "connecting"}`}>{hud?.offline ? "offline vs bots" : hud?.status === "open" ? "live" : (hud?.status ?? "connecting")}</span>
        </div>
        <div className="timer" id="timer">
          {fmt(hud?.roundLeft ?? 60)}
        </div>
        <small className="muted">bot brain gen {hud?.generation ?? 0}</small>
      </div>
      <ol className="hud tr scores" id="scores">
        {hud?.players.map((p) => (
          <li key={p.id} className={p.you ? "you" : ""}>
            <i style={{ background: p.color }} />
            <span>
              {p.bot ? "🤖 " : ""}
              {p.name}
            </span>
            <b>{p.score}</b>
            <em style={{ width: `${Math.round(p.energy * 100)}%` }} />
          </li>
        ))}
      </ol>
      <ul className="hud bl feed" id="feed">
        {hud?.feed.map((line) => (
          <li key={line.at + line.text} className={line.ai ? "ai" : ""}>
            {line.ai ? "🎙 " : ""}
            {line.text}
          </li>
        ))}
      </ul>
      <div className="hud br help">
        <kbd>WASD</kbd> / <kbd>↑↓←→</kbd> or drag to steer · <kbd>Space</kbd> boost · ram rivals to knock orbs loose
        <Link to="/" className="leave">
          ← Lobby
        </Link>
      </div>
    </div>
  );
}

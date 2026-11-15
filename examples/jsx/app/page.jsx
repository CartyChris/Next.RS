// A plain JSX page. `usePersistentState` keeps the score in localStorage,
// so it survives reloads even when this app is a single HTML file opened from disk.
import { usePersistentState } from "forge:store";
import "./styles.css";

export const meta = { title: "Mini Clicker" };

export default function Page() {
  const [score, setScore] = usePersistentState("clicker:score", 0);
  const [perClick, setPerClick] = usePersistentState("clicker:power", 1);
  const upgradeCost = perClick * 10;

  return (
    <main>
      <div>
        <h1>Mini Clicker</h1>
        <p className="score" id="score">{score}</p>
        <button id="gain" onClick={() => setScore((s) => s + perClick)}>
          Gain {perClick} {perClick === 1 ? "point" : "points"}
        </button>
        <button
          className="secondary"
          disabled={score < upgradeCost}
          onClick={() => {
            setScore((s) => s - upgradeCost);
            setPerClick((p) => p + 1);
          }}
        >
          Upgrade ({upgradeCost})
        </button>
      </div>
    </main>
  );
}

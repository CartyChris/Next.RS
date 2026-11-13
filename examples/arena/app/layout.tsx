import type { ReactNode } from "react";
import { Link, usePathname } from "forge:navigation";
import "./arena.css";

export default function Layout({ children }: { children: ReactNode }) {
  const path = usePathname();
  const playing = path.startsWith("/play/");
  return (
    <div className={playing ? "shell playing" : "shell"}>
      <header className="nav">
        <Link to="/" className="brand">
          <span className="logo">◆</span> FORGE ARENA
        </Link>
        <nav>
          <Link to="/" className={path === "/" ? "active" : ""}>
            Lobby
          </Link>
          <Link to="/lab" className={path === "/lab" ? "active" : ""}>
            Lab
          </Link>
          <Link to="/play/solo" className={playing ? "active" : ""}>
            Play
          </Link>
        </nav>
      </header>
      {children}
    </div>
  );
}

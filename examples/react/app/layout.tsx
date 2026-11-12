import type { ReactNode } from "react";
import { Link } from "forge:navigation";
import "./globals.css";

// Layouts wrap every page below them and keep their state across navigations.
export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <>
      <header>
        <strong>Forge</strong>
        <Link to="/">Home</Link>
        <Link to="/blog">Blog</Link>
        <Link to="/slow">Streaming</Link>
      </header>
      {children}
    </>
  );
}

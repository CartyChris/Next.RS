import type { ReactNode } from "react";
import { Theme } from "forge:ui";
import "./model-lab.css";

export default function Layout({ children }: { children: ReactNode }) {
  return <Theme preset="graphite" theme="dark" accent="verdigris" density="comfortable" className="model-lab-theme">{children}</Theme>;
}

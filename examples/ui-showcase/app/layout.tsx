import { Container, Nav, NavLink, Select, Theme } from "@nextrs/ui";
import { href, usePathname } from "forge:navigation";
import { useState, type ReactNode } from "react";
import "@nextrs/ui/styles.css";
import "./relay.css";

type ColorTheme = "system" | "light" | "dark";

function isColorTheme(value: string): value is ColorTheme {
  return value === "system" || value === "light" || value === "dark";
}

export default function RootLayout({ children }: { children: ReactNode }) {
  const pathname = usePathname();
  const [theme, setTheme] = useState<ColorTheme>("system");

  return (
    <Theme theme={theme} className="relay">
      <div className="relay-shell">
        <aside className="relay-sidebar">
          <a className="relay-brand" href={href("/")} aria-label="Relay home">
            <span className="relay-mark" aria-hidden="true" />
            Relay
          </a>
          <Nav label="Workspace" className="relay-nav">
            <NavLink href={href("/")} current={pathname === "/"}>
              <span className="relay-nav-icon relay-nav-icon--overview" aria-hidden="true" />
              Overview
            </NavLink>
            <NavLink href={href("/guide")} current={pathname === "/guide"}>
              <span className="relay-nav-icon relay-nav-icon--guide" aria-hidden="true" />
              Component guide
            </NavLink>
          </Nav>
          <div className="relay-sidebar-foot">
            <span className="relay-sidebar-foot-dot" aria-hidden="true" />
            Forge UI showcase
          </div>
        </aside>
        <div className="relay-main-column">
          <header className="relay-topbar">
            <Container size="wide" className="relay-topbar-inner">
              <p className="relay-breadcrumb">
                Workspace <span aria-hidden="true">/</span>{" "}
                <strong>{pathname === "/guide" ? "Component guide" : "Overview"}</strong>
              </p>
              <Select
                label="Color theme"
                className="relay-theme-select"
                value={theme}
                onChange={({ currentTarget }) => {
                  if (isColorTheme(currentTarget.value)) setTheme(currentTarget.value);
                }}
              >
                <option value="system">System</option>
                <option value="light">Light</option>
                <option value="dark">Dark</option>
              </Select>
            </Container>
          </header>
          {children}
        </div>
      </div>
    </Theme>
  );
}

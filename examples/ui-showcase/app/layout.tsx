import {
  CommandPalette, Container, Icon, IconButton, Nav, NavLink, Select, Theme, useDisclosure, useHotkey,
} from "forge:ui";
import { href, usePathname } from "forge:navigation";
import { useState, type ReactNode } from "react";
import "./relay.css";

type ColorTheme = "system" | "light" | "dark";

function isColorTheme(value: string): value is ColorTheme {
  return value === "system" || value === "light" || value === "dark";
}

export default function RootLayout({ children }: { children: ReactNode }) {
  const pathname = usePathname();
  const [theme, setTheme] = useState<ColorTheme>("system");
  const palette = useDisclosure();
  useHotkey("mod+k", (event) => {
    event.preventDefault();
    palette.toggle();
  });
  const title = pathname === "/guide" ? "Component guide" : pathname === "/components" ? "Component gallery" : "Overview";

  return (
    <Theme theme={theme} accent="violet" className="relay">
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
            <NavLink href={href("/components")} current={pathname === "/components"}>
              <span className="relay-nav-icon relay-nav-icon--gallery" aria-hidden="true" />
              Component gallery
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
                <strong>{title}</strong>
              </p>
              <IconButton label="Search commands" size="sm" className="relay-search" onClick={palette.onOpen}>
                <Icon name="search" />
              </IconButton>
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
      <CommandPalette
        open={palette.open}
        onOpenChange={palette.setOpen}
        commands={[
          { id: "overview", label: "Overview", group: "Go to", href: "/", icon: <Icon name="home" /> },
          { id: "guide", label: "Component guide", group: "Go to", href: "/guide", icon: <Icon name="info" /> },
          { id: "gallery", label: "Component gallery", group: "Go to", href: "/components", icon: <Icon name="spark" /> },
          { id: "light", label: "Use light theme", group: "Theme", icon: <Icon name="sun" />, onSelect: () => setTheme("light") },
          { id: "dark", label: "Use dark theme", group: "Theme", icon: <Icon name="moon" />, onSelect: () => setTheme("dark") },
          { id: "system", label: "Follow system theme", group: "Theme", icon: <Icon name="settings" />, onSelect: () => setTheme("system") },
        ]}
      />
    </Theme>
  );
}

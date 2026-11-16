// The app frame: sidebar navigation, top bar, command palette (Ctrl/Cmd+K) and toasts.
// Change `accent`, `radius` and `density` on <Theme> to restyle the whole app.
import {
  AppShell, CommandPalette, Icon, IconButton, Nav, NavLink, Row, Text, Theme, Toaster, Tooltip,
  useDisclosure, useHotkey, useThemeMode,
} from "forge:ui";
import type { ReactNode } from "react";
import "./starter.css";

const pages = [
  { href: "/", label: "Projects", icon: "home" },
  { href: "/settings", label: "Settings", icon: "settings" },
] as const;

export default function RootLayout({ children }: { children: ReactNode }) {
  const [mode, setMode] = useThemeMode("system");
  const palette = useDisclosure();
  useHotkey("mod+k", (event) => {
    event.preventDefault();
    palette.toggle();
  });
  const nextMode = mode === "dark" ? "light" : "dark";

  return (
    <Theme theme={mode} accent="ember">
      <AppShell
        brand={<><Icon name="spark" size={20} /> Starter</>}
        nav={
          <Nav label="Main" orientation="vertical">
            {pages.map((page) => (
              <NavLink key={page.href} href={page.href} matchPrefix>
                <Icon name={page.icon} /> {page.label}
              </NavLink>
            ))}
          </Nav>
        }
        sidebarFooter={<Text size="xs" tone="muted">Built with Next.RS</Text>}
        header={
          <Row justify="end" className="starter-tools">
            <Tooltip content="Search (Ctrl/⌘ K)" side="bottom">
              <IconButton label="Search" onClick={palette.onOpen}><Icon name="search" /></IconButton>
            </Tooltip>
            <IconButton label={`Use ${nextMode} theme`} onClick={() => setMode(nextMode)}>
              <Icon name={mode === "dark" ? "sun" : "moon"} />
            </IconButton>
          </Row>
        }
      >
        {children}
      </AppShell>
      <CommandPalette
        open={palette.open}
        onOpenChange={palette.setOpen}
        commands={[
          ...pages.map((page) => ({ id: page.href, label: page.label, group: "Go to", href: page.href, icon: <Icon name={page.icon} /> })),
          { id: "theme", label: `Use ${nextMode} theme`, group: "Theme", onSelect: () => setMode(nextMode) },
        ]}
      />
      <Toaster />
    </Theme>
  );
}

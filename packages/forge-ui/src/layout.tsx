import { useState } from "react";
import type { ReactNode } from "react";
import { cx, getAdapter } from "./config";
import { Drawer } from "./dialog";
import { Icon } from "./icon";

export interface AppShellProps {
  /** The product name or logo, shown at the top of the sidebar. */
  brand: ReactNode;
  /** The main navigation, usually `<Nav orientation="vertical">` with `NavLink`s. */
  nav: ReactNode;
  /** Top bar content: breadcrumbs, search, account menu. */
  header?: ReactNode;
  /** Pinned to the bottom of the sidebar. */
  sidebarFooter?: ReactNode;
  children: ReactNode;
  /** The label of the button that opens navigation on small screens. */
  menuLabel?: string;
  className?: string;
}

/**
 * The frame of an app: a sidebar with navigation, a top bar, and the page. On small screens
 * the sidebar becomes a drawer behind a menu button, and it closes when the route changes.
 */
export function AppShell({
  brand, nav, header, sidebarFooter, children, menuLabel = "Open navigation", className,
}: AppShellProps) {
  const pathname = getAdapter().usePathname?.();
  const [openAt, setOpenAt] = useState<string | null | undefined>(undefined);
  // Open only while still on the route where it was opened.
  const open = openAt !== undefined && openAt === (pathname ?? null);
  return (
    <div className={cx("forge-ui-shell", className)}>
      <aside className="forge-ui-shell-sidebar">
        <div className="forge-ui-shell-brand">{brand}</div>
        <div className="forge-ui-shell-nav">{nav}</div>
        {sidebarFooter != null && <div className="forge-ui-shell-sidebar-footer">{sidebarFooter}</div>}
      </aside>
      <div className="forge-ui-shell-main">
        <header className="forge-ui-shell-header">
          <button type="button" className="forge-ui-button forge-ui-button--ghost forge-ui-icon-button forge-ui-shell-menu"
            aria-label={menuLabel} onClick={() => setOpenAt(pathname ?? null)}>
            <Icon name="menu" size={20} />
          </button>
          <div className="forge-ui-shell-header-content">{header}</div>
        </header>
        <div className="forge-ui-shell-content">{children}</div>
      </div>
      <Drawer open={open} onOpenChange={(next) => setOpenAt(next ? pathname ?? null : undefined)} placement="left"
        size="sm" title={brand} closeLabel="Close navigation">
        {open && nav}
      </Drawer>
    </div>
  );
}

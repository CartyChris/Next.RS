import type { AnchorHTMLAttributes, ComponentType } from "react";

/**
 * How Forge UI talks to the app's router. The built-in `forge:ui` module wires this to
 * `forge:navigation` for you; with the `@nextrs/ui` package, call `configure()` once.
 */
export interface UiAdapter {
  /** Renders in-app links (client transitions, prefetch). Receives anchor props and `href`. */
  Link?: ComponentType<AnchorHTMLAttributes<HTMLAnchorElement> & { href: string }>;
  /** Turns an app path into the `href` to render (`#/path` in single-file builds). */
  href?: (to: string) => string;
  /** Hook returning the current pathname, for automatic `aria-current`. */
  usePathname?: () => string;
  /** Imperative navigation (command palette, menus). */
  navigate?: (to: string) => void;
}

const adapter: UiAdapter = {};

export function configure(next: UiAdapter): void {
  Object.assign(adapter, next);
}

export function getAdapter(): Readonly<UiAdapter> {
  return adapter;
}

/** `mailto:`, `https://…`, `//host`, and in-page `#fragment` links are never routed. */
export function isExternal(href: string): boolean {
  return /^(?:[a-z][a-z0-9+.-]*:|\/\/|#)/i.test(href);
}

/** The path an href points at, without hash-routing prefix, query or fragment. */
export function pathOf(href: string): string {
  const path = href.replace(/^#(?=\/)/, "").split(/[?#]/)[0];
  return path.length > 1 ? path.replace(/\/+$/, "") : path;
}

/** Join class names, skipping falsy values. */
export function cx(...parts: Array<string | false | null | undefined>): string {
  return parts.filter(Boolean).join(" ");
}

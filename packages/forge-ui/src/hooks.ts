import { useCallback, useEffect, useRef, useState } from "react";
import type { ThemeMode } from "./primitives";

/** Open/closed state with stable handlers, for dialogs, drawers and palettes. */
export function useDisclosure(initial = false) {
  const [open, setOpen] = useState(initial);
  const onOpen = useCallback(() => setOpen(true), []);
  const onClose = useCallback(() => setOpen(false), []);
  const toggle = useCallback(() => setOpen((value) => !value), []);
  return { open, setOpen, onOpen, onClose, toggle };
}

/**
 * Call `handler` on a key combination such as `"mod+k"`, `"shift+?"` or `"escape"`.
 * `mod` is Command on Apple devices and Control elsewhere. Typing in a field does not
 * trigger plain-key combos.
 */
export function useHotkey(combo: string, handler: (event: KeyboardEvent) => void, enabled = true): void {
  const latest = useRef(handler);
  latest.current = handler;
  useEffect(() => {
    if (!enabled) return;
    const parts = combo.toLowerCase().split("+").map((part) => part.trim());
    const key = parts[parts.length - 1];
    const want = (name: string) => parts.includes(name);
    const apple = /mac|iphone|ipad/i.test(navigator.platform ?? "");
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key.toLowerCase() !== key) return;
      const mod = apple ? event.metaKey : event.ctrlKey;
      if (want("mod") !== mod) return;
      if (!want("mod") && (event.metaKey || event.ctrlKey) !== (want("ctrl") || want("meta"))) return;
      if (want("shift") !== event.shiftKey && key.length > 1) return;
      if (want("alt") !== event.altKey) return;
      const target = event.target;
      const typing = target instanceof HTMLElement &&
        (target.isContentEditable || /^(input|textarea|select)$/i.test(target.tagName));
      if (typing && !want("mod") && !want("ctrl") && !want("alt") && key !== "escape") return;
      latest.current(event);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [combo, enabled]);
}

/**
 * A color theme that survives reloads. Starts as `initial` on the server and on first
 * paint (so hydration matches), then adopts the stored choice.
 */
export function useThemeMode(initial: ThemeMode = "system", storageKey = "forge-ui-theme") {
  const [mode, setMode] = useState<ThemeMode>(initial);
  useEffect(() => {
    try {
      const stored = localStorage.getItem(storageKey);
      if (stored === "light" || stored === "dark" || stored === "system") setMode(stored);
    } catch {
      // Storage can be unavailable (private mode, sandboxed frames).
    }
  }, [storageKey]);
  const set = useCallback((next: ThemeMode) => {
    setMode(next);
    try {
      localStorage.setItem(storageKey, next);
    } catch {
      // Keep the in-memory choice.
    }
  }, [storageKey]);
  return [mode, set] as const;
}

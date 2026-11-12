// forge:store: persistent local state (localStorage). Works from file:// too,
// which makes it the save-game / settings layer for single-file apps.
import { rt } from "./shared.js";

const read = (key, fallback) => {
  try {
    const raw = localStorage.getItem(key);
    return raw == null ? fallback : JSON.parse(raw);
  } catch {
    return fallback;
  }
};

export const store = {
  get: read,
  set(key, value) {
    try {
      localStorage.setItem(key, JSON.stringify(value));
    } catch {} // private mode / quota: state stays in memory
  },
  remove(key) {
    try {
      localStorage.removeItem(key);
    } catch {}
  },
};

/** `useState` that survives reloads and syncs across tabs. Server-rendered pages hydrate with `initial` first. */
export function usePersistentState(key, initial) {
  const { useState, useEffect, useCallback } = rt.React;
  const ssr = rt.server || rt.hydrating;
  const [value, setValue] = useState(() => (ssr ? initial : read(key, initial)));
  useEffect(() => {
    if (ssr) setValue(read(key, initial));
    const onStorage = (e) => e.key === key && setValue(read(key, initial));
    addEventListener("storage", onStorage);
    return () => removeEventListener("storage", onStorage);
  }, [key]);
  const set = useCallback(
    (next) =>
      setValue((prev) => {
        const v = typeof next === "function" ? next(prev) : next;
        store.set(key, v);
        return v;
      }),
    [key],
  );
  return [value, set];
}

// forge:cache (server): a process-wide TTL cache shared by all workers, with tag invalidation.
import { host } from "./host-ref.js";

const inflight = new Map();

/** Memoise `fn()` under `key` for `ttl` seconds. Concurrent callers share one computation. */
export async function cache(key, fn, { ttl = 60, tags = [] } = {}) {
  const hit = host.cacheGet(key);
  if (hit !== undefined) return JSON.parse(hit);
  let p = inflight.get(key);
  if (!p) {
    p = Promise.resolve()
      .then(fn)
      .then((value) => {
        host.cacheSet(key, JSON.stringify(value ?? null), ttl * 1000, tags.join(","));
        return value ?? null;
      })
      .finally(() => inflight.delete(key));
    inflight.set(key, p);
  }
  return p;
}

/** The stored value, or `undefined`. */
export function get(key) {
  const hit = host.cacheGet(String(key));
  return hit === undefined ? undefined : JSON.parse(hit);
}

/** Store a JSON-serialisable value for `ttl` seconds (default: a day). */
export function set(key, value, { ttl = 86400, tags = [] } = {}) {
  host.cacheSet(String(key), JSON.stringify(value ?? null), ttl * 1000, tags.join(","));
  return value;
}

/** Live keys that start with `prefix`, sorted. */
export const keys = (prefix = "") => host.cacheKeys(String(prefix));

/** Drop a key, or every entry carrying a tag. Also evicts cached pages tagged with it. */
export function invalidate(keyOrTag) {
  host.cacheDelete(String(keyOrTag));
}

// forge:cache (server): a process-wide TTL cache shared by all workers, with tag invalidation.
import { host } from "./host-ref.js";

const inflight = new Map();
/** How long one caller may compute a missing key before others take over. */
const LEASE_MS = 30_000;

/**
 * Memoise `fn()` under `key` for `ttl` seconds. Concurrent callers share one computation, on
 * this worker (one promise) and across workers (a lease in the shared cache: one computes,
 * the others wait for its value, and take over if it fails or stalls).
 */
export async function cache(key, fn, { ttl = 60, tags = [] } = {}) {
  const hit = host.cacheGet(key);
  if (hit !== undefined) return JSON.parse(hit);
  let p = inflight.get(key);
  if (!p) {
    p = compute(key, fn, ttl, tags).finally(() => inflight.delete(key));
    inflight.set(key, p);
  }
  return p;
}

async function compute(key, fn, ttl, tags) {
  for (let wait = 5; ; wait = Math.min(wait * 2, 100)) {
    const hit = host.cacheGet(key);
    if (hit !== undefined) return JSON.parse(hit);
    if (host.cacheLease(key, LEASE_MS)) {
      try {
        const value = (await fn()) ?? null;
        host.cacheSet(key, JSON.stringify(value), ttl * 1000, tags.join(","));
        return value;
      } catch (e) {
        host.cacheRelease(key);
        throw e;
      }
    }
    await new Promise((r) => setTimeout(r, wait));
  }
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

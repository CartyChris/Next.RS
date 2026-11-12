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

/** Drop a key, or every entry carrying a tag. Also evicts cached pages tagged with it. */
export function invalidate(keyOrTag) {
  host.cacheDelete(String(keyOrTag));
}

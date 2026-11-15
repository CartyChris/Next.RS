// forge:cache (browser): same API, in-memory, per tab.
const entries = new Map();

export async function cache(key, fn, { ttl = 60, tags = [] } = {}) {
  const hit = entries.get(key);
  if (hit && hit.until > Date.now()) return hit.value;
  const value = Promise.resolve().then(fn);
  entries.set(key, { value, until: Date.now() + ttl * 1000, tags });
  value.catch(() => entries.delete(key));
  return value;
}

export function invalidate(keyOrTag) {
  for (const [key, entry] of entries) if (key === keyOrTag || entry.tags.includes(keyOrTag)) entries.delete(key);
}

export function get(key) {
  const hit = entries.get(key);
  return hit && hit.until > Date.now() ? hit.value : undefined;
}

export function set(key, value, { ttl = 86400, tags = [] } = {}) {
  entries.set(key, { value, until: Date.now() + ttl * 1000, tags });
  return value;
}

export const keys = (prefix = "") => [...entries.keys()].filter((k) => k.startsWith(prefix) && get(k) !== undefined).sort();

// forge:test: a small test API for `forge test`. Tests run in the same embedded engine
// as server code, so loaders, API handlers and plain modules can be tested without Node.
const suites = [];
const registered = [];
const hooks = { beforeEach: [], afterEach: [] };

/** Group tests under a name. */
export function describe(name, body) {
  suites.push(name);
  const outer = [hooks.beforeEach.length, hooks.afterEach.length];
  try {
    body();
  } finally {
    suites.pop();
    // Hooks registered inside a group apply to that group only.
    [hooks.beforeEach.length, hooks.afterEach.length] = outer;
  }
}

/** Register a test. The function may be async; it fails by throwing. */
export function test(name, fn) {
  registered.push({ name: [...suites, name].join(" › "), fn, before: [...hooks.beforeEach], after: [...hooks.afterEach] });
}
export const it = test;
test.skip = (name) => registered.push({ name: [...suites, name].join(" › "), skip: true });

export const beforeEach = (fn) => void hooks.beforeEach.push(fn);
export const afterEach = (fn) => void hooks.afterEach.push(fn);

const show = (value) => {
  if (typeof value === "string") return JSON.stringify(value);
  if (typeof value === "function") return `[Function ${value.name || "anonymous"}]`;
  if (value instanceof Error) return `${value.name}: ${value.message}`;
  try {
    const json = JSON.stringify(value, (_, v) => (typeof v === "bigint" ? `${v}n` : v === undefined ? "__undefined__" : v));
    return json === undefined ? String(value) : json.replaceAll('"__undefined__"', "undefined");
  } catch {
    return String(value);
  }
};

const kind = (value) => Object.prototype.toString.call(value);
const bytes = (view) => Array.from(new Uint8Array(view.buffer ?? view, view.byteOffset ?? 0, view.byteLength));

function equal(a, b) {
  if (Object.is(a, b)) return true;
  if (typeof a !== "object" || typeof b !== "object" || a === null || b === null) return false;
  // Values kept in internal slots have no keys to compare, so a Date would equal `{}`.
  // Different built-in kinds are never equal; the same kind is compared by its content.
  if (kind(a) !== kind(b)) return false;
  if (a instanceof Date) return Object.is(a.getTime(), b.getTime());
  if (a instanceof RegExp) return a.source === b.source && a.flags === b.flags;
  if (a instanceof Error) return a.name === b.name && a.message === b.message;
  if (a instanceof Number || a instanceof String || a instanceof Boolean) return Object.is(a.valueOf(), b.valueOf());
  if (a instanceof ArrayBuffer || ArrayBuffer.isView(a)) return a.byteLength === b.byteLength && equal(bytes(a), bytes(b));
  if (a instanceof Set) return a.size === b.size && [...a].every((v) => b.has(v) || [...b].some((w) => equal(v, w)));
  if (a instanceof Map) {
    const has = ([k, v]) => (b.has(k) ? equal(v, b.get(k)) : [...b].some(([k2, v2]) => equal(k, k2) && equal(v, v2)));
    return a.size === b.size && [...a].every(has);
  }
  if (typeof URL === "function" && a instanceof URL) return b instanceof URL && a.href === b.href;
  if (Array.isArray(a) !== Array.isArray(b)) return false;
  const keys = Object.keys(a).filter((k) => a[k] !== undefined);
  const other = Object.keys(b).filter((k) => b[k] !== undefined);
  return keys.length === other.length && keys.every((k) => equal(a[k], b[k]));
}

class AssertionError extends Error {
  constructor(message) {
    super(message);
    this.name = "AssertionError";
  }
}

/** Assertions: `expect(value).toBe(…)`, `.toEqual`, `.toContain`, `.toThrow`, `.not`, and more. */
export function expect(actual) {
  const make = (negate) => {
    const check = (pass, verb, expected) => {
      if (pass === negate) {
        const tail = expected === undefined ? "" : ` ${show(expected)}`;
        throw new AssertionError(`expected ${show(actual)} ${negate ? "not " : ""}${verb}${tail}`);
      }
    };
    return {
      toBe: (expected) => check(Object.is(actual, expected), "to be", expected),
      toEqual: (expected) => check(equal(actual, expected), "to equal", expected),
      toBeTruthy: () => check(Boolean(actual), "to be truthy"),
      toBeFalsy: () => check(!actual, "to be falsy"),
      toBeNull: () => check(actual === null, "to be null"),
      toBeUndefined: () => check(actual === undefined, "to be undefined"),
      toBeDefined: () => check(actual !== undefined, "to be defined"),
      toBeGreaterThan: (n) => check(actual > n, "to be greater than", n),
      toBeGreaterThanOrEqual: (n) => check(actual >= n, "to be at least", n),
      toBeLessThan: (n) => check(actual < n, "to be less than", n),
      toBeLessThanOrEqual: (n) => check(actual <= n, "to be at most", n),
      toBeCloseTo: (n, digits = 2) => check(Math.abs(actual - n) < 10 ** -digits / 2, "to be close to", n),
      toContain: (item) => check(typeof actual === "string" ? actual.includes(item) : [...actual].some((v) => equal(v, item)), "to contain", item),
      toHaveLength: (n) => check(actual?.length === n, "to have length", n),
      toMatch: (pattern) => check(pattern instanceof RegExp ? pattern.test(actual) : String(actual).includes(pattern), "to match", String(pattern)),
      toHaveProperty: (key, value) => check(actual != null && key in actual && (value === undefined || equal(actual[key], value)), "to have property", key),
      toBeInstanceOf: (type) => check(actual instanceof type, "to be an instance of", type.name),
      toThrow(expected) {
        let thrown;
        try {
          actual();
        } catch (e) {
          thrown = e ?? new Error("thrown");
        }
        const matches = thrown && (expected === undefined || (expected instanceof RegExp ? expected.test(thrown.message) : String(thrown.message).includes(expected)));
        check(Boolean(matches), "to throw", expected === undefined ? undefined : String(expected));
      },
      async rejects(expected) {
        let thrown;
        try {
          await actual;
        } catch (e) {
          thrown = e ?? new Error("rejected");
        }
        const matches = thrown && (expected === undefined || String(thrown.message).includes(expected));
        check(Boolean(matches), "to reject", expected);
      },
    };
  };
  return { ...make(false), not: make(true) };
}

/** Run every registered test (used by `forge test`). */
export async function __run(file) {
  const results = [];
  for (const t of registered.splice(0)) {
    if (t.skip) {
      results.push({ file, name: t.name, skipped: true });
      continue;
    }
    const started = performance.now();
    let failed = false;
    let failure;
    const fail = (e) => {
      if (!failed) failure = e;
      failed = true;
    };
    try {
      for (const hook of t.before) await hook();
      await t.fn();
    } catch (e) {
      fail(e);
    }
    // Teardown always runs, and one failing hook does not stop the others, so a failed
    // test cannot leave state behind for the next one.
    for (const hook of t.after) {
      try {
        await hook();
      } catch (e) {
        fail(e);
      }
    }
    if (failed) {
      const e = failure;
      const stack = e instanceof AssertionError ? "" : String(e?.stack ?? "").split("\n").slice(0, 4).join("\n");
      results.push({ file, name: t.name, ok: false, ms: performance.now() - started, error: `${e?.name ?? "Error"}: ${e?.message ?? e}`, stack });
    } else {
      results.push({ file, name: t.name, ok: true, ms: performance.now() - started });
    }
  }
  hooks.beforeEach.length = hooks.afterEach.length = 0;
  return results;
}

// The one place the server bundle obtains the raw host ops.
export const host = globalThis.__forge_take_host?.() ?? null;

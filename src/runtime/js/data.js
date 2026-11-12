// forge:data: read the current route's loader data.
import { rt, useRoute } from "./shared.js";

/** React hook: the value returned by this route's `loader.ts`. */
export const useData = () => useRoute().data;

/** Non-hook access for vanilla pages (client only). */
export const getData = () => rt.state?.data ?? null;

/** Warm the route chunk and loader data for a path. */
export const preloadData = (to) => rt.prefetch?.(to);

// forge:navigation: links, programmatic navigation, and loader control flow.
import { rt, useRoute } from "./shared.js";

/** Throw from a loader (or call during render on the server) to answer 404. */
export function notFound(message = "Not found") {
  throw Object.assign(new Error(message), { __forge: "not-found", status: 404 });
}

/** Throw from a loader to redirect. */
export function redirect(location, status = 302) {
  throw Object.assign(new Error(`Redirect to ${location}`), { __forge: "redirect", location: String(location), status });
}

export function navigate(to, options) {
  return rt.navigate ? rt.navigate(to, options) : Promise.resolve();
}

export function prefetch(to) {
  rt.prefetch?.(to);
}

/** The href to put on an anchor for an app path (hash-prefixed in single-file builds). */
export function href(to) {
  return rt.href(to);
}

/** `<Link to="/about">`: an anchor with hover/focus prefetch. Plain `<a href>` also navigates client-side. */
export function Link(props) {
  if (!rt.Link) throw new Error("<Link> needs a React page");
  return rt.Link(props);
}

export function useRouter() {
  const s = useRoute();
  return { pathname: s.pathname, search: s.search, params: s.params, navigate, prefetch };
}
export const useParams = () => useRoute().params;
export const usePathname = () => useRoute().pathname;
export const useSearchParams = () => new URLSearchParams(useRoute().search);

/** Non-hook access for vanilla pages (client only). */
export const currentRoute = () => rt.state;

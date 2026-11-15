// React adapter pieces shared by server rendering and client hydration.
import * as React from "react";
import { rt } from "./shared.js";

const h = React.createElement;
const RouteContext = React.createContext(null);

rt.React = React;
rt.useRoute = () => React.useContext(RouteContext);
rt.Link = function Link({ to, href, prefetch = true, onMouseEnter, onFocus, onTouchStart, ...rest }) {
  const target = to ?? href;
  const warm = (user) => (e) => {
    user?.(e);
    if (prefetch) rt.prefetch?.(target);
  };
  return h("a", { ...rest, href: rt.href(target), onMouseEnter: warm(onMouseEnter), onFocus: warm(onFocus), onTouchStart: warm(onTouchStart) });
};

class Boundary extends React.Component {
  state = { error: null };
  static getDerivedStateFromError(error) {
    return { error };
  }
  componentDidUpdate(prev) {
    if (this.state.error && prev.resetKey !== this.props.resetKey) this.setState({ error: null });
  }
  render() {
    if (!this.state.error) return this.props.children;
    return h(this.props.fallback, { error: this.state.error, reset: () => this.setState({ error: null }) });
  }
}

// A vanilla page (`mount`/`render` exports) rendered inside React layouts.
const hosts = new WeakMap();
function vanillaHost(page) {
  let Host = hosts.get(page);
  if (Host) return Host;
  Host = function VanillaPage() {
    const ref = React.useRef(null);
    const s = rt.useRoute();
    const ctx = { params: s.params, data: s.data, pathname: s.pathname, search: s.search, navigate: rt.navigate };
    const html = React.useMemo(() => {
      const out = typeof page.render === "function" ? page.render(ctx) : "";
      return typeof out === "string" ? out : "";
    }, [s.pathname, s.search]);
    React.useEffect(() => {
      const cleanup = page.mount?.(ref.current, { ...ctx, hydrate: false });
      return () => typeof cleanup === "function" && cleanup();
    }, [s.pathname, s.search]);
    return h("div", { ref, "data-forge-page": "", dangerouslySetInnerHTML: { __html: html }, suppressHydrationWarning: true });
  };
  hosts.set(page, Host);
  return Host;
}

// layouts (outermost first) > ErrorBoundary > Suspense(Loading) > page.
// Layout components keep their identity across navigations, so their state persists.
export function App({ store }) {
  const s = React.useSyncExternalStore(store.subscribe, store.get, store.get);
  React.useEffect(() => void (rt.hydrating = false), []);
  const { page, layouts } = s.mod;
  const props = { params: s.params, data: s.data };
  let el = h(page.default ?? vanillaHost(page), props);
  if (page.Loading) el = h(React.Suspense, { fallback: h(page.Loading, props) }, el);
  if (page.ErrorBoundary) el = h(Boundary, { fallback: page.ErrorBoundary, resetKey: s.pathname }, el);
  for (let i = layouts.length - 1; i >= 0; i--) el = h(layouts[i].default, props, el);
  return h(RouteContext.Provider, { value: s }, el);
}

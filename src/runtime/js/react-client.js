import { createElement } from "react";
import { createRoot, hydrateRoot } from "react-dom/client";
import { App } from "./react.js";
import { rt } from "./shared.js";

export function mount(el, store, hydrate) {
  const app = createElement(App, { store });
  rt.hydrating = hydrate;
  let root;
  if (hydrate) {
    root = hydrateRoot(el, app, { onRecoverableError: (e) => console.warn("[forge] hydration:", e) });
  } else {
    el.textContent = "";
    root = createRoot(el);
    root.render(app);
  }
  return { unmount: () => root.unmount() };
}

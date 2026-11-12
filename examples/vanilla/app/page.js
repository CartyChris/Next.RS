// A page without a framework: `render` produces HTML (server-rendered and prerendered),
// `mount` makes it interactive in the browser and returns a cleanup function.
import "./styles.css";
import { nav } from "./_lib/notes.js";
import { store } from "forge:store";

export const meta = { title: "Forge Vanilla", description: "Zero-dependency Forge page" };

export function render() {
  return `<main>${nav}<h1>Hello Forge</h1><p>Clicks: <output id="count">0</output></p><button id="inc">Click me</button></main>`;
}

export function mount(el) {
  const out = el.querySelector("#count");
  let n = store.get("clicks", 0);
  out.textContent = n;
  const onClick = () => store.set("clicks", (out.textContent = ++n));
  el.querySelector("#inc").addEventListener("click", onClick);
  return () => el.querySelector("#inc")?.removeEventListener("click", onClick);
}

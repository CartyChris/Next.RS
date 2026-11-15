import { escape, nav, notes } from "../../_lib/notes.js";

export const meta = ({ data }) => ({ title: `${data.title} · Notes` });

export function render({ data }) {
  const links = notes.map((n) => `<li><a href="/notes/${n.id}">${escape(n.title)}</a></li>`).join("");
  return `<main>${nav}<h1>${escape(data.title)}</h1><p>${escape(data.body)}</p><ul>${links}</ul></main>`;
}

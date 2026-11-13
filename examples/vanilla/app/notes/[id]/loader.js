import { notFound } from "forge:navigation";
import { notes } from "../../_lib/notes.js";

// Runs on the server (or at build time for static / single-html targets).
export function loader({ params }) {
  const note = notes.find((n) => n.id === params.id);
  if (!note) notFound();
  return note;
}

// Which pages to prerender for static and single-html builds.
export const staticParams = () => notes.map((n) => ({ id: n.id }));

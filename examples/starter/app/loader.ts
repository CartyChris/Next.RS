// Runs on the server for each request (and at build time for static and single-file output).
import { projects } from "./_data";

export function loader() {
  return { projects };
}

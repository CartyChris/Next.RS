import { createModelView as openThree } from "./three-view";
import { createModelView as openRust } from "./rust-view";
import type { ViewOptions } from "./model";

export const canSelectRenderer = true;
export { initialParameters } from "./model";
export type { ModelParameters, ModelStats, ModelView } from "./model";

export function createModelView(container: HTMLDivElement, options: ViewOptions) {
  const engine = options.engine ?? new URLSearchParams(location.search).get("renderer");
  return (engine === "rust" ? openRust : openThree)(container, options);
}

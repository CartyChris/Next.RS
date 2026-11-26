import { openEditor } from "./model";
import type { Graphics, ViewOptions } from "./model";
import { bindOrbit } from "./orbit-input";
import { createRustRenderer } from "./rust-renderer";

export const canSelectRenderer = false;
export { initialParameters } from "./model";
export type { ModelParameters, ModelStats, ModelView } from "./model";

async function openRust(makeCanvas: () => HTMLCanvasElement, signal: AbortSignal): Promise<Graphics> {
  const canvas = makeCanvas();
  const renderer = await createRustRenderer(canvas, signal);
  const stopInput = bindOrbit(canvas, renderer);
  return {
    canvas, backend: "rust-webgl2", fallbackReason: null,
    setMesh(mesh, material) { renderer.setMesh(mesh, material.color); },
    resize(width, height, dpr) {
      renderer.resize(Math.max(1, Math.round(width * dpr)), Math.max(1, Math.round(height * dpr)));
    },
    reset() { renderer.reset(); },
    orbit(dx, dy) { renderer.orbit(dx, dy); },
    zoom(factor) { renderer.zoom(factor); },
    draw(rotation, wireframe, normals) { renderer.draw(rotation, wireframe, normals); },
    dispose() { stopInput(); renderer.dispose(); },
  };
}

export function createModelView(container: HTMLDivElement, options: ViewOptions) {
  return openEditor(container, options, openRust);
}

import { adaptiveQuality, fit, loop } from "forge:game";
import type { FitDisposer } from "forge:game";
import { loadMeshGenerator } from "forge:mesh";
import type { MeshData, MeshKind } from "forge:mesh";
import wasmUrl from "forge:wasm/mesh";
import { encodeGlb } from "./glb";
import type { GlbOptions } from "./glb";

export interface ModelParameters {
  kind: MeshKind;
  detail: number;
  seed: number;
  scale: number;
  variation: number;
}

export const initialParameters: ModelParameters = { kind: "torus", detail: 48, seed: 17, scale: 1, variation: 0.5 };

export type ModelSource = { kind: "procedural"; parameters: ModelParameters }
  | { kind: "import"; mesh: MeshData; filename: string };

export interface ModelStats {
  vertices: number;
  triangles: number;
  generationMs: number | null;
  memoryBytes: number;
  backend: "webgpu" | "webgl" | "rust-webgl2";
  fallbackReason: string | null;
  bounds: MeshData["bounds"];
}

export interface ModelView {
  generate(parameters: ModelParameters): void;
  importMesh(mesh: MeshData, filename: string): void;
  setWireframe(value: boolean): void;
  setNormals(value: boolean): void;
  setPaused(value: boolean): void;
  reset(): void;
  zoom(factor: number): void;
  exportGlb(): Promise<void>;
  dispose(): void;
}

export interface ViewOptions {
  signal: AbortSignal;
  paused: boolean;
  normals?: boolean;
  wireframe?: boolean;
  source?: ModelSource;
  engine?: "rust" | "three";
  onStats: (stats: ModelStats) => void;
  onError: (error: Error) => void;
}

export interface Graphics {
  canvas: HTMLCanvasElement;
  backend: ModelStats["backend"];
  fallbackReason: string | null;
  setMesh(mesh: MeshData, material: GlbOptions, scale: number): void;
  resize(width: number, height: number, dpr: number): void;
  orbit(dx: number, dy: number): void;
  reset(): void;
  zoom(factor: number): void;
  draw(rotation: number, wireframe: boolean, normals: boolean): void;
  dispose(): void;
}

// GLB uses linear RGB. Keeping the conversion here gives both renderers and the
// exported material exactly the same base color for each procedural model.
function linear(channel: number) {
  const value = channel / 255;
  return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
}

function materialFor(name: string, parameters?: ModelParameters): GlbOptions {
  const kind = parameters?.kind;
  const rgb = kind === "terrain" ? 0x77a68f : kind === "rock" ? 0x7e9fa0 : 0xc9a874;
  const matte = kind === "rock" || kind === "terrain";
  const extras: GlbOptions["extras"] = parameters
    ? { generator: "Next.RS Rust mesh ABI v1", ...parameters, units: "metres", upAxis: "Y" }
    : { generator: "Next.RS mesh JSON", units: "metres", upAxis: "Y" };
  return {
    name, extras,
    color: [linear(rgb >> 16), linear((rgb >> 8) & 255), linear(rgb & 255)],
    roughness: matte ? 0.82 : 0.38,
    metalness: matte ? 0.05 : 0.25,
  };
}

// The canvas factory lets a fallback renderer request a fresh context surface.
// Browser startup stays in the mount call, including in server/static builds.
export async function openEditor(
  container: HTMLDivElement, options: ViewOptions,
  openGraphics: (makeCanvas: () => HTMLCanvasElement, signal: AbortSignal) => Promise<Graphics>,
): Promise<ModelView> {
  const { signal, onStats, onError } = options;
  const generator = await loadMeshGenerator(wasmUrl, { signal });
  let pendingCanvas: HTMLCanvasElement | undefined;
  const makeCanvas = () => {
    if (signal.aborted) throw new DOMException("Viewer setup was cancelled.", "AbortError");
    const canvas = document.createElement("canvas");
    canvas.id = "model-canvas";
    canvas.tabIndex = 0;
    canvas.setAttribute("aria-label", "3D model. Drag to orbit, pinch or scroll to zoom. Arrow keys rotate the view; plus and minus zoom; Home resets.");
    canvas.dataset.frames = "0";
    canvas.dataset.modelRevision = "0";
    container.replaceChildren(canvas);
    pendingCanvas = canvas;
    return canvas;
  };
  let graphics: Graphics;
  try {
    graphics = await openGraphics(makeCanvas, signal);
  } catch (error) {
    generator.dispose();
    pendingCanvas?.remove();
    throw error;
  }

  const { canvas, backend, fallbackReason } = graphics;
  canvas.dataset.renderer = backend;
  canvas.dataset.wasm = "rust";
  let disposed = false;
  let rotation = 0, revision = 0, frames = 0;
  let { paused, wireframe = false, normals = false } = options;
  let current: { mesh: MeshData; material: GlbOptions } | undefined;
  let stopFit: FitDisposer | undefined;
  let stopLoop: (() => void) | undefined;
  const quality = adaptiveQuality({
    minDpr: 0.75, maxDpr: 1.5, initialDpr: Math.max(0.75, Math.min(devicePixelRatio || 1, 1.5)),
    onChange: () => stopFit?.refresh(),
  });
  const downloads = new Map<string, ReturnType<typeof setTimeout>>();
  const reset = () => { rotation = 0; graphics.reset(); };
  const install = (mesh: MeshData, material: GlbOptions, scale: number, generationMs: number | null) => {
    if (disposed) throw new Error("Model view has been disposed.");
    graphics.setMesh(mesh, material, scale);
    current = { mesh, material };
    reset();
    canvas.dataset.modelRevision = String(++revision);
    canvas.dataset.meshValid = "true";
    onStats({
      vertices: mesh.vertexCount, triangles: mesh.triangleCount, memoryBytes: generator.memoryBytes,
      bounds: mesh.bounds, backend, fallbackReason, generationMs,
    });
  };
  const generate = (next: ModelParameters) => {
    if (disposed) throw new Error("Model view has been disposed.");
    const started = performance.now();
    const mesh = generator.generate(next);
    install(mesh, materialFor(`${next.kind}-${next.seed}`, next), next.scale, performance.now() - started);
  };
  const importMesh = (mesh: MeshData, filename: string) => {
    const name = filename.replace(/\.json$/i, "").slice(0, 96) || "imported";
    const gridScale = Math.max(...mesh.bounds.max.map((v, i) => v - mesh.bounds.min[i])) / 2;
    install(mesh, materialFor(name), gridScale, null);
  };

  const keyboard = (event: KeyboardEvent) => {
    switch (event.key) {
      case "Home": reset(); break;
      case "+": case "=": graphics.zoom(1 / 1.15); break;
      case "-": graphics.zoom(1.15); break;
      case "ArrowLeft": graphics.orbit(-0.1, 0); break;
      case "ArrowRight": graphics.orbit(0.1, 0); break;
      case "ArrowUp": graphics.orbit(0, -0.1); break;
      case "ArrowDown": graphics.orbit(0, 0.1); break;
      default: return;
    }
    event.preventDefault();
  };
  const contextLost = () => {
    dispose();
    onError(new Error("The WebGL context was lost."));
  };
  const dispose = () => {
    if (disposed) return;
    disposed = true;
    signal.removeEventListener("abort", dispose);
    canvas.removeEventListener("keydown", keyboard);
    canvas.removeEventListener("webglcontextlost", contextLost);
    stopLoop?.();
    stopFit?.();
    quality.dispose();
    graphics.dispose();
    generator.dispose();
    current = undefined;
    for (const [url, timer] of downloads) { clearTimeout(timer); URL.revokeObjectURL(url); }
    downloads.clear();
    canvas.remove();
  };
  const draw = () => {
    graphics.draw(rotation, wireframe, normals);
    canvas.dataset.frames = String(++frames);
  };
  try {
    if (signal.aborted) throw new DOMException("Viewer setup was cancelled.", "AbortError");
    signal.addEventListener("abort", dispose, { once: true });
    canvas.addEventListener("keydown", keyboard);
    if (backend === "rust-webgl2") canvas.addEventListener("webglcontextlost", contextLost);
    stopFit = fit(canvas, (width, height, dpr) => {
      graphics.resize(width, height, dpr);
      canvas.dataset.dpr = String(dpr);
    }, { maxDpr: () => quality.dpr });
    const source = options.source ?? { kind: "procedural", parameters: initialParameters };
    if (source.kind === "procedural") generate(source.parameters);
    else importMesh(source.mesh, source.filename);
    draw(); // Fail startup if the first frame cannot be submitted.
    let previousTime = performance.now();
    stopLoop = loop({
      update(dt) { if (!paused) rotation = (rotation + dt * 0.24) % (2 * Math.PI); },
      render() {
        try {
          const now = performance.now();
          quality.sample(now - previousTime);
          previousTime = now;
          draw();
        } catch (error) {
          dispose();
          onError(error instanceof Error ? error : new Error("The graphics renderer stopped."));
        }
      },
    });
  } catch (error) {
    dispose();
    throw error;
  }

  return {
    generate, importMesh,
    setWireframe(value) { wireframe = value; },
    setNormals(value) { normals = value; },
    setPaused(value) { paused = value; },
    reset,
    zoom(factor) { graphics.zoom(factor); },
    async exportGlb() {
      if (!current) throw new Error("Model view has been disposed.");
      const bytes = encodeGlb(current.mesh, current.material);
      const url = URL.createObjectURL(new Blob([bytes], { type: "model/gltf-binary" }));
      const anchor = document.createElement("a");
      anchor.href = url;
      anchor.download = `model-${current.material.name}.glb`;
      document.body.append(anchor);
      anchor.click();
      anchor.remove();
      downloads.set(url, setTimeout(() => { URL.revokeObjectURL(url); downloads.delete(url); }, 30_000));
    },
    dispose,
  };
}

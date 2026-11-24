import * as THREE from "three/webgpu";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";
import { adaptiveQuality, createRenderer, fit, loop } from "forge:game";
import { loadMeshGenerator } from "forge:mesh";
import type { MeshData, MeshKind } from "forge:mesh";
import wasmUrl from "forge:wasm/mesh";
import { encodeGlb } from "./glb";

export interface ModelParameters {
  kind: MeshKind;
  detail: number;
  seed: number;
  scale: number;
  variation: number;
}

export const initialParameters: ModelParameters = { kind: "torus", detail: 48, seed: 17, scale: 1, variation: 0.5 };

export interface ModelStats {
  vertices: number;
  triangles: number;
  generationMs: number;
  memoryBytes: number;
  backend: "webgpu" | "webgl";
  fallbackReason: string | null;
  bounds: MeshData["bounds"];
}

export interface ModelView {
  generate(parameters: ModelParameters): void;
  setWireframe(value: boolean): void;
  setNormals(value: boolean): void;
  setPaused(value: boolean): void;
  reset(): void;
  zoom(factor: number): void;
  exportGlb(): Promise<void>;
  dispose(): void;
}

interface ViewOptions {
  signal: AbortSignal;
  paused: boolean;
  onStats: (stats: ModelStats) => void;
  onError: (error: Error) => void;
}

function backendOf(renderer: THREE.WebGPURenderer): "webgpu" | "webgl" {
  // Three may perform its own fallback during init(). Inspect its backend identity
  // at this library boundary rather than reporting the requested renderer.
  return "isWebGPUBackend" in renderer.backend && renderer.backend.isWebGPUBackend === true ? "webgpu" : "webgl";
}

function geometryFrom(mesh: MeshData): THREE.BufferGeometry {
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.BufferAttribute(mesh.positions, 3));
  geometry.setAttribute("normal", new THREE.BufferAttribute(mesh.normals, 3));
  geometry.setAttribute("uv", new THREE.BufferAttribute(mesh.uvs, 2));
  geometry.setIndex(new THREE.BufferAttribute(mesh.indices, 1));
  geometry.computeBoundingBox();
  geometry.computeBoundingSphere();
  return geometry;
}

export async function createModelView(container: HTMLDivElement, options: ViewOptions): Promise<ModelView> {
  const { signal, onStats, onError } = options;
  let canvas = document.createElement("canvas");
  const prepareCanvas = () => {
    canvas.id = "model-canvas";
    canvas.tabIndex = 0;
    canvas.setAttribute("aria-label", "3D model. Drag to orbit, pinch or scroll to zoom. Arrow keys rotate the view; plus and minus zoom; Home resets.");
    canvas.dataset.frames = "0";
    canvas.dataset.modelRevision = "0";
    container.replaceChildren(canvas);
  };
  prepareCanvas();
  const generator = await loadMeshGenerator(wasmUrl, { signal });
  let selected;
  try {
    selected = await createRenderer({
      webgpu: () => new THREE.WebGPURenderer({ canvas, antialias: true }),
      webgl: () => {
        // A failed WebGPU init can leave a canvas bound to its context. Supply a
        // fresh surface for the WebGL attempt, including the no-WebGPU case.
        canvas = document.createElement("canvas");
        prepareCanvas();
        return new THREE.WebGPURenderer({ canvas, antialias: true, forceWebGL: true });
      },
      prefer: new URLSearchParams(location.search).get("renderer") === "webgl" ? "webgl" : "auto",
      backendOf,
      signal,
    });
  } catch (error) {
    generator.dispose();
    canvas.remove();
    throw error;
  }
  const { renderer, backend, fallbackReason } = selected;
  canvas.dataset.renderer = backend;
  canvas.dataset.wasm = "rust";
  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  renderer.toneMappingExposure = 1.1;

  const scene = new THREE.Scene();
  scene.background = new THREE.Color(0x101a20);
  const camera = new THREE.PerspectiveCamera(40, 1, 0.01, 2000);
  const controls = new OrbitControls(camera, canvas);
  controls.enableDamping = true;
  controls.dampingFactor = 0.08;
  controls.enablePan = false;
  controls.minPolarAngle = 0.05;
  controls.maxPolarAngle = Math.PI * 0.93;

  scene.add(new THREE.HemisphereLight(0xe6f5ff, 0x375457, 2.6));
  const key = new THREE.DirectionalLight(0xffedcf, 4);
  key.position.set(4, 6, 5);
  scene.add(key);
  const rim = new THREE.DirectionalLight(0x6bcabf, 3);
  rim.position.set(-4, 2, -3);
  scene.add(rim);
  const grid = new THREE.GridHelper(8, 32, 0x48646a, 0x273b43);
  scene.add(grid);
  const material = new THREE.MeshStandardMaterial({ color: 0xc9a874, roughness: 0.38, metalness: 0.25 });
  const normalMaterial = new THREE.MeshNormalMaterial();
  const model = new THREE.Mesh<THREE.BufferGeometry, THREE.Material>(new THREE.BufferGeometry(), material);
  scene.add(model);

  let disposed = false;
  let paused = options.paused;
  let revision = 0;
  let frames = 0;
  let parameters = initialParameters;
  let currentMesh: MeshData | null = null;
  let bounds: MeshData["bounds"] = { min: [-1, -1, -1], max: [1, 1, 1] };
  let stopLoop = () => {};
  let stopFit: (() => void) & { refresh?: () => void } = () => {};
  const downloads = new Map<string, ReturnType<typeof setTimeout>>();

  const reset = () => {
    const extent = Math.max(...bounds.max.map((value, axis) => value - bounds.min[axis]));
    const center = new THREE.Vector3(...bounds.min).add(new THREE.Vector3(...bounds.max)).multiplyScalar(0.5);
    controls.target.copy(center);
    camera.position.copy(center).add(new THREE.Vector3(1.35, 0.95, 1.6).multiplyScalar(extent));
    camera.near = Math.max(extent / 1000, 0.0001);
    camera.far = Math.max(extent * 100, 10);
    camera.updateProjectionMatrix();
    controls.minDistance = extent * 0.65;
    controls.maxDistance = extent * 8;
    model.rotation.set(0, 0, 0);
    controls.update();
  };

  const generate = (next: ModelParameters) => {
    if (disposed) throw new Error("Model view has been disposed.");
    const started = performance.now();
    const mesh = generator.generate(next);
    const generationMs = performance.now() - started;
    const geometry = geometryFrom(mesh);
    const old = model.geometry;
    model.geometry = geometry;
    old.dispose();
    parameters = { ...next };
    currentMesh = mesh;
    bounds = mesh.bounds;
    model.name = `${next.kind}-${next.seed}`;
    model.userData = { generator: "Next.RS Rust mesh ABI v1", ...parameters, units: "metres", upAxis: "Y" };
    material.color.setHex({ sphere: 0xc9a874, torus: 0xc9a874, rock: 0x7e9fa0, terrain: 0x77a68f }[next.kind]);
    material.roughness = next.kind === "rock" || next.kind === "terrain" ? 0.82 : 0.38;
    material.metalness = next.kind === "rock" || next.kind === "terrain" ? 0.05 : 0.25;
    grid.scale.setScalar(next.scale);
    grid.position.y = mesh.bounds.min[1] - next.scale * 0.04;
    canvas.dataset.modelRevision = String(++revision);
    canvas.dataset.meshValid = "true";
    reset();
    onStats({ vertices: mesh.vertexCount, triangles: mesh.triangleCount, generationMs, memoryBytes: generator.memoryBytes, backend, fallbackReason, bounds });
  };

  const zoom = (factor: number) => {
    const offset = camera.position.clone().sub(controls.target);
    offset.setLength(THREE.MathUtils.clamp(offset.length() * factor, controls.minDistance, controls.maxDistance));
    camera.position.copy(controls.target).add(offset);
    controls.update();
  };
  const keyboard = (event: KeyboardEvent) => {
    if (["+", "=", "-", "Home"].includes(event.key)) {
      event.preventDefault();
      if (event.key === "Home") reset();
      else zoom(event.key === "-" ? 1.15 : 1 / 1.15);
    } else if (["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(event.key)) {
      event.preventDefault();
      const offset = new THREE.Spherical().setFromVector3(camera.position.clone().sub(controls.target));
      if (event.key === "ArrowLeft") offset.theta -= 0.1;
      if (event.key === "ArrowRight") offset.theta += 0.1;
      if (event.key === "ArrowUp") offset.phi -= 0.1;
      if (event.key === "ArrowDown") offset.phi += 0.1;
      offset.phi = THREE.MathUtils.clamp(offset.phi, controls.minPolarAngle, controls.maxPolarAngle);
      camera.position.copy(controls.target).add(new THREE.Vector3().setFromSpherical(offset));
      controls.update();
    }
  };
  canvas.addEventListener("keydown", keyboard);

  const quality = adaptiveQuality({
    minDpr: 0.75,
    maxDpr: 1.5,
    initialDpr: Math.max(0.75, Math.min(devicePixelRatio || 1, 1.5)),
    onChange: () => stopFit.refresh?.(),
  });

  const dispose = () => {
    if (disposed) return;
    disposed = true;
    stopLoop();
    stopFit();
    quality.dispose();
    controls.dispose();
    canvas.removeEventListener("keydown", keyboard);
    signal.removeEventListener("abort", dispose);
    model.geometry.dispose();
    material.dispose();
    normalMaterial.dispose();
    grid.geometry.dispose();
    const gridMaterials = Array.isArray(grid.material) ? grid.material : [grid.material];
    gridMaterials.forEach((entry) => entry.dispose());
    scene.clear();
    renderer.dispose();
    generator.dispose();
    currentMesh = null;
    for (const [url, timer] of downloads) { clearTimeout(timer); URL.revokeObjectURL(url); }
    downloads.clear();
    canvas.remove();
  };
  signal.addEventListener("abort", dispose, { once: true });

  try {
    stopFit = fit(canvas, (width, height, dpr) => {
      renderer.setPixelRatio(dpr);
      renderer.setSize(width, height, false);
      canvas.dataset.dpr = String(dpr);
      camera.aspect = width / height;
      camera.updateProjectionMatrix();
    }, { maxDpr: () => quality.dpr });
    generate(initialParameters);
    let previousFrame = performance.now();
    const render = () => {
      if (disposed) return;
      try {
        const now = performance.now();
        quality.sample(now - previousFrame);
        previousFrame = now;
        controls.update();
        renderer.render(scene, camera);
        canvas.dataset.frames = String(++frames);
      } catch (error) {
        dispose();
        onError(error instanceof Error ? error : new Error("The graphics renderer stopped."));
      }
    };
    // Verify a rendered frame before reporting startup success.
    renderer.render(scene, camera);
    canvas.dataset.frames = String(++frames);
    stopLoop = loop({ update: (dt) => { if (!paused) model.rotation.y += dt * 0.24; }, render });
  } catch (error) {
    dispose();
    throw error;
  }

  return {
    generate,
    setWireframe(value) { material.wireframe = value; normalMaterial.wireframe = value; },
    setNormals(value) { model.material = value ? normalMaterial : material; },
    setPaused(value) { paused = value; },
    reset,
    zoom,
    async exportGlb() {
      if (disposed || !currentMesh) throw new Error("Model view has been disposed.");
      // Export canonical Y-up geometry with a regular PBR material, independent
      // of preview rotation and diagnostic modes. Encoding completes in this
      // event turn: navigation cannot interrupt an asynchronous FileReader.
      const bytes = encodeGlb(currentMesh, {
        name: model.name,
        extras: model.userData,
        color: [material.color.r, material.color.g, material.color.b],
        roughness: material.roughness,
        metalness: material.metalness,
      });
      const filename = `model-${parameters.kind}-${parameters.seed}.glb`;
      const url = URL.createObjectURL(new Blob([bytes], { type: "model/gltf-binary" }));
      const anchor = document.createElement("a");
      anchor.href = url;
      anchor.download = filename;
      document.body.append(anchor);
      anchor.click();
      anchor.remove();
      downloads.set(url, setTimeout(() => { URL.revokeObjectURL(url); downloads.delete(url); }, 30_000));
    },
    dispose,
  };
}

import * as THREE from "three/webgpu";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";
import { createRenderer } from "forge:game";
import type { MeshData } from "forge:mesh";
import { openEditor } from "./model";
import type { Graphics, ViewOptions } from "./model";

export const canSelectRenderer = false;

function backendOf(renderer: THREE.WebGPURenderer): "webgpu" | "webgl" {
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

async function openThree(makeCanvas: () => HTMLCanvasElement, signal: AbortSignal): Promise<Graphics> {
  const { renderer, backend, fallbackReason } = await createRenderer({
    webgpu: () => new THREE.WebGPURenderer({ canvas: makeCanvas(), antialias: true }),
    webgl: () => new THREE.WebGPURenderer({ canvas: makeCanvas(), antialias: true, forceWebGL: true }),
    prefer: new URLSearchParams(location.search).get("renderer") === "webgl" ? "webgl" : "auto",
    backendOf, signal,
  });
  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  renderer.toneMappingExposure = 1.1;
  const canvas = renderer.domElement;
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
  const material = new THREE.MeshStandardMaterial();
  const normalMaterial = new THREE.MeshNormalMaterial();
  const model = new THREE.Mesh<THREE.BufferGeometry, THREE.Material>(new THREE.BufferGeometry(), material);
  const pivot = new THREE.Group();
  pivot.add(model);
  scene.add(pivot);
  let bounds: MeshData["bounds"] = { min: [-1, -1, -1], max: [1, 1, 1] };
  return {
    canvas, backend, fallbackReason,
    setMesh(mesh, options, gridScale) {
      const geometry = geometryFrom(mesh);
      model.geometry.dispose();
      model.geometry = geometry;
      bounds = mesh.bounds;
      // Rotate around the mesh center even when a producer uses world-space
      // positions. The original coordinates remain intact for GLB export.
      pivot.position.set(...bounds.min).add(new THREE.Vector3(...bounds.max)).multiplyScalar(0.5);
      model.position.copy(pivot.position).multiplyScalar(-1);
      material.color.setRGB(...options.color);
      material.roughness = options.roughness;
      material.metalness = options.metalness;
      grid.scale.setScalar(gridScale);
      grid.position.set(pivot.position.x, bounds.min[1] - gridScale * 0.04, pivot.position.z);
    },
    resize(width, height, dpr) {
      renderer.setPixelRatio(dpr);
      renderer.setSize(width, height, false);
      camera.aspect = width / height;
      camera.updateProjectionMatrix();
    },
    reset() {
      const extent = Math.max(...bounds.max.map((value, axis) => value - bounds.min[axis]));
      const center = new THREE.Vector3(...bounds.min).add(new THREE.Vector3(...bounds.max)).multiplyScalar(0.5);
      controls.target.copy(center);
      camera.position.copy(center).add(new THREE.Vector3(1.35, 0.95, 1.6).multiplyScalar(extent));
      camera.near = Math.max(extent / 1000, 0.0001);
      camera.far = Math.max(extent * 100, 10);
      camera.updateProjectionMatrix();
      controls.minDistance = extent * 0.65;
      controls.maxDistance = extent * 8;
      controls.update();
    },
    zoom(factor) {
      const offset = camera.position.clone().sub(controls.target);
      offset.setLength(THREE.MathUtils.clamp(offset.length() * factor, controls.minDistance, controls.maxDistance));
      camera.position.copy(controls.target).add(offset);
      controls.update();
    },
    orbit(dx, dy) {
      const offset = new THREE.Spherical().setFromVector3(camera.position.clone().sub(controls.target));
      offset.theta += dx;
      offset.phi = THREE.MathUtils.clamp(offset.phi + dy, controls.minPolarAngle, controls.maxPolarAngle);
      camera.position.copy(controls.target).add(new THREE.Vector3().setFromSpherical(offset));
      controls.update();
    },
    draw(rotation, wireframe, normals) {
      pivot.rotation.y = rotation;
      material.wireframe = normalMaterial.wireframe = wireframe;
      model.material = normals ? normalMaterial : material;
      controls.update();
      renderer.render(scene, camera);
    },
    dispose() {
      controls.dispose();
      model.geometry.dispose();
      material.dispose();
      normalMaterial.dispose();
      grid.geometry.dispose();
      const materials = Array.isArray(grid.material) ? grid.material : [grid.material];
      materials.forEach((entry) => entry.dispose());
      scene.clear();
      renderer.dispose();
    },
  };
}

export function createModelView(container: HTMLDivElement, options: ViewOptions) {
  return openEditor(container, options, openThree);
}

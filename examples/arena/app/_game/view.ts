// three.js renderer for the arena. Client-only (imported from a client-only page).

import * as THREE from "three";
import { EffectComposer } from "three/addons/postprocessing/EffectComposer.js";
import { OutputPass } from "three/addons/postprocessing/OutputPass.js";
import { RenderPass } from "three/addons/postprocessing/RenderPass.js";
import { UnrealBloomPass } from "three/addons/postprocessing/UnrealBloomPass.js";
import { fit } from "forge:game";
import floorFrag from "./floor.frag";
import floorVert from "./floor.vert";
import { ARENA, ORB_R, PLAYER_R } from "./sim";

export type Drawn = {
  id: number;
  x: number;
  z: number;
  vx: number;
  vz: number;
  score: number;
  energy: number;
  stunned: boolean;
  name: string;
  color: string;
  bot: boolean;
  you: boolean;
};

type Avatar = { group: THREE.Group; body: THREE.Mesh; mat: THREE.MeshStandardMaterial; ring: THREE.Mesh; label: THREE.Sprite; key: string; score: number };

function label(text: string, color: string): THREE.Sprite {
  const c = document.createElement("canvas");
  c.width = 256;
  c.height = 64;
  const g = c.getContext("2d")!;
  g.font = "600 30px system-ui, sans-serif";
  g.textAlign = "center";
  g.textBaseline = "middle";
  g.shadowColor = color;
  g.shadowBlur = 12;
  g.fillStyle = "#f8f9fa";
  g.fillText(text, 128, 32);
  const tex = new THREE.CanvasTexture(c);
  tex.colorSpace = THREE.SRGBColorSpace;
  const sprite = new THREE.Sprite(new THREE.SpriteMaterial({ map: tex, depthWrite: false, transparent: true }));
  sprite.scale.set(3.2, 0.8, 1);
  sprite.position.y = 1.7;
  return sprite;
}

export function createView(canvas: HTMLCanvasElement) {
  const renderer = new THREE.WebGLRenderer({ canvas, antialias: true, powerPreference: "high-performance" });
  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  const scene = new THREE.Scene();
  scene.background = new THREE.Color(0x04050b);
  scene.fog = new THREE.Fog(0x04050b, 28, 70);
  const camera = new THREE.PerspectiveCamera(52, 1, 0.1, 200);
  camera.position.set(0, 19, 17);
  camera.lookAt(0, 0, 0);

  const composer = new EffectComposer(renderer);
  composer.addPass(new RenderPass(scene, camera));
  const bloom = new UnrealBloomPass(new THREE.Vector2(1, 1), 0.55, 0.4, 0.55);
  composer.addPass(bloom);
  composer.addPass(new OutputPass());

  scene.add(new THREE.HemisphereLight(0x8fb4ff, 0x10081a, 0.9));
  const sun = new THREE.DirectionalLight(0xffffff, 1.4);
  sun.position.set(6, 14, 8);
  scene.add(sun);

  const floorUniforms = { uTime: { value: 0 }, uArena: { value: ARENA } };
  const floor = new THREE.Mesh(
    new THREE.PlaneGeometry(ARENA * 2 + 14, ARENA * 2 + 14),
    new THREE.ShaderMaterial({ uniforms: floorUniforms, vertexShader: floorVert, fragmentShader: floorFrag }),
  );
  floor.rotation.x = -Math.PI / 2;
  scene.add(floor);

  const wallMat = new THREE.MeshBasicMaterial({ color: 0x3bc9db });
  for (const [x, z, w, d] of [
    [0, -ARENA, ARENA * 2, 0.12],
    [0, ARENA, ARENA * 2, 0.12],
    [-ARENA, 0, 0.12, ARENA * 2],
    [ARENA, 0, 0.12, ARENA * 2],
  ]) {
    const wall = new THREE.Mesh(new THREE.BoxGeometry(w, 0.35, d), wallMat);
    wall.position.set(x, 0.18, z);
    scene.add(wall);
  }

  const MAX_ORBS = 32;
  const orbs = new THREE.InstancedMesh(
    new THREE.IcosahedronGeometry(ORB_R, 1),
    new THREE.MeshStandardMaterial({ color: 0xffe066, emissive: 0xffb300, emissiveIntensity: 1.6, roughness: 0.3 }),
    MAX_ORBS,
  );
  orbs.count = 0;
  scene.add(orbs);

  const avatars = new Map<number, Avatar>();
  const bodyGeo = new THREE.SphereGeometry(PLAYER_R, 32, 20);
  const ringGeo = new THREE.RingGeometry(PLAYER_R * 1.25, PLAYER_R * 1.45, 40);
  const bursts: { mesh: THREE.Mesh; t: number }[] = [];
  const burstGeo = new THREE.RingGeometry(0.3, 0.45, 32);

  function avatar(p: Drawn): Avatar {
    const key = `${p.name}|${p.color}|${p.you}`;
    const existing = avatars.get(p.id);
    if (existing && existing.key === key) return existing;
    if (existing) scene.remove(existing.group);
    const mat = new THREE.MeshStandardMaterial({ color: p.color, emissive: p.color, emissiveIntensity: 0.35, roughness: 0.35, metalness: 0.1 });
    const body = new THREE.Mesh(bodyGeo, mat);
    body.position.y = PLAYER_R;
    const ring = new THREE.Mesh(ringGeo, new THREE.MeshBasicMaterial({ color: 0xffffff, transparent: true, opacity: 0.85, side: THREE.DoubleSide }));
    ring.rotation.x = -Math.PI / 2;
    ring.position.y = 0.03;
    ring.visible = p.you;
    const group = new THREE.Group();
    const tag = label(p.bot ? `🤖 ${p.name}` : p.name, p.color);
    group.add(body, ring, tag);
    scene.add(group);
    const a = { group, body, mat, ring, label: tag, key, score: p.score };
    avatars.set(p.id, a);
    return a;
  }

  const stopFit = fit(
    canvas,
    (w, h, dpr) => {
      renderer.setPixelRatio(dpr);
      renderer.setSize(w, h, false);
      composer.setPixelRatio(dpr);
      composer.setSize(w, h);
      // Bloom is soft anyway: half resolution costs a quarter of the fill rate.
      bloom.resolution.set(w / 2, h / 2);
      camera.aspect = w / h;
      camera.updateProjectionMatrix();
    },
    { maxDpr: 1.5 },
  );

  const focus = new THREE.Vector3();
  const m = new THREE.Matrix4();
  const q = new THREE.Quaternion();
  const s = new THREE.Vector3();
  const pos = new THREE.Vector3();

  function update(players: Drawn[], orbList: [number, number][], time: number, dt: number) {
    floorUniforms.uTime.value = time;
    const seen = new Set<number>();
    for (const p of players) {
      seen.add(p.id);
      const a = avatar(p);
      a.group.position.set(p.x, 0, p.z);
      const speed = Math.hypot(p.vx, p.vz);
      a.body.scale.set(1 + speed * 0.015, 1 - speed * 0.01, 1 + speed * 0.015);
      a.mat.emissiveIntensity = p.stunned ? 2 + Math.sin(time * 40) * 1.5 : 0.25 + p.energy * 0.3;
      a.ring.rotation.z = time * 1.5;
      if (p.score > a.score) {
        const burst = new THREE.Mesh(burstGeo, new THREE.MeshBasicMaterial({ color: 0xffe066, transparent: true, side: THREE.DoubleSide }));
        burst.rotation.x = -Math.PI / 2;
        burst.position.set(p.x, 0.1, p.z);
        scene.add(burst);
        bursts.push({ mesh: burst, t: 0 });
      }
      a.score = p.score;
    }
    for (const [id, a] of avatars) {
      if (!seen.has(id)) {
        scene.remove(a.group);
        avatars.delete(id);
      }
    }

    orbs.count = Math.min(orbList.length, MAX_ORBS);
    for (let i = 0; i < orbs.count; i++) {
      const [x, z] = orbList[i];
      pos.set(x, 0.55 + Math.sin(time * 3 + i) * 0.15, z);
      q.setFromEuler(new THREE.Euler(time * 0.9 + i, time * 1.3 + i, 0));
      const k = 1 + Math.sin(time * 5 + i * 1.7) * 0.12;
      s.set(k, k, k);
      orbs.setMatrixAt(i, m.compose(pos, q, s));
    }
    orbs.instanceMatrix.needsUpdate = true;

    for (let i = bursts.length - 1; i >= 0; i--) {
      const b = bursts[i];
      b.t += dt;
      const k = 1 + b.t * 9;
      b.mesh.scale.set(k, k, k);
      (b.mesh.material as THREE.MeshBasicMaterial).opacity = Math.max(0, 1 - b.t * 2.2);
      if (b.t > 0.5) {
        scene.remove(b.mesh);
        bursts.splice(i, 1);
      }
    }

    const me = players.find((p) => p.you);
    const goal = me ? new THREE.Vector3(me.x * 0.55, 0, me.z * 0.55) : new THREE.Vector3();
    focus.lerp(goal, 1 - Math.exp(-dt * 4));
    camera.position.set(focus.x, 19, focus.z + 17);
    camera.lookAt(focus);
    composer.render(dt);
  }

  return {
    update,
    dispose() {
      stopFit();
      composer.dispose();
      renderer.dispose();
    },
  };
}

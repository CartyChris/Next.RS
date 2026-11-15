// A three.js game as a Forge page. No `render` export, so the page is client-only:
// the server (and build-time prerender) never evaluates three.js or touches WebGL.
// `forge build --target single-html` turns this into one self-contained HTML file.
import * as THREE from "three";
import { loop, input, fit } from "forge:game";
import { store } from "forge:store";
import fragmentShader from "./orb.frag";
import vertexShader from "./orb.vert";
import "./game.css";

export const meta = { title: "Orb Runner", description: "Collect the orbs. Arrow keys / WASD or drag." };

export function mount(el) {
  el.innerHTML = `<div class="stage"><canvas></canvas></div><div class="hud">Score <b id="score">0</b> · Best <b id="best">0</b></div>`;
  const canvas = el.querySelector("canvas");
  const scoreEl = el.querySelector("#score");
  const bestEl = el.querySelector("#best");

  const renderer = new THREE.WebGLRenderer({ canvas, antialias: true });
  const scene = new THREE.Scene();
  scene.fog = new THREE.Fog(0x05060a, 12, 40);
  const camera = new THREE.PerspectiveCamera(60, 1, 0.1, 100);
  camera.position.set(0, 9, 11);
  camera.lookAt(0, 0, 0);
  scene.add(new THREE.HemisphereLight(0x8899ff, 0x221100, 1.2));
  const sun = new THREE.DirectionalLight(0xffffff, 1.5);
  sun.position.set(5, 10, 3);
  scene.add(sun);

  const ARENA = 9;
  const floor = new THREE.Mesh(new THREE.PlaneGeometry(ARENA * 2, ARENA * 2), new THREE.MeshStandardMaterial({ color: 0x1b1f2a }));
  floor.rotation.x = -Math.PI / 2;
  scene.add(floor, new THREE.GridHelper(ARENA * 2, 18, 0x334, 0x223));

  const player = new THREE.Mesh(new THREE.BoxGeometry(1, 1, 1), new THREE.MeshStandardMaterial({ color: 0x4dabf7 }));
  player.position.y = 0.5;
  scene.add(player);

  const orbMaterial = new THREE.ShaderMaterial({ uniforms: { uTime: { value: 0 } }, vertexShader, fragmentShader });
  const orbs = Array.from({ length: 6 }, () => {
    const orb = new THREE.Mesh(new THREE.SphereGeometry(0.35, 24, 16), orbMaterial);
    place(orb);
    scene.add(orb);
    return orb;
  });
  function place(orb) {
    orb.position.set((Math.random() * 2 - 1) * (ARENA - 1), 0.5, (Math.random() * 2 - 1) * (ARENA - 1));
  }

  const keys = input();
  let score = 0;
  let best = store.get("orb-runner:best", 0);
  bestEl.textContent = best;
  const velocity = new THREE.Vector3();
  const prev = new THREE.Vector3();
  const target = new THREE.Vector3();
  let time = 0;

  const stopFit = fit(canvas, (w, h, dpr) => {
    renderer.setPixelRatio(dpr);
    renderer.setSize(w, h, false);
    camera.aspect = w / h;
    camera.updateProjectionMatrix();
  });

  const stopLoop = loop({
    step: 1 / 120,
    update(dt) {
      time += dt;
      prev.copy(player.position);
      target.set(keys.axis("ArrowLeft", "ArrowRight") || keys.axis("KeyA", "KeyD"), 0, keys.axis("ArrowUp", "ArrowDown") || keys.axis("KeyW", "KeyS"));
      if (keys.pointer.down) {
        target.set(keys.pointer.x / canvas.clientWidth - 0.5, 0, keys.pointer.y / canvas.clientHeight - 0.5).multiplyScalar(2);
      }
      if (target.lengthSq() > 1) target.normalize();
      velocity.lerp(target.multiplyScalar(9), 1 - Math.exp(-10 * dt));
      player.position.addScaledVector(velocity, dt);
      player.position.x = THREE.MathUtils.clamp(player.position.x, -ARENA + 0.5, ARENA - 0.5);
      player.position.z = THREE.MathUtils.clamp(player.position.z, -ARENA + 0.5, ARENA - 0.5);
      player.rotation.y += velocity.length() * dt;
      for (const orb of orbs) {
        orb.position.y = 0.5 + Math.sin(time * 3 + orb.position.x) * 0.15;
        if (orb.position.distanceToSquared(player.position) < 0.8) {
          place(orb);
          scoreEl.textContent = ++score;
          if (score > best) store.set("orb-runner:best", (bestEl.textContent = best = score));
        }
      }
      keys.endFrame();
    },
    render(alpha) {
      orbMaterial.uniforms.uTime.value = time;
      const drawn = prev.clone().lerp(player.position, alpha);
      camera.position.set(drawn.x * 0.5, 9, drawn.z * 0.5 + 11);
      camera.lookAt(drawn.x * 0.5, 0, drawn.z * 0.5);
      renderer.render(scene, camera);
    },
  });

  return () => {
    stopLoop();
    stopFit();
    keys.dispose();
    renderer.dispose();
  };
}

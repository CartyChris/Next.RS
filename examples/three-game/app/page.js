// A three.js game as a Forge page. No `render` export, so the page is client-only:
// the server (and build-time prerender) never evaluates three.js or touches WebGL.
// `forge build --target single-html` turns this into one self-contained HTML file.
import * as THREE from "three";
import { loop, input, fit, joystick, actionButton, gamepad, adaptiveQuality } from "forge:game";
import { store } from "forge:store";
import fragmentShader from "./orb.frag";
import vertexShader from "./orb.vert";
import "./game.css";

export const meta = { title: "Orb Runner", description: "Collect the orbs. Play with a keyboard, touch controls, or gamepad." };

export function mount(el) {
  el.innerHTML = `
    <div class="stage"><canvas aria-label="Orb Runner arena"></canvas></div>
    <div class="hud">
      <div>Score <b id="score">0</b> · Best <b id="best">0</b></div>
      <p id="controls-help">WASD / arrows, drag, or gamepad to move.<br>Hold Space, Shift, or the primary gamepad button to dash.</p>
    </div>
    <div class="touch-controls" role="group" aria-label="Touch controls">
      <div class="stick-control">
        <div id="game-stick" role="group" aria-label="Movement joystick" aria-describedby="controls-help" tabindex="0" data-active="false"><span class="stick-thumb" aria-hidden="true"></span></div>
        <span class="control-label">Move</span>
      </div>
      <button id="game-dash" type="button" aria-pressed="false" aria-keyshortcuts="Space Shift"><span>Dash</span><small>Hold</small></button>
    </div>`;
  const canvas = el.querySelector("canvas");
  const scoreEl = el.querySelector("#score");
  const bestEl = el.querySelector("#best");
  const stickEl = el.querySelector("#game-stick");
  const stickThumb = stickEl.querySelector(".stick-thumb");
  const dashEl = el.querySelector("#game-dash");

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
  // Canvas dragging has its own pointer state: a second finger can dash while
  // the joystick steers without either gesture changing the drag direction.
  const drag = input(canvas);
  const stick = joystick(stickEl, {
    onChange({ x, y, active }) {
      const travel = Math.max(0, (stickEl.clientWidth - stickThumb.clientWidth) / 2 - 6);
      stickThumb.style.transform = `translate(${x * travel}px, ${y * travel}px)`;
      stickEl.dataset.active = String(active);
    },
  });
  const pad = gamepad();
  const showDash = (down) => {
    const value = String(down);
    if (dashEl.getAttribute("aria-pressed") !== value) dashEl.setAttribute("aria-pressed", value);
  };
  const dash = actionButton(dashEl, { keys: ["Space", "ShiftLeft", "ShiftRight"], onChange: showDash });
  let score = 0;
  let best = store.get("orb-runner:best", 0);
  bestEl.textContent = best;
  const velocity = new THREE.Vector3();
  const prev = new THREE.Vector3();
  const target = new THREE.Vector3();
  const drawn = new THREE.Vector3();
  let time = 0, lastFrame = 0, disposed = false;

  const quality = adaptiveQuality({ maxDpr: 2, onChange: () => stopFit.refresh() });
  const stopFit = fit(canvas, (w, h, dpr) => {
    renderer.setPixelRatio(dpr);
    renderer.setSize(w, h, false);
    camera.aspect = w / h;
    camera.updateProjectionMatrix();
  }, { maxDpr: () => quality.dpr });
  const onVisibility = () => { lastFrame = 0; };
  document.addEventListener("visibilitychange", onVisibility);

  const stopLoop = loop({
    step: 1 / 120,
    update(dt) {
      time += dt;
      prev.copy(player.position);
      pad.poll();
      target.set(
        keys.axis("ArrowLeft", "ArrowRight") || keys.axis("KeyA", "KeyD") || pad.x || Number(pad.down(15)) - Number(pad.down(14)),
        0,
        keys.axis("ArrowUp", "ArrowDown") || keys.axis("KeyW", "KeyS") || pad.y || Number(pad.down(13)) - Number(pad.down(12)),
      );
      if (drag.pointer.down) {
        target.set(drag.pointer.x / canvas.clientWidth - 0.5, 0, drag.pointer.y / canvas.clientHeight - 0.5).multiplyScalar(2);
      }
      if (stick.active) target.set(stick.x, 0, stick.y);
      if (target.lengthSq() > 1) target.normalize();
      const dashing = dash.down || pad.down(0);
      showDash(dashing);
      velocity.lerp(target.multiplyScalar(dashing ? 14 : 9), 1 - Math.exp(-10 * dt));
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
      drag.endFrame();
      dash.endFrame();
    },
    render(alpha) {
      const now = performance.now();
      if (lastFrame) quality.sample(now - lastFrame);
      lastFrame = now;
      orbMaterial.uniforms.uTime.value = time;
      drawn.copy(prev).lerp(player.position, alpha);
      camera.position.set(drawn.x * 0.5, 9, drawn.z * 0.5 + 11);
      camera.lookAt(drawn.x * 0.5, 0, drawn.z * 0.5);
      renderer.render(scene, camera);
    },
  });

  return () => {
    if (disposed) return;
    disposed = true;
    stopLoop();
    stopFit();
    quality.dispose();
    document.removeEventListener("visibilitychange", onVisibility);
    keys.dispose();
    drag.dispose();
    stick.dispose();
    dash.dispose();
    pad.dispose();
    const geometries = new Set(), materials = new Set();
    scene.traverse((object) => {
      if (object.geometry) geometries.add(object.geometry);
      if (Array.isArray(object.material)) object.material.forEach((material) => materials.add(material));
      else if (object.material) materials.add(object.material);
    });
    geometries.forEach((geometry) => geometry.dispose());
    materials.forEach((material) => material.dispose());
    renderer.dispose();
    renderer.forceContextLoss();
  };
}

import { input } from "forge:game";

interface Camera {
  orbit(dx: number, dy: number): void;
  zoom(factor: number): void;
}

// forge:game owns pointer capture, cancellation, blur and page visibility.
// Snapshot positions after its handler runs. Rebase when the finger count
// changes, so lifting one finger after a pinch can't jump the camera.
export function bindOrbit(canvas: HTMLCanvasElement, camera: Camera) {
  const state = input(canvas);
  let previous = new Map<number, { x: number; y: number }>();
  const gesture = (event: PointerEvent) => {
    const points = [...state.pointers.values()];
    if (event.type === "pointermove" && points.length === previous.size) {
      const [first, second] = points;
      const before = first && previous.get(first.pointerId);
      if (before && points.length === 1) {
        const radiansPerPixel = 2 * Math.PI / Math.max(canvas.clientHeight, 1);
        camera.orbit((before.x - first.x) * radiansPerPixel, (before.y - first.y) * radiansPerPixel);
      } else if (before && points.length === 2) {
        const other = previous.get(second.pointerId);
        if (other) {
          const lastDistance = Math.hypot(before.x - other.x, before.y - other.y);
          const distance = Math.hypot(first.x - second.x, first.y - second.y);
          if (lastDistance > 1 && distance > 1) camera.zoom(lastDistance / distance);
        }
      }
    }
    previous = new Map(points.map(p => [p.pointerId, { x: p.x, y: p.y }]));
  };
  const events = ["pointerdown", "pointermove", "pointerup", "pointercancel", "lostpointercapture"] as const;
  for (const type of events) canvas.addEventListener(type, gesture);
  const wheel = (event: WheelEvent) => {
    event.preventDefault();
    const unit = event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? canvas.clientHeight : 1;
    camera.zoom(Math.exp(Math.max(-2, Math.min(2, event.deltaY * unit * 0.001))));
  };
  canvas.addEventListener("wheel", wheel, { passive: false });
  return () => {
    for (const type of events) canvas.removeEventListener(type, gesture);
    canvas.removeEventListener("wheel", wheel);
    state.dispose();
  };
}

// Neon grid floor: anti-aliased lines, a slow radial pulse, and a glowing arena edge.
uniform float uTime;
uniform float uArena;
varying vec2 vPos;

void main() {
  vec2 p = vPos;
  vec2 cell = p * 0.5;
  vec2 g = abs(fract(cell - 0.5) - 0.5) / fwidth(cell);
  float line = 1.0 - min(min(g.x, g.y), 1.0);
  float box = max(abs(p.x), abs(p.y));
  float inside = step(box, uArena);
  float pulse = 0.5 + 0.5 * sin(uTime * 1.6 - length(p) * 0.4);
  vec3 base = vec3(0.015, 0.02, 0.045);
  vec3 grid = mix(vec3(0.04, 0.18, 0.38), vec3(0.08, 0.55, 0.95), pulse) * line * (0.25 + 0.75 * inside);
  float edge = smoothstep(0.45, 0.0, abs(box - uArena));
  gl_FragColor = vec4(base + grid * 0.45 + vec3(0.25, 0.85, 1.0) * edge * 0.8, 1.0);
}

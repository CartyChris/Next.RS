uniform float uTime;
varying vec3 vNormal;
void main() {
  float rim = pow(1.0 - abs(vNormal.z), 2.0);
  vec3 color = mix(vec3(1.0, 0.75, 0.2), vec3(1.0, 0.95, 0.6), 0.5 + 0.5 * sin(uTime * 4.0));
  gl_FragColor = vec4(color + rim * 0.8, 1.0);
}

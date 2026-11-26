#version 300 es
precision highp float;
in vec3 worldNormal;
uniform vec3 baseColor; // linear RGB
uniform bool showNormals;
out vec4 fragment;

void main() {
    vec3 n = normalize(worldNormal);
    if (showNormals) {
        fragment = vec4(n * 0.5 + 0.5, 1.0);
        return;
    }
    float key = max(dot(n, normalize(vec3(4.0, 6.0, 5.0))), 0.0);
    float rim = max(dot(n, normalize(vec3(-4.0, 2.0, -3.0))), 0.0);
    vec3 light = vec3(0.18 + 0.12 * (n.y + 1.0))
               + vec3(1.0, 0.86, 0.68) * key + vec3(0.32, 0.63, 0.70) * rim;
    fragment = vec4(pow(clamp(baseColor * light, 0.0, 1.0), vec3(1.0 / 2.2)), 1.0);
}

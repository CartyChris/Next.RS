#version 300 es
layout(location=0) in vec3 position;
layout(location=1) in vec3 normal;
uniform mat4 viewProjection;
uniform vec4 centerInvRadius;
uniform vec2 yaw; // sine, cosine
out vec3 worldNormal;

vec3 rotateY(vec3 p) {
    return vec3(yaw.y * p.x + yaw.x * p.z, p.y, yaw.y * p.z - yaw.x * p.x);
}
void main() {
    vec3 object = (position - centerInvRadius.xyz) * centerInvRadius.w;
    gl_Position = viewProjection * vec4(rotateY(object), 1.0);
    worldNormal = rotateY(normal);
}

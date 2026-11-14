// Grass policy from PR255: rolling gusts, independent blade phase, anchored roots.
fn deform(position: vec3<f32>, uv: vec2<f32>, seconds: f32, parameters: vec4<f32>) -> vec3<f32> {
    let gust = 0.6 * sin(dot(position.xz, vec2(0.11, 0.04)) - seconds * 1.3)
        + 0.4 * sin(dot(position.xz, vec2(-0.05, 0.23)) - seconds * 2.1 + uv.x * 6.283185307);
    let bend = parameters.x * (0.6 + 0.4 * gust) * uv.y * uv.y;
    return position + vec3(0.94 * bend, -0.25 * abs(bend), 0.34 * bend);
}

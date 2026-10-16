// Modern water: animated ripples in the texture and a Fresnel sheen toward the sky colour.
// Implements the water_surface library contract of cyrus-render.

fn water_ripple(world_position: vec3<f32>, time: f32) -> vec2<f32> {
    let p = world_position.xz;
    return vec2(sin(dot(p, vec2(1.3, 0.7)) - time * 1.7),
                sin(dot(p, vec2(-0.6, 2.1)) - time * 2.3));
}

fn water_uv_offset(world_position: vec3<f32>, time: f32, weight: f32) -> vec2<f32> {
    return water_ripple(world_position, time) * (0.006 * weight);
}

fn water_color(color: vec3<f32>, world_position: vec3<f32>, normal: vec3<f32>, camera: vec3<f32>, sky: vec3<f32>, time: f32, weight: f32) -> vec3<f32> {
    let ripple = water_ripple(world_position, time);
    let rippled_normal = normalize(normal + vec3(ripple.x, 0.0, ripple.y) * (0.12 * weight));
    let to_camera = normalize(camera - world_position);
    let sheen = pow(1.0 - clamp(dot(rippled_normal, to_camera), 0.0, 1.0), 5.0);
    let rippled = color * (1.0 + 0.035 * (ripple.x + ripple.y) * weight);
    return mix(rippled, sky, sheen * 0.12 * weight);
}

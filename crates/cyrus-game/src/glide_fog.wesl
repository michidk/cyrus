import bevy_render::color_operations::{linear_to_srgb, srgb_to_linear};

// Verified continuous Glide lookup and encoded-channel blend, preservation docs/engine/fog.md.
fn fog_coordinate(depth: f32) -> f32 {
    let exponent = floor(log2(max(depth, 1.0)));
    return clamp(4.0 * exponent + 8.0 * (1.0 - exp2(exponent) / max(depth, 1.0)), 0.0, 63.0);
}

fn blend_fog(linear_rgb: vec3<f32>, fog_rgb: vec3<f32>, density: f32) -> vec3<f32> {
    return srgb_to_linear(mix(linear_to_srgb(linear_rgb), fog_rgb, (density + 1.0) / 256.0));
}

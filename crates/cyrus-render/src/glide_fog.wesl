import bevy_render::color_operations::{linear_to_srgb, srgb_to_linear};

// Verified continuous Glide lookup and encoded-channel blend, preservation docs/engine/fog.md.
fn fog_coordinate(depth: f32) -> f32 {
    let exponent = floor(log2(max(depth, 1.0)));
    return clamp(4.0 * exponent + 8.0 * (1.0 - exp2(exponent) / max(depth, 1.0)), 0.0, 63.0);
}

fn blend_fog(linear_rgb: vec3<f32>, fog_rgb: vec3<f32>, density: f32) -> vec3<f32> {
    return srgb_to_linear(mix(linear_to_srgb(linear_rgb), fog_rgb, (density + 1.0) / 256.0));
}

// Voodoo 1 reciprocal-depth precision, per preservation's fog documentation.
fn fog_table_position(depth: f32) -> vec2<u32> {
    if depth <= 1.0 { return vec2(0u); }
    return fog_table_position_from_reciprocal(u32(4294967296.0 / depth));
}

// Positive fixed reciprocal depth below 2^32, after pixel stepping.
// https://github.com/michidk/redguard-preservation/blob/e8588b5/docs/engine/fog.md
fn fog_table_position_from_reciprocal(reciprocal: u32) -> vec2<u32> {
    var encoded = 65535u;
    if reciprocal >= 65536u {
        let exponent = countLeadingZeros(reciprocal);
        encoded = min(65535u, ((exponent << 12u) | ((~reciprocal >> (19u - exponent)) & 4095u)) + 1u);
    }
    return vec2(encoded >> 10u, (encoded >> 2u) & 255u);
}

fn table_density(current: f32, next: f32, fraction: u32) -> f32 {
    let delta = ((u32(next) + 256u - u32(current)) * 4u) % 256u;
    return current + f32((delta * fraction) >> 10u);
}

fn blend_fog_integer(linear_rgb: vec3<f32>, fog_rgb: vec3<f32>, density: f32) -> vec3<f32> {
    let color = round(clamp(linear_to_srgb(linear_rgb), vec3(0.0), vec3(1.0)) * 255.0);
    let fog = round(fog_rgb * 255.0);
    let result = clamp(color + floor((fog - color) * ((density + 1.0) / 256.0)), vec3(0.0), vec3(255.0));
    return srgb_to_linear(result / 255.0);
}

// Constant/texture alpha arithmetic before framebuffer storage quantization,
// preservation model-lighting.md and sky.md.
fn framebuffer_blend_integer(destination: vec3<f32>, tint: vec3<f32>, alpha: u32) -> vec3<f32> {
    let encoded = vec3<u32>(round(clamp(linear_to_srgb(destination), vec3(0.0), vec3(1.0)) * 255.0));
    let source = vec3<u32>(round(clamp(tint, vec3(0.0), vec3(1.0)) * 255.0));
    let blended = (source * (alpha + 1u)) / vec3(256u)
        + (encoded * (256u - alpha)) / vec3(256u);
    return srgb_to_linear(vec3<f32>(min(blended, vec3(255u))) / 255.0);
}

// Stored framebuffer channels, preservation docs/engine/fog.md.
fn framebuffer_dither(linear_rgb: vec3<f32>, pixel: vec2<u32>) -> vec3<f32> {
    let thresholds = array<u32, 16>(0u, 8u, 2u, 10u, 12u, 4u, 14u, 6u, 3u, 11u, 1u, 9u, 15u, 7u, 13u, 5u);
    let threshold = thresholds[(pixel.y % 4u) * 4u + pixel.x % 4u];
    let color = vec3<u32>(round(clamp(linear_to_srgb(linear_rgb), vec3(0.0), vec3(1.0)) * 255.0));
    let red_blue = min((2u * color - color / 16u + color / 128u + vec3(threshold)) / 16u, vec3(31u));
    let green = min((4u * color.y - color.y / 16u + color.y / 64u + threshold) / 16u, 63u);
    let expanded = vec3<u32>((red_blue.x << 3u) | (red_blue.x >> 2u), (green << 2u) | (green >> 4u), (red_blue.z << 3u) | (red_blue.z >> 2u));
    return srgb_to_linear(vec3<f32>(expanded) / 255.0);
}

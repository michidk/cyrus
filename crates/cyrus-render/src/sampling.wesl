import bevy_pbr::render::pbr_bindings::{base_color_texture, base_color_sampler};
import bevy_render::color_operations::linear_to_srgb;

// Software Voodoo 1 filtering uses encoded bytes and four-bit fractions.
fn sample_encoded(uv: vec2<f32>, point: bool) -> vec3<f32> {
    if point { return round(linear_to_srgb(textureSampleLevel(base_color_texture, base_color_sampler, uv, 0.0).rgb) * 255.0) / 255.0; }
    let size = vec2<f32>(textureDimensions(base_color_texture));
    let texel = uv * size - vec2(0.5);
    // Exported images reverse rows, so original downward V rounding reverses too.
    let fraction = vec2(floor(fract(texel.x) * 16.0), ceil(fract(texel.y) * 16.0)) / 16.0;
    let origin = floor(texel) + vec2(0.5);
    let a = round(linear_to_srgb(textureSampleLevel(base_color_texture, base_color_sampler, origin / size, 0.0).rgb) * 255.0);
    let b = round(linear_to_srgb(textureSampleLevel(base_color_texture, base_color_sampler, (origin + vec2(1.0, 0.0)) / size, 0.0).rgb) * 255.0);
    let c = round(linear_to_srgb(textureSampleLevel(base_color_texture, base_color_sampler, (origin + vec2(0.0, 1.0)) / size, 0.0).rgb) * 255.0);
    let d = round(linear_to_srgb(textureSampleLevel(base_color_texture, base_color_sampler, (origin + vec2(1.0)) / size, 0.0).rgb) * 255.0);
    let top = a + floor((b - a) * fraction.x);
    let bottom = c + floor((d - c) * fraction.x);
    return (top + floor((bottom - top) * fraction.y)) / 255.0;
}


// The original compares filtered encoded RGB before vertex-light modulation.
fn matches_chroma(rgb: vec3<f32>, key: vec3<f32>) -> bool {
    return all(round(rgb * 255.0) == key);
}

// Signed Voodoo color planes use twelve fractional bits and per-pixel stepping.
fn color_channel(plane: vec4<i32>, offset: vec2<i32>) -> f32 {
    let value = plane.x + offset.x * plane.y + offset.y * plane.z;
    let channel = (value >> 12u) & 4095;
    return f32(select(select(channel & 255, 255, channel == 256), 0, channel == 4095));
}

// Sample the reduced, zero-padded allocation without changing shared source images.
// https://github.com/michidk/redguard-preservation/commit/955cc9f
fn model_texel(pixel: vec2<i32>, allocation: vec4<u32>, zero: vec3<f32>) -> vec3<f32> {
    let wrapped = vec2<u32>(pixel) & (allocation.xy - vec2(1u));
    let source = wrapped * allocation.zw;
    let size = textureDimensions(base_color_texture);
    if any(source >= size) { return zero; }
    let flipped = vec2<i32>(vec2(source.x, size.y - 1u - source.y));
    return round(linear_to_srgb(textureLoad(base_color_texture, flipped, 0).rgb) * 255.0);
}

fn sample_model(coordinate: vec2<i32>, allocation: vec4<u32>, level: u32, point: bool, zero: vec3<f32>) -> vec3<f32> {
    if point {
        return model_texel(coordinate >> vec2(level + 18u), allocation, zero) / 255.0;
    }
    let fixed = (coordinate >> vec2(level + 14u)) - vec2(8);
    let origin = fixed >> vec2(4u);
    let fraction = vec2<f32>(fixed & vec2(15)) / 16.0;
    let a = model_texel(origin, allocation, zero);
    let b = model_texel(origin + vec2(1, 0), allocation, zero);
    let c = model_texel(origin + vec2(0, 1), allocation, zero);
    let d = model_texel(origin + vec2(1), allocation, zero);
    let top = a + floor((b - a) * fraction.x);
    let bottom = c + floor((d - c) * fraction.x);
    return (top + floor((bottom - top) * fraction.y)) / 255.0;
}

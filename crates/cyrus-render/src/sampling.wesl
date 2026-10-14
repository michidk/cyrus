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


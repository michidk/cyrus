import package::scene_shading::fog;
import bevy_render::color_operations::linear_to_srgb;
@if(BINDLESS)
import bevy_render::bindless::{bindless_textures_2d, bindless_samplers_filtering};
@if(BINDLESS)
import bevy_pbr::render::pbr_bindings::material_indices;
@if(!BINDLESS)
import bevy_pbr::render::pbr_bindings::{base_color_texture, base_color_sampler};
@if(BINDLESS)
import package::scene_shading::material_slot;

// The drawn material's base colour texture, through the bindless arrays where Bevy uses them.
fn base_color_level(uv: vec2<f32>) -> vec4<f32> {
@if(BINDLESS) {
    let bindings = material_indices[material_slot];
    return textureSampleLevel(bindless_textures_2d[bindings.base_color_texture], bindless_samplers_filtering[bindings.base_color_sampler], uv, 0.0);
}
@if(!BINDLESS) {
    return textureSampleLevel(base_color_texture, base_color_sampler, uv, 0.0);
}
}

fn base_color_load(pixel: vec2<i32>) -> vec4<f32> {
@if(BINDLESS) {
    return textureLoad(bindless_textures_2d[material_indices[material_slot].base_color_texture], pixel, 0);
}
@if(!BINDLESS) {
    return textureLoad(base_color_texture, pixel, 0);
}
}

fn base_color_size() -> vec2<u32> {
@if(BINDLESS) {
    return textureDimensions(bindless_textures_2d[material_indices[material_slot].base_color_texture]);
}
@if(!BINDLESS) {
    return textureDimensions(base_color_texture);
}
}

// World COL channels pass through six-bit storage before texture upload.
// https://github.com/michidk/redguard-preservation/commit/395e669
fn encoded_texel(uv: vec2<f32>, world_palette: bool) -> vec3<f32> {
    let rgb = round(linear_to_srgb(base_color_level(uv).rgb) * 255.0);
    return select(rgb, floor(rgb / 4.0) * 4.0, world_palette);
}

// Software Voodoo 1 filtering uses encoded bytes and four-bit fractions.
fn sample_encoded_single(uv: vec2<f32>, point: bool, world_palette: bool) -> vec3<f32> {
    if point { return encoded_texel(uv, world_palette) / 255.0; }
    let size = vec2<f32>(base_color_size());
    let texel = uv * size - vec2(0.5);
    // Exported images reverse rows, so original downward V rounding reverses too.
    let fraction = vec2(floor(fract(texel.x) * 16.0), ceil(fract(texel.y) * 16.0)) / 16.0;
    let origin = floor(texel) + vec2(0.5);
    let a = encoded_texel(origin / size, world_palette);
    let b = encoded_texel((origin + vec2(1.0, 0.0)) / size, world_palette);
    let c = encoded_texel((origin + vec2(0.0, 1.0)) / size, world_palette);
    let d = encoded_texel((origin + vec2(1.0)) / size, world_palette);
    let top = a + floor((b - a) * fraction.x);
    let bottom = c + floor((d - c) * fraction.x);
    return (top + floor((bottom - top) * fraction.y)) / 255.0;
}

// Placed flats point-sample the converted world palette; exported alpha marks index zero.
// https://github.com/michidk/redguard-preservation/commit/fe530cc
fn sample_flat_single(uv: vec2<f32>) -> vec3<f32> {
    let texel = base_color_level(uv);
    let rgb = round(linear_to_srgb(texel.rgb) * 255.0);
    return select(floor(rgb / 4.0) * 4.0, vec3(0.0), texel.a == 0.0) / 255.0;
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
    let size = base_color_size();
    if any(source >= size) { return zero; }
    let flipped = vec2<i32>(vec2(source.x, size.y - 1u - source.y));
    // World model textures share the converted COL table; exported alpha marks index zero.
    // https://github.com/michidk/redguard-preservation/commit/fe530cc
    let texel = base_color_load(flipped);
    let rgb = round(linear_to_srgb(texel.rgb) * 255.0);
    return select(floor(rgb / 4.0) * 4.0, zero, texel.a == 0.0);
}

fn sample_model_single(coordinate: vec2<i32>, allocation: vec4<u32>, level: u32, point: bool, zero: vec3<f32>) -> vec3<f32> {
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

// Modern footprint integration preserves indexed allocations without mipmaps.
// Up to sixteen taps cover the major axis of a pixel's texture footprint.
fn anisotropic_axis(dx: vec2<f32>, dy: vec2<f32>) -> vec3<f32> {
    let xx = dot(dx, dx);
    let yy = dot(dy, dy);
    let major = select(dy, dx, xx >= yy);
    let count = clamp(ceil(length(major) / max(sqrt(min(xx, yy)), 1.0)), 1.0, 16.0);
    return vec3(major, count);
}

fn sample_encoded(uv: vec2<f32>, point: bool, world_palette: bool) -> vec3<f32> {
    let size = vec2<f32>(base_color_size());
    let axis = anisotropic_axis(dpdx(uv) * size, dpdy(uv) * size);
    if fog.lighting.z == 0.0 { return sample_encoded_single(uv, point, world_palette); }
    var sum = vec3(0.0);
    for (var i = 0u; i < u32(axis.z); i++) {
        sum += sample_encoded_single(uv + axis.xy / size * ((f32(i) + 0.5) / axis.z - 0.5), false, world_palette);
    }
    return sum / axis.z;
}

fn sample_flat(uv: vec2<f32>) -> vec3<f32> {
    let size = vec2<f32>(base_color_size());
    let axis = anisotropic_axis(dpdx(uv) * size, dpdy(uv) * size);
    if fog.lighting.z == 0.0 { return sample_flat_single(uv); }
    var sum = vec3(0.0);
    for (var i = 0u; i < u32(axis.z); i++) {
        sum += sample_flat_single(uv + axis.xy / size * ((f32(i) + 0.5) / axis.z - 0.5));
    }
    return sum / axis.z;
}

fn sample_model(coordinate: vec2<i32>, allocation: vec4<u32>, level: u32, point: bool, zero: vec3<f32>) -> vec3<f32> {
    let scale = exp2(f32(level + 18u));
    let position = vec2<f32>(coordinate) / scale;
    let axis = anisotropic_axis(dpdx(position), dpdy(position));
    if fog.lighting.z == 0.0 { return sample_model_single(coordinate, allocation, level, point, zero); }
    var sum = vec3(0.0);
    for (var i = 0u; i < u32(axis.z); i++) {
        let offset = axis.xy * ((f32(i) + 0.5) / axis.z - 0.5) * scale;
        sum += sample_model_single(coordinate + vec2<i32>(offset), allocation, level, false, zero);
    }
    return sum / axis.z;
}
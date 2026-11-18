// Textured models lit by per-corner levels, as the original's Glide colour combine draws them:
// the interpolated level becomes an integer L per channel and each 8-bit texel channel T becomes
// floor(T * (L + bias) / 256). Levels interpolate in screen space, texels in perspective. Textures are
// raw 8-bit values, so bilinear filtering also mixes them in that space.
import bevy_pbr::render::mesh_view_bindings::view;
import constants::MATERIAL_BIND_GROUP;
import bevy_render::color_operations::srgb_to_linear;

@group(MATERIAL_BIND_GROUP) @binding(0) var lit_texture: texture_2d<f32>;
@group(MATERIAL_BIND_GROUP) @binding(1) var lit_sampler: sampler;
@group(MATERIAL_BIND_GROUP) @binding(2) var<uniform> four_bit_filter: vec4<u32>;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @if(!WEBGL2)
    @location(1) @interpolate(linear) level: vec4<f32>,
    @if(WEBGL2)
    @location(1) level: vec4<f32>,
};

@vertex
fn vertex(
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) level: vec4<f32>,
) -> VertexOutput {
    var out: VertexOutput;
    out.position = view.clip_from_world * vec4(position, 1.0);
    out.uv = uv;
    out.level = level;
    // Cancel perspective interpolation on WebGL, which has no noperspective qualifier.
    @if(WEBGL2) {
        out.level *= out.position.w;
    }
    return out;
}

// Book sampling follows preservation's encoded-channel Voodoo filter: four-bit
// fractions, horizontal integer interpolation, then vertical integer interpolation.
fn wrapped_texel(pixel: vec2<i32>, size: vec2<i32>) -> vec3<f32> {
    let wrapped = ((pixel % size) + size) % size;
    return round(textureLoad(lit_texture, wrapped, 0).rgb * 255.0);
}

fn book_sample(uv: vec2<f32>) -> vec3<f32> {
    let size = vec2<i32>(textureDimensions(lit_texture));
    let texel = uv * vec2<f32>(size) - 0.5;
    let pixel = vec2<i32>(floor(texel));
    let weight = floor(fract(texel) * 16.0) / 16.0;
    let a = wrapped_texel(pixel, size);
    let b = wrapped_texel(pixel + vec2(1, 0), size);
    let c = wrapped_texel(pixel + vec2(0, 1), size);
    let d = wrapped_texel(pixel + vec2(1, 1), size);
    let upper = a + floor((b - a) * weight.x);
    let lower = c + floor((d - c) * weight.x);
    return upper + floor((lower - upper) * weight.y);
}

// Published software-Voodoo 4x4 framebuffer storage, in encoded channels.
// The book target uses top-left framebuffer coordinates before its display sprite.
fn book_framebuffer(colour: vec3<f32>, position: vec2<f32>) -> vec3<f32> {
    let matrix = array<u32, 16>(0u, 8u, 2u, 10u, 12u, 4u, 14u, 6u,
                                3u, 11u, 1u, 9u, 15u, 7u, 13u, 5u);
    let pixel = vec2<u32>(position);
    let threshold = f32(matrix[(pixel.y % 4u) * 4u + pixel.x % 4u]);
    let five = vec3<f32>(2.0) * colour - floor(colour / 16.0)
        + floor(colour / 128.0) + threshold;
    var stored = vec3<u32>(floor(five / 16.0));
    stored.y = u32(floor((4.0 * colour.y - floor(colour.y / 16.0)
        + floor(colour.y / 64.0) + threshold) / 16.0));
    let red_blue = (stored << vec3<u32>(3u)) | (stored >> vec3<u32>(2u));
    let green = (stored.y << 2u) | (stored.y >> 4u);
    return vec3<f32>(f32(red_blue.x), f32(green), f32(red_blue.z));
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    var screen_level = in.level;
    @if(WEBGL2) {
        screen_level *= in.position.w;
    }
    let texel = textureSample(lit_texture, lit_sampler, in.uv);
    // Alpha zero is the colour key: the original draws no pixel there.
    if texel.a < 0.5 / 255.0 {
        discard;
    }
    var raw = round(texel.rgb * 255.0);
    if four_bit_filter.x != 0u {
        raw = book_sample(in.uv);
    }
    let level = floor(clamp(screen_level.rgb, vec3(0.0), vec3(255.0)));
    var lit = floor(raw * (level + screen_level.w) / 256.0);
    if four_bit_filter.x != 0u || four_bit_filter.y != 0u {
        lit = book_framebuffer(lit, in.position.xy);
    }
    return vec4(srgb_to_linear(lit / 255.0), texel.a);
}

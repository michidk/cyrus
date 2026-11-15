// Textured models lit by per-corner levels, as the original's Glide colour combine draws them:
// the interpolated level becomes an integer L per channel and each 8-bit texel channel T becomes
// floor(T * (L + bias) / 256). Levels interpolate in screen space, texels in perspective. Textures are
// raw 8-bit values, so bilinear filtering also mixes them in that space.
import bevy_pbr::render::mesh_view_bindings::view;
import constants::MATERIAL_BIND_GROUP;
import bevy_render::color_operations::srgb_to_linear;

@group(MATERIAL_BIND_GROUP) @binding(0) var lit_texture: texture_2d<f32>;
@group(MATERIAL_BIND_GROUP) @binding(1) var lit_sampler: sampler;
@group(MATERIAL_BIND_GROUP) @binding(2) var<uniform> four_bit_filter: u32;

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
    if four_bit_filter != 0u {
        raw = book_sample(in.uv);
    }
    let level = floor(clamp(screen_level.rgb, vec3(0.0), vec3(255.0)));
    let lit = floor(raw * (level + screen_level.w) / 256.0);
    return vec4(srgb_to_linear(lit / 255.0), texel.a);
}

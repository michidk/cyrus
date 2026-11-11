// The Windows 3dfx splash's draws: a texel times the vertex's interpolated colour, both in the
// 8-bit space the original's Glide hardware mixed them in, scaled by the splash's fog visibility.
// Colours interpolate in screen space, texels in perspective.
import bevy_pbr::render::mesh_view_bindings::view;
import constants::MATERIAL_BIND_GROUP;
import bevy_render::color_operations::srgb_to_linear;

@group(MATERIAL_BIND_GROUP) @binding(0) var splash_texture: texture_2d<f32>;
@group(MATERIAL_BIND_GROUP) @binding(1) var splash_sampler: sampler;
@group(MATERIAL_BIND_GROUP) @binding(2) var<uniform> visibility: vec4<f32>;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(linear) color: vec3<f32>,
};

@vertex
fn vertex(
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
) -> VertexOutput {
    var out: VertexOutput;
    out.position = view.clip_from_world * vec4(position, 1.0);
    out.uv = uv;
    out.color = color.rgb;
    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let texel = textureSample(splash_texture, splash_sampler, in.uv).rgb;
    let mixed = clamp(in.color / 255.0 * texel * visibility.x, vec3(0.0), vec3(1.0));
    return vec4(srgb_to_linear(mixed), 1.0);
}

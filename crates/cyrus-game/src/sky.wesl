import bevy_pbr::render::mesh_view_bindings::view;
import constants::MATERIAL_BIND_GROUP;
import package::glide_fog::{fog_coordinate, blend_fog};

@group(MATERIAL_BIND_GROUP) @binding(0) var sky: texture_2d<f32>;
@group(MATERIAL_BIND_GROUP) @binding(1) var sky_sampler: sampler;
@group(MATERIAL_BIND_GROUP) @binding(2) var<uniform> plane: vec4<f32>;
@group(MATERIAL_BIND_GROUP) @binding(3) var<uniform> fog_rgb: vec4<f32>;
@group(MATERIAL_BIND_GROUP) @binding(4) var<uniform> scroll: vec4<f32>;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
};

@vertex
fn vertex(@location(0) position: vec3<f32>) -> VertexOutput {
    return VertexOutput(vec4(position, 1.0));
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let screen_uv = (in.position.xy - view.main_pass_viewport.xy) / view.main_pass_viewport.zw;
    let near_h = view.view_from_clip * vec4(screen_uv * vec2(2.0, -2.0) + vec2(-1.0, 1.0), 1.0, 1.0);
    let near = near_h.xyz / near_h.w;
    let ray = (view.world_from_view * vec4(near, 0.0)).xyz;
    if ray.y == 0.0 { discard; }
    let distance = (-plane.x / 20.0 - view.world_position.y) / ray.y;
    let relative_xz = ray.xz * distance;
    let depth = -near.z * distance;
    if distance <= 0.0 || depth < 0.05 || depth > 3250.0 || any(abs(relative_xz) > vec2(3250.0)) {
        discard;
    }
    // Verified texel mapping in preservation's docs/engine/sky.md; scene export flips X/Y.
    let uv = (-plane.y * relative_xz * 20.0 / 65000.0 - view.world_position.xz * 20.0 * 0.009 + scroll.xy) / plane.zw;
    let coordinate = fog_coordinate(depth * 20.0);
    let density = select(clamp(coordinate - 32.0, 0.0, 30.0) * 8.0,
                         240.0 + (coordinate - 62.0) * 15.0, coordinate >= 62.0);
    let color = textureSampleLevel(sky, sky_sampler, uv, 0.0).rgb;
    return vec4(blend_fog(color, fog_rgb.rgb, density), 1.0);
}

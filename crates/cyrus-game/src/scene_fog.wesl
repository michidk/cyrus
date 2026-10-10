import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::view,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting},
    pbr_types::STANDARD_MATERIAL_FLAGS_UNLIT_BIT,
};
@if(TONEMAP_IN_SHADER)
import bevy_core_pipeline::tonemapping::{tone_mapping, screen_space_dither};
@if(TONEMAP_IN_SHADER)
import bevy_render::maths::powsafe;
@if(PREMULTIPLY_ALPHA)
import bevy_pbr::render::pbr_functions::premultiply_alpha;
import constants::MATERIAL_BIND_GROUP;
import package::glide_fog::{fog_coordinate, blend_fog};

struct SceneFog {
    color: vec4<f32>,
    clipping: vec4<f32>,
    table: array<vec4<f32>, 16>,
};
@group(MATERIAL_BIND_GROUP) @binding(100) var<uniform> fog: SceneFog;

fn density_at(index: u32) -> f32 {
    return fog.table[index / 4u][index % 4u];
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    let depth = -(view.view_from_world * in.world_position).z * 20.0;
    // Bevy's infinite perspective projection only uses far for object culling.
    if depth < fog.clipping.x || depth > fog.clipping.y { discard; }
    var input = pbr_input_from_standard_material(in, is_front);
    input.material.base_color = alpha_discard(input.material.flags, input.material.alpha_cutoff, input.material.base_color);
    var out: FragmentOutput;
    if (input.material.flags & STANDARD_MATERIAL_FLAGS_UNLIT_BIT) == 0u {
        out.color = apply_pbr_lighting(input);
    } else {
        out.color = input.material.base_color;
    }
@if(TONEMAP_IN_SHADER) {
    out.color = tone_mapping(out.color, view.color_grading);
@if(DEBAND_DITHER) {
    var rgb = powsafe(out.color.rgb, 1.0 / 2.2);
    rgb += screen_space_dither(input.frag_coord.xy);
    out.color = vec4(powsafe(rgb, 2.2), out.color.a);
}
}
    let coordinate = fog_coordinate(depth);
    let index = u32(floor(coordinate));
    let density = density_at(index);
    let next = density_at(min(index + 1u, 63u));
    let delta = f32((u32(next) + 256u - u32(density)) * 4u % 256u) / 4.0;
    out.color = vec4(blend_fog(out.color.rgb, fog.color.rgb, density + delta * fract(coordinate)), out.color.a);
@if(PREMULTIPLY_ALPHA) {
    out.color = premultiply_alpha(input.material.flags, out.color);
}
    return out;
}

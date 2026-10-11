import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::{view, globals},
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
import package::glide_fog::{fog_table_position, table_density, blend_fog, blend_fog_integer, framebuffer_rgb565};

struct SceneFog {
    color: vec4<f32>,
    clipping: vec4<f32>,
    table: array<vec4<f32>, 16>,
    water: vec4<f32>,
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
    var surface = in;
    var water_weight = 0.0;
    var ripple = vec2(0.0);
@if(VERTEX_UVS_B) {
    water_weight = fog.water.x * fog.water.y * clamp(in.uv_b.x, 0.0, 1.0);
    let position = in.world_position.xz;
    ripple = vec2(sin(dot(position, vec2(1.3, 0.7)) - globals.time * 1.7),
                  sin(dot(position, vec2(-0.6, 2.1)) - globals.time * 2.3));
@if(VERTEX_UVS_A) {
    surface.uv += ripple * (0.006 * water_weight);
}
}
    var input = pbr_input_from_standard_material(surface, is_front);
    if water_weight > 0.0 {
        input.N = normalize(input.N + vec3(ripple.x, 0.0, ripple.y) * (0.12 * water_weight));
    }
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
    if water_weight > 0.0 {
        let to_camera = normalize(view.world_position - in.world_position.xyz);
        let sheen = pow(1.0 - clamp(dot(input.N, to_camera), 0.0, 1.0), 5.0);
        let reflected_sky = blend_fog(vec3(0.0), fog.color.rgb, 255.0);
        let rippled = out.color.rgb * (1.0 + 0.035 * (ripple.x + ripple.y) * water_weight);
        out.color = vec4(mix(rippled, reflected_sky, sheen * 0.12 * water_weight), out.color.a);
    }
    if fog.clipping.z != 0.0 {
        let density = 255.0 * smoothstep(fog.clipping.y * 0.45, fog.clipping.y, depth);
        out.color = vec4(blend_fog(out.color.rgb, fog.color.rgb, density), out.color.a);
    } else {
        let position = fog_table_position(depth);
        let density = table_density(density_at(position.x), density_at(min(position.x + 1u, 63u)), position.y);
        let fogged = blend_fog_integer(out.color.rgb, fog.color.rgb, density);
        out.color = vec4(framebuffer_rgb565(fogged, in.position.xy), out.color.a);
    }
@if(PREMULTIPLY_ALPHA) {
    out.color = premultiply_alpha(input.material.flags, out.color);
}
    return out;
}

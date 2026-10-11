import bevy_pbr::render::{
    forward_io::{Vertex, VertexOutput, FragmentOutput, decompress_vertex},
    mesh_functions,
    mesh_view_bindings::{view, globals},
    view_transformations::position_world_to_clip,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
};
import bevy_render::color_operations::{linear_to_srgb, srgb_to_linear};
import package::scene_shading::{fog, scene_color};
import package::glide_fog::blend_fog;

struct TerrainVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) water: f32,
    @location(4) @interpolate(linear) color: vec3<f32>,
    @location(5) @interpolate(flat) instance_index: u32,
};

@vertex
fn vertex(raw: Vertex) -> TerrainVertex {
    let vertex = decompress_vertex(raw, raw.instance_index);
    let transform = mesh_functions::get_world_from_local(raw.instance_index);
    var out: TerrainVertex;
    out.world_position = mesh_functions::mesh_position_local_to_world(transform, vec4(vertex.position, 1.0));
    out.position = position_world_to_clip(out.world_position.xyz);
    out.normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, raw.instance_index);
    out.uv = vertex.uv;
@if(VERTEX_UVS_B)
    out.water = vertex.uv_b.x;
@if(VERTEX_COLORS)
    out.color = vertex.color.rgb;
    out.instance_index = raw.instance_index;
    return out;
}

@fragment
fn fragment(in: TerrainVertex, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    let depth = -(view.view_from_world * in.world_position).z * 20.0;
    if depth < fog.clipping.x || depth > fog.clipping.y { discard; }
    var surface: VertexOutput;
    surface.position = in.position;
    surface.world_position = in.world_position;
    surface.world_normal = in.normal;
    surface.uv = in.uv;
@if(VERTEX_COLORS)
    surface.color = vec4(1.0);
@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    surface.instance_index = in.instance_index;
    let water_weight = fog.water.x * clamp(in.water, 0.0, 1.0);
    let position = in.world_position.xz;
    let ripple = vec2(sin(dot(position, vec2(1.3, 0.7)) - globals.time * 1.7),
                      sin(dot(position, vec2(-0.6, 2.1)) - globals.time * 2.3));
    surface.uv += ripple * (0.006 * water_weight);
    var input = pbr_input_from_standard_material(surface, is_front);
    let base = alpha_discard(input.material.flags, input.material.alpha_cutoff, input.material.base_color);
    // Terrain lighting modulates encoded texture RGB before scene fog.
    var color = srgb_to_linear(linear_to_srgb(base.rgb) * in.color);
    if water_weight > 0.0 {
        let normal = normalize(input.N + vec3(ripple.x, 0.0, ripple.y) * (0.12 * water_weight));
        let to_camera = normalize(view.world_position - in.world_position.xyz);
        let sheen = pow(1.0 - clamp(dot(normal, to_camera), 0.0, 1.0), 5.0);
        let reflected_sky = blend_fog(vec3(0.0), fog.color.rgb, 255.0);
        let rippled = color * (1.0 + 0.035 * (ripple.x + ripple.y) * water_weight);
        color = mix(rippled, reflected_sky, sheen * 0.12 * water_weight);
    }
    var out: FragmentOutput;
    out.color = vec4(scene_color(color, depth, in.position.xy), base.a);
    return out;
}

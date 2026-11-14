import bevy_pbr::render::{
    forward_io::{Vertex, VertexOutput, FragmentOutput, decompress_vertex},
    mesh_functions,
    mesh_view_bindings::{view, globals},
    view_transformations::position_world_to_clip,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::apply_pbr_lighting,
};
import package::scene_shading::{SceneFog, fog, scene_fog_at_position};
import package::glide_fog::fog_table_position;
import constants::MATERIAL_BIND_GROUP;
import package::geometry_surface::deform;
struct GeometryUniform { fog: SceneFog, parameters: vec4<f32>, };
@group(MATERIAL_BIND_GROUP) @binding(50) var<uniform> geometry: GeometryUniform;
fn load_material(instance_index: u32) { fog = geometry.fog; }

struct GeometryVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
    @location(3) @interpolate(flat) instance_index: u32,
};

@vertex
fn vertex(raw: Vertex) -> GeometryVertex {
    load_material(raw.instance_index);
    let vertex = decompress_vertex(raw, raw.instance_index);
    let transform = mesh_functions::get_world_from_local(raw.instance_index);
    var out: GeometryVertex;
    var world = mesh_functions::mesh_position_local_to_world(transform, vec4(vertex.position, 1.0));
    // UVs and parameters are interpreted by the active deformation library.
    world = vec4(deform(world.xyz, vertex.uv, globals.time, geometry.parameters), 1.0);
    out.world_position = world;
    out.position = position_world_to_clip(world.xyz);
    out.normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, raw.instance_index);
@if(VERTEX_COLORS)
    out.color = vertex.color.rgb;
@if(!VERTEX_COLORS)
    out.color = vec3(1.0);
    out.instance_index = raw.instance_index;
    return out;
}

@fragment
fn fragment(in: GeometryVertex, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    load_material(in.instance_index);
    let depth = -(view.view_from_world * in.world_position).z * 20.0;
    if depth < fog.clipping.x || depth > fog.clipping.y { discard; }
    var surface: VertexOutput;
    surface.position = in.position;
    surface.world_position = in.world_position;
    surface.world_normal = in.normal;
    surface.uv = vec2(0.0);
@if(VERTEX_COLORS)
    surface.color = vec4(1.0);
@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    surface.instance_index = in.instance_index;
    var lit = pbr_input_from_standard_material(surface, is_front);
    lit.material.base_color = vec4(in.color, 1.0);
    lit.material.emissive = vec4(0.0);
    lit.material.metallic = 0.0;
    lit.material.perceptual_roughness = 1.0;
    lit.material.reflectance = vec3(0.0);
    var out: FragmentOutput;
    out.color = vec4(scene_fog_at_position(apply_pbr_lighting(lit).rgb, depth, fog_table_position(depth)), 1.0);
    return out;
}

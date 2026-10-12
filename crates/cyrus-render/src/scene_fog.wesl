import bevy_pbr::render::{
    forward_io::{Vertex, VertexOutput, FragmentOutput, decompress_vertex},
    mesh_functions,
    mesh_view_bindings::view,
    view_transformations::position_world_to_clip,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
};
@if(PREMULTIPLY_ALPHA)
import bevy_pbr::render::pbr_functions::premultiply_alpha;
import bevy_render::color_operations::{linear_to_srgb, srgb_to_linear};
import package::scene_shading::{fog, scene_color};

struct ModelVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec3<f32>,
    @location(4) @interpolate(flat) instance_index: u32,
};

@vertex
fn vertex(raw: Vertex) -> ModelVertex {
    let vertex = decompress_vertex(raw, raw.instance_index);
    let transform = mesh_functions::get_world_from_local(raw.instance_index);
    var out: ModelVertex;
    out.world_position = mesh_functions::mesh_position_local_to_world(transform, vec4(vertex.position, 1.0));
    out.position = position_world_to_clip(out.world_position.xyz);
    // The original model matrix acts directly on normals, retaining their magnitude.
    out.normal = (transform * vec4(vertex.normal, 0.0)).xyz;
    out.uv = vertex.uv;
    // shortcut: local lights and retained instance RGB await recovered admission and retention rules.
    let weight = max(0.0, -dot(fog.direction.xyz, out.normal));
    let color = clamp(fog.ambient.xyz + vec3(vertex.uv_b.x) + weight * fog.directional.xyz, vec3(0.0), vec3(255.0));
    // Cancel perspective weighting for screen-linear vertex RGB, including WebGL.
    out.color = color * (vertex.uv_b.y / 255.0) * out.position.w;
    out.instance_index = raw.instance_index;
    return out;
}

@fragment
fn fragment(in: ModelVertex, @builtin(front_facing) is_front: bool) -> FragmentOutput {
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
    let input = pbr_input_from_standard_material(surface, is_front);
    let base = alpha_discard(input.material.flags, input.material.alpha_cutoff, input.material.base_color);
    let color = srgb_to_linear(linear_to_srgb(base.rgb) * (in.color * in.position.w));
    var out: FragmentOutput;
    out.color = vec4(scene_color(color, depth, in.position.xy), base.a);
@if(PREMULTIPLY_ALPHA) {
    out.color = premultiply_alpha(input.material.flags, out.color);
}
    return out;
}

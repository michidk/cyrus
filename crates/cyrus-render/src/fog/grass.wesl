import bevy_pbr::render::{
    forward_io::{Vertex, VertexOutput, FragmentOutput, decompress_vertex},
    mesh_functions,
    mesh_view_bindings::{view, globals},
    view_transformations::position_world_to_clip,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_types::PbrInput,
    pbr_functions::apply_pbr_lighting,
};
import package::scene_shading::{SceneFog, fog, material_slot, scene_fog_at_position};
import package::glide_fog::fog_table_position;
import constants::MATERIAL_BIND_GROUP;
@if(BINDLESS)
import bevy_pbr::render::mesh_bindings::mesh;

// `grass::GrassUniform`.
struct GrassMaterial {
    fog: SceneFog,
    // x: how far the wind bends a blade's tip, in world units.
    wind: vec4<f32>,
};

@if(BINDLESS) {
// The grass extension's bindless index table, `range(50..51)`.
struct GrassBindings {
    material: u32,
};
@group(MATERIAL_BIND_GROUP) @binding(100) var<storage> grass_bindings: array<GrassBindings>;
@group(MATERIAL_BIND_GROUP) @binding(101) var<storage> grass_materials: array<GrassMaterial>;
}
@if(!BINDLESS)
@group(MATERIAL_BIND_GROUP) @binding(50) var<uniform> grass_material: GrassMaterial;

var<private> wind: vec4<f32>;

// Loads the drawn blades' fog and wind; both entry points call it first.
fn load_grass_material(instance_index: u32) {
@if(BINDLESS) {
    material_slot = mesh[instance_index].material_and_lightmap_bind_group_slot & 0xffffu;
    let material = grass_materials[grass_bindings[material_slot].material];
    fog = material.fog;
    wind = material.wind;
}
@if(!BINDLESS) {
    fog = grass_material.fog;
    wind = grass_material.wind;
}
}

struct GrassVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
    @location(3) @interpolate(flat) instance_index: u32,
};

// The wind's heading, as `foliage::WIND`.
const WIND_HEADING: vec2<f32> = vec2(0.94, 0.34);

// How far the wind has bent a blade at a point, along the heading: gusts roll across the meadow
// in waves, and each blade lags its own phase. `tip` is 0 at the base and 1 at the tip, so the
// base stays planted.
fn wind_bend(position: vec2<f32>, phase: f32, tip: f32) -> f32 {
    let tau = 6.283185307;
    let gust = 0.6 * sin(dot(position, vec2(0.11, 0.04)) - globals.time * 1.3)
        + 0.4 * sin(dot(position, vec2(-0.05, 0.23)) - globals.time * 2.1 + phase * tau);
    return wind.x * (0.6 + 0.4 * gust) * tip * tip;
}

@vertex
fn vertex(raw: Vertex) -> GrassVertex {
    load_grass_material(raw.instance_index);
    let vertex = decompress_vertex(raw, raw.instance_index);
    let transform = mesh_functions::get_world_from_local(raw.instance_index);
    var out: GrassVertex;
    var world = mesh_functions::mesh_position_local_to_world(transform, vec4(vertex.position, 1.0));
    // The blade's phase and its height along it travel in UV 0.
    let bend = wind_bend(world.xz, vertex.uv.x, vertex.uv.y);
    world = vec4(world.x + WIND_HEADING.x * bend, world.y - 0.25 * abs(bend), world.z + WIND_HEADING.y * bend, 1.0);
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
fn fragment(in: GrassVertex, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    load_grass_material(in.instance_index);
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

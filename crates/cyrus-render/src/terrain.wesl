import bevy_pbr::render::{
    forward_io::{Vertex, VertexOutput, FragmentOutput, decompress_vertex},
    mesh_functions,
    mesh_view_bindings::{view, globals},
    view_transformations::position_world_to_clip,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
};
import bevy_render::color_operations::srgb_to_linear;
import package::scene_shading::{fog, scene_color};
import package::glide_fog::blend_fog;
import package::sampling::sample_encoded;
import package::terrain_texture::terrain_texture_uv;
import bevy_pbr::render::pbr_bindings::base_color_texture;

struct TerrainVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) water: f32,
    @location(4) color: vec3<f32>,
    @location(5) @interpolate(flat) instance_index: u32,
@if(PREPARED_TERRAIN)
    @location(6) @interpolate(flat) red: vec4<i32>,
@if(PREPARED_TERRAIN)
    @location(7) @interpolate(flat) green: vec4<i32>,
@if(PREPARED_TERRAIN)
    @location(8) @interpolate(flat) blue: vec4<i32>,
@if(PREPARED_TERRAIN)
    @location(9) depth: f32,
@if(PREPARED_TERRAIN)
    @location(10) @interpolate(flat) texture0: vec4<u32>,
@if(PREPARED_TERRAIN)
    @location(11) @interpolate(flat) texture1: vec4<u32>,
@if(PREPARED_TERRAIN)
    @location(12) @interpolate(flat) texture2: vec4<u32>,
@if(PREPARED_TERRAIN)
    @location(13) @interpolate(flat) texture3: vec4<u32>,
@if(PREPARED_TERRAIN)
    @location(14) @interpolate(flat) texture4: vec4<u32>,
};

@vertex
fn vertex(raw: Vertex,
@if(PREPARED_TERRAIN)
    @location(8) projected: vec4<f32>,
@if(PREPARED_TERRAIN)
    @location(9) red: vec4<i32>,
@if(PREPARED_TERRAIN)
    @location(10) green: vec4<i32>,
@if(PREPARED_TERRAIN)
    @location(11) blue: vec4<i32>,
@if(PREPARED_TERRAIN)
    @location(4) texture0: vec4<u32>,
@if(PREPARED_TERRAIN)
    @location(12) texture1: vec4<u32>,
@if(PREPARED_TERRAIN)
    @location(13) texture2: vec4<u32>,
@if(PREPARED_TERRAIN)
    @location(14) texture3: vec4<u32>,
@if(PREPARED_TERRAIN)
    @location(15) texture4: vec4<u32>,
) -> TerrainVertex {
    let vertex = decompress_vertex(raw, raw.instance_index);
    let transform = mesh_functions::get_world_from_local(raw.instance_index);
    var out: TerrainVertex;
    out.world_position = mesh_functions::mesh_position_local_to_world(transform, vec4(vertex.position, 1.0));
    out.position = position_world_to_clip(out.world_position.xyz);
@if(PREPARED_TERRAIN) {
    out.position = view.clip_from_view * vec4(0.0, 0.0, -projected.z, 1.0);
    out.position.x = (projected.x / view.viewport.z * 2.0 - 1.0) * out.position.w;
    out.position.y = (1.0 - projected.y / view.viewport.w * 2.0) * out.position.w;
    out.red = red;
    out.green = green;
    out.blue = blue;
    out.depth = projected.z * 20.0;
    out.texture0 = texture0;
    out.texture1 = texture1;
    out.texture2 = texture2;
    out.texture3 = texture3;
    out.texture4 = texture4;
}
    out.normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, raw.instance_index);
    out.uv = vertex.uv;
@if(VERTEX_UVS_B)
    out.water = vertex.uv_b.x;
@if(VERTEX_COLORS)
    // WebGL lacks affine varyings; cancel perspective weighting with clip W.
    out.color = vertex.color.rgb * out.position.w;
    out.instance_index = raw.instance_index;
    return out;
}


@if(PREPARED_TERRAIN)
fn terrain_channel(plane: vec4<i32>, offset: vec2<i32>) -> f32 {
    let value = plane.x + offset.x * plane.y + offset.y * plane.z;
    let channel = (value >> 12u) & 4095;
    return f32(select(select(channel & 255, 255, channel == 256), 0, channel == 4095));
}

@fragment
fn fragment(in: TerrainVertex, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var depth = -(view.view_from_world * in.world_position).z * 20.0;
@if(PREPARED_TERRAIN) { depth = in.depth; }
@if(!PREPARED_TERRAIN) {
    if depth < fog.clipping.x || depth > fog.clipping.y { discard; }
}
    var surface: VertexOutput;
    surface.position = in.position;
    surface.world_position = in.world_position;
    surface.world_normal = in.normal;
    surface.uv = in.uv;
@if(PREPARED_TERRAIN) {
    let pixel = vec2<i32>(floor(in.position.xy - view.viewport.xy)) - vec2(in.red.w, in.green.w);
    surface.uv = terrain_texture_uv(in.texture0, in.texture1, in.texture2, in.texture3, in.texture4, pixel, textureDimensions(base_color_texture));
}
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
    var encoded: vec3<f32>;
@if(!PREPARED_TERRAIN)
    encoded = sample_encoded(surface.uv, false) * (in.color * in.position.w);
@if(PREPARED_TERRAIN) {
    let offset = vec2<i32>(floor(in.position.xy - view.viewport.xy)) - vec2(in.red.w, in.green.w);
    let light = vec3(terrain_channel(in.red, offset), terrain_channel(in.green, offset), terrain_channel(in.blue, offset));
    encoded = floor(round(sample_encoded(surface.uv, false) * 255.0) * (light + vec3(1.0)) / 256.0) / 255.0;
}
    var color = srgb_to_linear(encoded);
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

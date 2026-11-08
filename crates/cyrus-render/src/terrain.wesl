import bevy_pbr::render::{
    forward_io::{Vertex, VertexOutput, FragmentOutput, decompress_vertex},
    mesh_functions,
    mesh_view_bindings::{view, globals},
    view_transformations::position_world_to_clip,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
    pbr_types::PbrInput,
};
@if(MODERN_LIGHTING)
import bevy_pbr::render::pbr_functions::apply_pbr_lighting;
import bevy_render::color_operations::srgb_to_linear;
import package::scene_shading::{fog, point_sampled, scene_color_at_fog_position, scene_fog_at_position};
import package::glide_fog::{blend_fog, fog_table_position, fog_table_position_from_reciprocal};
import package::sampling::{sample_encoded, color_channel};
import package::terrain_texture::{terrain_texture_uv, texture_plane};
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

// The modern waves of `water::modern_displacement` at a grid position: the height above the base
// grid and its slope along x and z. `fog.water.z` is the amplitude, `fog.water.w` the cell size.
fn modern_wave(position: vec2<f32>, seconds: f32) -> vec3<f32> {
    let tau = 6.283185307;
    let amplitude = fog.water.z;
    let along = tau / (fog.water.w * 6.0);
    let across = tau / (fog.water.w * 2.5);
    let phase = (position.x + position.y * 0.35) * along - seconds * 0.8;
    let cross = (position.y - position.x * 0.6) * across - seconds * 1.3;
    let height = amplitude * (0.5 + 0.35 * sin(phase) + 0.15 * sin(cross));
    let slope_x = amplitude * (0.35 * cos(phase) * along - 0.15 * cos(cross) * 0.6 * across);
    let slope_z = amplitude * (0.35 * cos(phase) * 0.35 * along + 0.15 * cos(cross) * across);
    return vec3(height, slope_x, slope_z);
}

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
    var position = vertex.position;
    var normal = vertex.normal;
@if(VERTEX_UVS_B) {
    if fog.water.w > 0.0 && vertex.uv_b.x > 0.0 {
        let wave = modern_wave(position.xz, globals.time);
        position.y += wave.x;
        normal = normalize(vec3(-wave.y, 1.0, -wave.z));
    }
}
    out.world_position = mesh_functions::mesh_position_local_to_world(transform, vec4(position, 1.0));
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
    out.normal = mesh_functions::mesh_normal_local_to_world(normal, raw.instance_index);
    out.uv = vertex.uv;
@if(VERTEX_UVS_B)
    out.water = vertex.uv_b.x;
@if(VERTEX_COLORS)
    // WebGL lacks affine varyings; cancel perspective weighting with clip W.
    out.color = vertex.color.rgb * out.position.w;
    out.instance_index = raw.instance_index;
    return out;
}


// The terrain lit by Bevy's lights as a matte material; water, weighted by `water`, is glossy
// with its ripples in `normal`, a Modern tuning choice.
@if(MODERN_LIGHTING)
fn modern_lit(input: PbrInput, albedo: vec3<f32>, water: f32, normal: vec3<f32>) -> vec3<f32> {
    var lit = input;
    lit.material.base_color = vec4(albedo, 1.0);
    lit.material.emissive = vec4(0.0);
    lit.material.metallic = 0.0;
    lit.material.perceptual_roughness = mix(1.0, 0.12, water);
    lit.material.reflectance = vec3(0.5 * water);
    lit.N = normalize(mix(lit.N, normal, water));
    return apply_pbr_lighting(lit).rgb;
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
    var fog_position = fog_table_position(depth);
@if(PREPARED_TERRAIN) {
    let pixel = vec2<i32>(floor(in.position.xy - view.viewport.xy)) - vec2(in.red.w, in.green.w);
    surface.uv = terrain_texture_uv(in.texture0, in.texture1, in.texture2, in.texture3, in.texture4, pixel, textureDimensions(base_color_texture));
    let reciprocal = texture_plane(in.texture3.xy, in.texture3.zw, in.texture4.xy, pixel);
    fog_position = fog_table_position_from_reciprocal(reciprocal.x);
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
    encoded = sample_encoded(surface.uv, point_sampled(), true) * (in.color * in.position.w);
@if(PREPARED_TERRAIN) {
    let offset = vec2<i32>(floor(in.position.xy - view.viewport.xy)) - vec2(in.red.w, in.green.w);
    let light = vec3(color_channel(in.red, offset), color_channel(in.green, offset), color_channel(in.blue, offset));
    encoded = floor(round(sample_encoded(surface.uv, point_sampled(), true) * 255.0) * (light + vec3(1.0)) / 256.0) / 255.0;
}
    var color = srgb_to_linear(encoded);
@if(MODERN_LIGHTING)
    color = modern_lit(input, srgb_to_linear(sample_encoded(surface.uv, point_sampled(), true)), clamp(in.water, 0.0, 1.0),
        normalize(input.N + vec3(ripple.x, 0.0, ripple.y) * 0.05));
    if water_weight > 0.0 {
        let normal = normalize(input.N + vec3(ripple.x, 0.0, ripple.y) * (0.12 * water_weight));
        let to_camera = normalize(view.world_position - in.world_position.xyz);
        let sheen = pow(1.0 - clamp(dot(normal, to_camera), 0.0, 1.0), 5.0);
        let reflected_sky = blend_fog(vec3(0.0), fog.color.rgb, 255.0);
        let rippled = color * (1.0 + 0.035 * (ripple.x + ripple.y) * water_weight);
        color = mix(rippled, reflected_sky, sheen * 0.12 * water_weight);
    }
    var out: FragmentOutput;
    out.color = vec4(scene_color_at_fog_position(color, depth, in.position.xy, fog_position), base.a);
    // Modern lighting keeps the scene fog but not the original's 16-bit framebuffer.
    // The water weight in the alpha marks reflecting pixels for the modern screen pass, which
    // restores an opaque alpha.
@if(MODERN_LIGHTING)
    out.color = vec4(scene_fog_at_position(color, depth, fog_position), 1.0 - 0.5 * clamp(in.water, 0.0, 1.0));
    return out;
}

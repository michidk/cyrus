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
import bevy_render::color_operations::{srgb_to_linear, linear_to_srgb};
import package::scene_shading::{SceneFog, fog, material_slot, point_sampled, scene_fog_at_position};
import package::glide_fog::{blend_fog, fog_table_position, fog_table_position_from_reciprocal};
import package::sampling::{base_color_level, base_color_size, sample_encoded, color_channel};
import package::terrain_texture::{terrain_texture_uv, texture_plane};
import package::water_surface::{water_wave, water_ripple, shore_foam, shore_wetness};
import constants::MATERIAL_BIND_GROUP;
@if(BINDLESS)
import bevy_pbr::render::mesh_bindings::mesh;

@if(BINDLESS) {
// The terrain extension's bindless index table, `range(50..51)`.
struct TerrainBindings {
    material: u32,
};
@group(MATERIAL_BIND_GROUP) @binding(100) var<storage> terrain_bindings: array<TerrainBindings>;
@group(MATERIAL_BIND_GROUP) @binding(101) var<storage> terrain_materials: array<SceneFog>;
}
@if(!BINDLESS)
@group(MATERIAL_BIND_GROUP) @binding(50) var<uniform> terrain_material: SceneFog;

// Loads the drawn terrain's fog; both entry points call it first.
fn load_terrain_material(instance_index: u32) {
@if(BINDLESS) {
    material_slot = mesh[instance_index].material_and_lightmap_bind_group_slot & 0xffffu;
    fog = terrain_materials[terrain_bindings[material_slot].material];
}
@if(!BINDLESS) {
    fog = terrain_material;
}
}

struct TerrainVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    // x: the water weight; y: the distance to the shore in grid cells, positive on water.
    @location(3) water: vec2<f32>,
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
    load_terrain_material(raw.instance_index);
    let vertex = decompress_vertex(raw, raw.instance_index);
    let transform = mesh_functions::get_world_from_local(raw.instance_index);
    var out: TerrainVertex;
    var position = vertex.position;
    var normal = vertex.normal;
@if(VERTEX_UVS_B) {
    // The modern waves, `fog.water.z` high on a grid of `fog.water.w` cells, calming at the shore.
    if fog.water.w > 0.0 && vertex.uv_b.x > 0.0 {
        let calm = 1.0 - smoothstep(0.5, 3.0, shore_distance(vertex.uv_b.y));
        let wave = water_wave(position.xz, globals.time, fog.water.z, fog.water.w, calm);
        position.y += wave.x;
        // Far off, the waves' tilt would band the water's shine into regular stripes.
        let world = mesh_functions::mesh_position_local_to_world(transform, vec4(position, 1.0)).xyz;
        let fade = 1.0 - smoothstep(60.0, 260.0, length(world - view.world_position));
        normal = normalize(vec3(-wave.y * fade, 1.0, -wave.z * fade));
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
    // Without a water channel the terrain is land, far from any shore.
    out.water = vec2(0.0, -4.0);
@if(VERTEX_UVS_B)
    out.water = vertex.uv_b;
@if(VERTEX_COLORS)
    // WebGL lacks affine varyings; cancel perspective weighting with clip W.
    out.color = vertex.color.rgb * out.position.w;
    out.instance_index = raw.instance_index;
    return out;
}


// How much the terrain texel at `uv`, kept inside the cell's tile, looks like water, 0..1: water
// texels are bluer than they are red, sand and rock are not. A Modern tuning choice that finds the
// waterline the beach tiles draw.
fn texel_water(uv: vec2<f32>) -> f32 {
    let inside = vec2(0.5) / vec2<f32>(base_color_size());
    let rgb = linear_to_srgb(base_color_level(clamp(uv, inside, vec2(1.0) - inside)).rgb);
    return smoothstep(0.02, 0.12, rgb.b - rgb.r);
}

// Roughly how many texels `uv` lies from land in the same tile, from rings of eight taps out to
// eight texels; `LAND_FAR` where no ring finds any.
const LAND_FAR: f32 = 10.0;
fn land_texels(uv: vec2<f32>) -> f32 {
    let texel = 1.0 / vec2<f32>(base_color_size());
    var inner = 0.0;
    for (var ring = 0u; ring < 4u; ring++) {
        let radius = 1.5 * f32(ring + 1u) + 0.5 * f32(ring * ring);
        var land = 0.0;
        for (var i = 0u; i < 8u; i++) {
            let angle = (f32(i) + 0.5 * f32(ring)) * 0.7853982;
            land = max(land, 1.0 - texel_water(uv + vec2(cos(angle), sin(angle)) * radius * texel));
        }
        if land > 0.5 { return mix(inner, radius, 1.5 - land); }
        inner = radius;
    }
    return LAND_FAR;
}

// The terrain lit by Bevy's lights as a matte material; water, weighted by `water`, is glossy
// with its ripples in `normal`, and wet sand, weighted by `wet`, a little glossy; Modern tuning
// choices.
@if(MODERN_LIGHTING)
fn modern_lit(input: PbrInput, albedo: vec3<f32>, water: f32, wet: f32, normal: vec3<f32>) -> vec3<f32> {
    var lit = input;
    lit.material.base_color = vec4(albedo, 1.0);
    lit.material.emissive = vec4(0.0);
    lit.material.metallic = 0.0;
    lit.material.perceptual_roughness = mix(mix(1.0, 0.45, wet), 0.12, water);
    lit.material.reflectance = vec3(0.5 * water);
    lit.N = normalize(mix(lit.N, normal, water));
    return apply_pbr_lighting(lit).rgb;
}

@fragment
fn fragment(in: TerrainVertex, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    load_terrain_material(in.instance_index);
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
    surface.uv = terrain_texture_uv(in.texture0, in.texture1, in.texture2, in.texture3, in.texture4, pixel, base_color_size());
    let reciprocal = texture_plane(in.texture3.xy, in.texture3.zw, in.texture4.xy, pixel);
    fog_position = fog_table_position_from_reciprocal(reciprocal.x);
}
@if(VERTEX_COLORS)
    surface.color = vec4(1.0);
@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    surface.instance_index = in.instance_index;
    let position = in.world_position.xz;
    // Modern water (`fog.water.x`): foam where the sea meets the shore, which neither shines nor
    // reflects, and wet sand on the beach behind it.
    let shore = shore_distance(in.water.y);
    // Away from the shore: the wave vertices, and the still water the water textures draw.
    var surface_water = max(clamp(in.water.x, 0.0, 1.0), smoothstep(-0.15, 0.35, shore));
    // How far, in texels, this water lies from the drawn waterline.
    var land = LAND_FAR;
    if fog.water.x > 0.0 && shore > -1.0 && shore < 1.0 {
        // Near it, the texels themselves: the beach tiles draw the waterline across the cell.
        surface_water = texel_water(surface.uv);
        // Where a water cell meets a land cell, the cell border is the waterline.
        let border = land_border_distance(in.uv, land_border_mask(in.water.y));
        land = min(land_texels(surface.uv), border * f32(base_color_size().x));
    }
    let foam = fog.water.x * surface_water * shore_foam(position, land, shore, globals.time);
    let water_mask = surface_water * (1.0 - foam);
    let wet = fog.water.x * shore_wetness(shore, position, globals.time) * (1.0 - surface_water);
    let water_weight = fog.water.x * water_mask;
    let ripple = water_ripple(position, globals.time, length(view.world_position - in.world_position.xyz));
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
@if(MODERN_LIGHTING) {
    // Wet sand is darker; foam is near white.
    let foam_color = vec3(0.80, 0.86, 0.88);
    let albedo = srgb_to_linear(sample_encoded(surface.uv, point_sampled(), true));
    color = modern_lit(input, mix(albedo * (1.0 - 0.45 * wet), foam_color, foam), water_mask, wet,
        normalize(input.N + vec3(ripple.x, 0.0, ripple.y) * 0.05));
}
    if water_weight > 0.0 {
        let normal = normalize(input.N + vec3(ripple.x, 0.0, ripple.y) * (0.12 * water_weight));
        let to_camera = normalize(view.world_position - in.world_position.xyz);
        let sheen = pow(1.0 - clamp(dot(normal, to_camera), 0.0, 1.0), 5.0);
        let reflected_sky = blend_fog(vec3(0.0), fog.color.rgb, 255.0);
        let rippled = color * (1.0 + 0.035 * (ripple.x + ripple.y) * water_weight);
        color = mix(rippled, reflected_sky, sheen * 0.12 * water_weight);
    }
    var out: FragmentOutput;
    out.color = vec4(scene_fog_at_position(color, depth, fog_position), base.a);
    // The water weight in the alpha marks reflecting pixels for the modern screen pass, which
    // restores an opaque alpha.
@if(MODERN_LIGHTING)
    out.color = vec4(scene_fog_at_position(color, depth, fog_position), 1.0 - 0.5 * water_mask);
    return out;
}

// The terrain water channel's y packs the grid shore distance (within ±4.5) with ten times the
// cell's land-border mask, which is the same at every vertex of the cell (`Shore::channel`).
fn shore_distance(channel: f32) -> f32 {
    return channel - 10.0 * floor((channel + 5.0) / 10.0);
}

fn land_border_mask(channel: f32) -> u32 {
    return u32(max(floor((channel + 5.0) / 10.0), 0.0));
}

// Distance from `uv` to the nearest edge or corner of the cell's tile that borders a land cell, in
// tiles; large where none does. Bits 0 to 3: edges u = 0, u = 1, v = 0, v = 1; 4 to 7: corners
// (0, 0), (1, 0), (0, 1), (1, 1).
fn land_border_distance(uv: vec2<f32>, mask: u32) -> f32 {
    let p = clamp(uv, vec2(0.0), vec2(1.0));
    var d = 10.0;
    if (mask & 1u) != 0u { d = min(d, p.x); }
    if (mask & 2u) != 0u { d = min(d, 1.0 - p.x); }
    if (mask & 4u) != 0u { d = min(d, p.y); }
    if (mask & 8u) != 0u { d = min(d, 1.0 - p.y); }
    if (mask & 16u) != 0u { d = min(d, length(p)); }
    if (mask & 32u) != 0u { d = min(d, length(p - vec2(1.0, 0.0))); }
    if (mask & 64u) != 0u { d = min(d, length(p - vec2(0.0, 1.0))); }
    if (mask & 128u) != 0u { d = min(d, length(p - vec2(1.0))); }
    return d;
}

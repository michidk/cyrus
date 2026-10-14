import bevy_pbr::render::{
    forward_io::{Vertex, VertexOutput, FragmentOutput, decompress_vertex},
    mesh_functions,
    mesh_view_bindings::view,
    view_transformations::position_world_to_clip,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
    pbr_types::STANDARD_MATERIAL_FLAGS_BASE_COLOR_TEXTURE_BIT,
};
@if(PREMULTIPLY_ALPHA)
import bevy_pbr::render::pbr_functions::premultiply_alpha;
import bevy_render::color_operations::{linear_to_srgb, srgb_to_linear};
import package::scene_shading::{fog, scene_color, scene_fog_at_position};
import package::glide_fog::{fog_table_position, framebuffer_rgb565, framebuffer_dither, framebuffer_blend_integer};
@if(VIEW_TRANSMISSION_TEXTURE)
import bevy_pbr::render::mesh_view_bindings::view_transmission_texture;
import package::model_lights::{apply_local_lights, model, compositing};
import package::sampling::{sample_encoded, matches_chroma};
import constants::MATERIAL_BIND_GROUP;
@group(MATERIAL_BIND_GROUP) @binding(104) var<uniform> chroma_key: vec4<f32>;


struct ModelVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec3<f32>,
    @location(4) @interpolate(flat) instance_index: u32,
    @location(5) @interpolate(flat) retained_solid: u32,
};

@vertex
fn vertex(raw: Vertex,
@if(PREPARED_MODEL)
    @location(8) projected: vec4<f32>,
) -> ModelVertex {
    let vertex = decompress_vertex(raw, raw.instance_index);
    let transform = mesh_functions::get_world_from_local(raw.instance_index);
    var out: ModelVertex;
    out.world_position = mesh_functions::mesh_position_local_to_world(transform, vec4(vertex.position, 1.0));
    out.position = position_world_to_clip(out.world_position.xyz);
@if(PREPARED_MODEL) {
    out.position = view.clip_from_view * vec4(0.0, 0.0, -projected.z, 1.0);
    out.position.x = (projected.x / view.viewport.z * 2.0 - 1.0) * out.position.w;
    out.position.y = (1.0 - projected.y / view.viewport.w * 2.0) * out.position.w;
}
    // The original model matrix acts directly on normals, retaining their magnitude.
    out.normal = (transform * vec4(vertex.normal, 0.0)).xyz;
    out.uv = vertex.uv;
    let weight = max(0.0, -dot(fog.direction.xyz, out.normal));
    var color = fog.ambient.xyz + vec3(vertex.uv_b.x) + weight * fog.directional.xyz;
    var tint = vertex.uv_b.y;
    out.retained_solid = 0u;
@if(VERTEX_COLORS) {
    color = vertex.color.rgb * 255.0;
    tint = 1.0;
    out.retained_solid = u32(vertex.color.a);
}
    // Prepared model RGB has already received source-corner lighting and clipping.
    if vertex.uv_b.x >= 0.0 {
        color = apply_local_lights(color, vertex.position, vertex.normal, transform);
    }
    // Cancel perspective weighting for screen-linear vertex RGB, including WebGL.
    out.color = clamp(color, vec3(0.0), vec3(255.0)) * (tint / 255.0) * out.position.w;
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
    var base = input.material.base_color;
    if model.parameters.w < 0.0 {
        base = alpha_discard(input.material.flags, input.material.alpha_cutoff, base);
    } else {
        base.a = 1.0;
    }
    var texture_rgb = linear_to_srgb(base.rgb);
    if model.parameters.w >= 0.0 && (input.material.flags & STANDARD_MATERIAL_FLAGS_BASE_COLOR_TEXTURE_BIT) != 0u {
        texture_rgb = sample_encoded(surface.uv, (u32(model.parameters.w) & 1u) != 0u);
    }
    if chroma_key.w != 0.0 && in.retained_solid == 0u && matches_chroma(texture_rgb, chroma_key.rgb) {
        discard;
    }
    texture_rgb = select(texture_rgb, vec3(1.0), in.retained_solid != 0u);
    let color = srgb_to_linear(texture_rgb * (in.color * in.position.w));
    var out: FragmentOutput;
    out.color = vec4(scene_color(color, depth, in.position.xy), base.a);
@if(VIEW_TRANSMISSION_TEXTURE) {
    if compositing.x >= 0.0 {
        var result = scene_fog_at_position(color, depth, fog_table_position(depth));
        if compositing.x < 254.0 {
            let destination = textureLoad(view_transmission_texture, vec2<i32>(in.position.xy), 0).rgb;
            result = framebuffer_blend_integer(destination, linear_to_srgb(result), u32(compositing.x) & 255u, framebuffer_dither(in.position.xy));
        }
        if fog.clipping.z == 0.0 { result = framebuffer_rgb565(result, in.position.xy); }
        out.color = vec4(result, 1.0);
    }
}
@if(PREMULTIPLY_ALPHA) {
    out.color = premultiply_alpha(input.material.flags, out.color);
}
    return out;
}

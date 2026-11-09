// The camera prepass of scene models under modern lighting: Bevy's standard prepass, but
// chroma-keyed texels are cut out as in the main pass. Without the cut, a keyed model such as a palm
// frond would either cover its whole mesh in the prepass or, skipped, leave the depth and normals of
// what lies behind it, which ambient occlusion and contact shadows then darken it with.
import bevy_pbr::{
    render::{pbr_bindings, pbr_functions, pbr_prepass_functions, pbr_types, mesh_bindings::mesh},
    prepass::io as prepass_io,
};
@if(BINDLESS)
import bevy_pbr::render::pbr_bindings::material_indices;
import package::model_lights::{load_model_material, model};
import package::sampling::{sample_encoded, matches_chroma};
import package::scene_shading::point_sampled;

fn material_flags(instance_index: u32) -> u32 {
@if(BINDLESS) {
    let slot = mesh[instance_index].material_and_lightmap_bind_group_slot & 0xffffu;
    return pbr_bindings::material_array[material_indices[slot].material].flags;
}
@if(!BINDLESS) {
    return pbr_bindings::material.flags;
}
}

// Discards what the main pass discards: alpha-masked texels, and texels matching the chroma key.
fn discard_cut_out(in: prepass_io::VertexOutput, flags: u32) {
    pbr_prepass_functions::prepass_sample_color_and_alpha_discard(in);
    load_model_material(in.instance_index);
@if(VERTEX_UVS) {
    let textured = (flags & pbr_types::STANDARD_MATERIAL_FLAGS_BASE_COLOR_TEXTURE_BIT) != 0u;
    if model.chroma_key.w != 0.0 && model.parameters.w >= 0.0 && textured {
        let rgb = sample_encoded(in.uv, point_sampled() || (u32(model.parameters.w) & 1u) != 0u, false);
        if matches_chroma(rgb, model.chroma_key.rgb) { discard; }
    }
}
}

@if(PREPASS_FRAGMENT)
@fragment
fn fragment(in: prepass_io::VertexOutput, @builtin(front_facing) is_front: bool) -> prepass_io::FragmentOutput {
    let flags = material_flags(in.instance_index);
    discard_cut_out(in, flags);
    var out: prepass_io::FragmentOutput;
@if(UNCLIPPED_DEPTH_ORTHO_EMULATION)
    out.frag_depth = in.unclipped_depth;
@if(NORMAL_PREPASS) {
    let double_sided = (flags & pbr_types::STANDARD_MATERIAL_FLAGS_DOUBLE_SIDED_BIT) != 0u;
    let front = pbr_functions::winding_corrected_front_facing(mesh[in.instance_index].flags, is_front);
    let normal = pbr_functions::prepare_world_normal(in.world_normal, double_sided, front);
    out.normal = vec4(normal * 0.5 + vec3(0.5), 1.0);
}
@if(MOTION_VECTOR_PREPASS)
    out.motion_vector = pbr_prepass_functions::calculate_motion_vector(in.world_position, in.previous_world_position);
    return out;
}
@else
@fragment
fn fragment(in: prepass_io::VertexOutput) {
    discard_cut_out(in, material_flags(in.instance_index));
}

import constants::MATERIAL_BIND_GROUP;
import package::scene_shading::{SceneFog, fog, material_slot};
@if(BINDLESS)
import bevy_render::bindless::bindless_textures_2d;
@if(BINDLESS)
import bevy_pbr::render::mesh_bindings::mesh;

struct ModelParameters {
    placement: mat4x4<f32>,
    parameters: vec4<f32>,
    solid_color: vec4<f32>,
    compositing: vec4<f32>,
    chroma_key: vec4<f32>,
    texture_layout: vec4<u32>,
    texture_scale: vec4<f32>,
    // x: 1 on calm water.
    surface: vec4<f32>,
};

// `fog::ModelUniform`.
struct ModelMaterial {
    fog: SceneFog,
    model: ModelParameters,
};

@if(BINDLESS) {
// The model extension's bindless index table, `range(50..52)`.
struct ModelBindings {
    material: u32,
    lights: u32,
};
@group(MATERIAL_BIND_GROUP) @binding(100) var<storage> model_bindings: array<ModelBindings>;
@group(MATERIAL_BIND_GROUP) @binding(101) var<storage> model_materials: array<ModelMaterial>;
}
@if(!BINDLESS) {
@group(MATERIAL_BIND_GROUP) @binding(50) var<uniform> model_material: ModelMaterial;
@group(MATERIAL_BIND_GROUP) @binding(51) var lights: texture_2d<f32>;
}

var<private> model: ModelParameters;

// Loads the drawn model's material into `fog` and `model`; both entry points call it first.
fn load_model_material(instance_index: u32) {
@if(BINDLESS) {
    material_slot = mesh[instance_index].material_and_lightmap_bind_group_slot & 0xffffu;
    let material = model_materials[model_bindings[material_slot].material];
    fog = material.fog;
    model = material.model;
}
@if(!BINDLESS) {
    fog = model_material.fog;
    model = model_material.model;
}
}

fn light_rows() -> i32 {
@if(BINDLESS) {
    return i32(textureDimensions(bindless_textures_2d[model_bindings[material_slot].lights]).y);
}
@if(!BINDLESS) {
    return i32(textureDimensions(lights).y);
}
}

fn light_byte_texel(pixel: vec2<i32>) -> vec4<f32> {
@if(BINDLESS) {
    return textureLoad(bindless_textures_2d[model_bindings[material_slot].lights], pixel, 0);
}
@if(!BINDLESS) {
    return textureLoad(lights, pixel, 0);
}
}

// One of the light table's two `vec4` columns. The table stores each 32-bit float as the four
// bytes of one RGBA8 texel (`lighting::runtime`), which binding arrays accept where a 32-bit float
// texture is not filterable; packing the normalised bytes back restores the exact bits.
fn light_texel(column: i32, row: i32) -> vec4<f32> {
    var value: vec4<f32>;
    for (var component = 0; component < 4; component += 1) {
        value[component] = bitcast<f32>(pack4x8unorm(light_byte_texel(vec2(column * 4 + component, row))));
    }
    return value;
}

fn ordered_dot(a: vec3<f32>, b: vec3<f32>) -> f32 {
    return (a.x * b.x + a.y * b.y) + a.z * b.z;
}

fn apply_local_lights(initial: vec3<f32>, position: vec3<f32>, normal: vec3<f32>, transform: mat4x4<f32>) -> vec3<f32> {
    if model.parameters.z == 0.0 { return initial; }
    let axes = vec3(-1.0, -1.0, 1.0);
    // Billboard-local X is camera right, Y is up and Z points toward the camera.
    let local_axes = select(axes, vec3(1.0, -1.0, -1.0), model.compositing.x >= 0.0);
    let vertex = position * local_axes * 20.0;
    let supplied_normal = normal * local_axes;
    var origin = transform[3].xyz * axes * 20.0 + vec3(0.0, 0.0, 65535.99609375);
    var basis = mat3x3(transform[0].xyz * axes * local_axes.x, transform[1].xyz * axes * local_axes.y, transform[2].xyz * axes * local_axes.z);
    if model.parameters.y != 0.0 {
        origin = model.placement[3].xyz;
        basis = mat3x3(model.placement[0].xyz, model.placement[1].xyz, model.placement[2].xyz);
    }
    var result = initial;
    let count = light_rows();
    for (var index = 256; index < count; index += 1) {
        let light = light_texel(0, index);
        let relative = light.xyz - origin;
        if ordered_dot(relative, relative) - model.parameters.x * model.parameters.x > light.w * light.w { continue; }
        let delta = vertex - transpose(basis) * relative;
        let squared = ordered_dot(delta, delta);
        if squared == 0.0 { continue; }
        let bits = bitcast<u32>(squared);
        let exponent = i32((bits >> 23u) & 255u) - 127;
        let table_index = i32((bits >> 16u) & 127u) + 128 * (exponent & 1);
        let roots = light_texel(0, table_index);
        let fraction = bitcast<u32>(roots.y) & 8323072u;
        let bias = select(253, 254, fraction == 0u);
        let inverse = bitcast<f32>(fraction | (u32((bias - exponent) / 2) << 23u));
        let normal_dot = ordered_dot(delta * inverse, supplied_normal);
        if normal_dot >= 0.0 { continue; }
        let distance = bitcast<f32>((bitcast<u32>(roots.x) & 8323072u) | (u32((exponent >> 1) + 127) << 23u));
        let strength = light_texel(1, index);
        let attenuation = strength.w / ((2.0 * distance + squared) + distance);
        if attenuation >= 0.0 { result += strength.rgb * (-normal_dot * attenuation); }
    }
    return result;
}

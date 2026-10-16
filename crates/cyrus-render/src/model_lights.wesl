import constants::MATERIAL_BIND_GROUP;

@group(MATERIAL_BIND_GROUP) @binding(101) var lights: texture_2d<f32>;
struct ModelParameters {
    placement: mat4x4<f32>,
    parameters: vec4<f32>,
    solid_color: vec4<f32>,
    compositing: vec4<f32>,
    chroma_key: vec4<f32>,
    texture_layout: vec4<u32>,
    texture_scale: vec4<f32>,
};
@group(MATERIAL_BIND_GROUP) @binding(102) var<uniform> model: ModelParameters;

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
    let count = i32(textureDimensions(lights).y);
    for (var index = 256; index < count; index += 1) {
        let light = textureLoad(lights, vec2(0, index), 0);
        let relative = light.xyz - origin;
        if ordered_dot(relative, relative) - model.parameters.x * model.parameters.x > light.w * light.w { continue; }
        let delta = vertex - transpose(basis) * relative;
        let squared = ordered_dot(delta, delta);
        if squared == 0.0 { continue; }
        let bits = bitcast<u32>(squared);
        let exponent = i32((bits >> 23u) & 255u) - 127;
        let table_index = i32((bits >> 16u) & 127u) + 128 * (exponent & 1);
        let roots = textureLoad(lights, vec2(0, table_index), 0);
        let fraction = bitcast<u32>(roots.y) & 8323072u;
        let bias = select(253, 254, fraction == 0u);
        let inverse = bitcast<f32>(fraction | (u32((bias - exponent) / 2) << 23u));
        let normal_dot = ordered_dot(delta * inverse, supplied_normal);
        if normal_dot >= 0.0 { continue; }
        let distance = bitcast<f32>((bitcast<u32>(roots.x) & 8323072u) | (u32((exponent >> 1) + 127) << 23u));
        let strength = textureLoad(lights, vec2(1, index), 0);
        let attenuation = strength.w / ((2.0 * distance + squared) + distance);
        if attenuation >= 0.0 { result += strength.rgb * (-normal_dot * attenuation); }
    }
    return result;
}

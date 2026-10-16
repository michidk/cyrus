// Water surface hook for terrain shading. Mods replace this whole library; the
// built-in version keeps the original look, where water is plain terrain.
// `world_position` and `camera` are exported scene units, `time` is seconds,
// `weight` is 1 on water vertices and 0 elsewhere, interpolated across faces.

// Offset added to the terrain texture coordinates before sampling.
fn water_uv_offset(world_position: vec3<f32>, time: f32, weight: f32) -> vec2<f32> {
    return vec2(0.0);
}

// Final linear colour of a water fragment before scene fog. `sky` is the fog colour.
fn water_color(color: vec3<f32>, world_position: vec3<f32>, normal: vec3<f32>, camera: vec3<f32>, sky: vec3<f32>, time: f32, weight: f32) -> vec3<f32> {
    return color;
}

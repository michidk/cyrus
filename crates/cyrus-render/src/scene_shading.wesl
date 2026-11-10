import package::glide_fog::{fog_table_position, table_density, blend_fog, blend_fog_integer};

struct SceneFog {
    color: vec4<f32>,
    clipping: vec4<f32>,
    table: array<vec4<f32>, 16>,
    water: vec4<f32>,
    ambient: vec4<f32>,
    directional: vec4<f32>,
    direction: vec4<f32>,
    lighting: vec4<f32>,
};

// World textures are point sampled while `Area::Textures` is Modern.
fn point_sampled() -> bool {
    return fog.lighting.y != 0.0;
}
// The drawn material's fog, which the entry point loads first: `model_lights::load_model_material`
// for models, `load_terrain_material` for terrain.
var<private> fog: SceneFog;
// The drawn mesh's slot in the bindless material tables; unused without bindless materials.
var<private> material_slot: u32;

fn scene_color(color: vec3<f32>, depth: f32) -> vec3<f32> {
    return scene_fog_at_position(color, depth, fog_table_position(depth));
}

// Higher values keep the near scene clearer and thicken the haze closer to the far clip; a Modern
// tuning choice.
const MODERN_FOG_STEEPNESS: f32 = 4.0;

fn scene_fog_at_position(color: vec3<f32>, depth: f32, position: vec2<u32>) -> vec3<f32> {
    if fog.clipping.z != 0.0 {
        // Modern fog: an exponential-squared haze normalised to be opaque at the far clip, so distant
        // scenery fades into the sky colour instead of being cut off.
        let t = clamp(depth / fog.clipping.y, 0.0, 1.0);
        let density = 255.0 * (1.0 - exp(-MODERN_FOG_STEEPNESS * t * t)) / (1.0 - exp(-MODERN_FOG_STEEPNESS));
        return blend_fog(color, fog.color.rgb, density);
    }
    let next = min(position.x + 1u, 63u);
    let density = table_density(fog.table[position.x / 4u][position.x % 4u], fog.table[next / 4u][next % 4u], position.y);
    return blend_fog_integer(color, fog.color.rgb, density);
}

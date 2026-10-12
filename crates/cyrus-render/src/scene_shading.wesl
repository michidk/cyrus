import constants::MATERIAL_BIND_GROUP;
import package::glide_fog::{fog_table_position, table_density, blend_fog, blend_fog_integer, framebuffer_rgb565};

struct SceneFog {
    color: vec4<f32>,
    clipping: vec4<f32>,
    table: array<vec4<f32>, 16>,
    water: vec4<f32>,
};
@group(MATERIAL_BIND_GROUP) @binding(100) var<uniform> fog: SceneFog;

fn scene_color(color: vec3<f32>, depth: f32, pixel: vec2<f32>) -> vec3<f32> {
    if fog.clipping.z != 0.0 {
        let density = 255.0 * smoothstep(fog.clipping.y * 0.45, fog.clipping.y, depth);
        return blend_fog(color, fog.color.rgb, density);
    }
    let position = fog_table_position(depth);
    let next = min(position.x + 1u, 63u);
    let density = table_density(fog.table[position.x / 4u][position.x % 4u], fog.table[next / 4u][next % 4u], position.y);
    return framebuffer_rgb565(blend_fog_integer(color, fog.color.rgb, density), pixel);
}

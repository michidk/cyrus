// The scene fog over the traced image; see `haze.rs`. The same lookups as `scene_shading`'s
// `scene_fog_at_position`, without the original's 16-bit framebuffer.
import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;
import bevy_render::view::View;
import package::glide_fog::{fog_table_position, table_density, blend_fog, blend_fog_integer};

struct TracedFog {
    color: vec4<f32>,
    clipping: vec4<f32>,
    table: array<vec4<f32>, 16>,
};

@group(0) @binding(0) var screen: texture_2d<f32>;
@group(0) @binding(1) var depth: texture_depth_2d;
@group(0) @binding(2) var<uniform> view: View;
@group(0) @binding(3) var<uniform> fog: TracedFog;

// As `scene_shading`'s modern fog.
const MODERN_FOG_STEEPNESS: f32 = 4.0;

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(in.position.xy);
    let color = textureLoad(screen, pixel, 0);
    let ndc_depth = textureLoad(depth, pixel, 0);
    // Nothing was drawn here; the sky and the forward-drawn meshes come later.
    if ndc_depth <= 0.0 { return color; }
    let ndc = vec4(in.uv.x * 2.0 - 1.0, 1.0 - in.uv.y * 2.0, ndc_depth, 1.0);
    let position = view.view_from_clip * ndc;
    // Engine units, as the scene materials measure fog depth.
    let distance = -position.z / position.w * 20.0;
    if fog.clipping.z != 0.0 {
        let t = clamp(distance / fog.clipping.y, 0.0, 1.0);
        let density = 255.0 * (1.0 - exp(-MODERN_FOG_STEEPNESS * t * t)) / (1.0 - exp(-MODERN_FOG_STEEPNESS));
        return vec4(blend_fog(color.rgb, fog.color.rgb, density), color.a);
    }
    let table = fog_table_position(distance);
    let next = min(table.x + 1u, 63u);
    let density = table_density(fog.table[table.x / 4u][table.x % 4u], fog.table[next / 4u][next % 4u], table.y);
    return vec4(blend_fog_integer(color.rgb, fog.color.rgb, density), color.a);
}

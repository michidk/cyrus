// The modern lighting's screen pass: water reflections and the sun's lens flare; see `post.rs`.
import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;
import bevy_render::view::View;
import package::water_surface::{water_wave, water_ripple};

struct ModernPost {
    sun: vec4<f32>,
    sun_color: vec4<f32>,
    horizon: vec4<f32>,
    time: vec4<f32>,
};

@group(0) @binding(0) var screen: texture_2d<f32>;
@group(0) @binding(1) var screen_sampler: sampler;
@group(0) @binding(2) var depth: texture_depth_2d;
@group(0) @binding(3) var<uniform> view: View;
@group(0) @binding(4) var<uniform> settings: ModernPost;

// Reflection march: steps, the first step and its growth, in world units.
const MARCH_STEPS: i32 = 48;
const FIRST_STEP: f32 = 0.5;
const STEP_GROWTH: f32 = 1.1;

fn depth_at(uv: vec2<f32>) -> f32 {
    let size = vec2<i32>(textureDimensions(depth));
    let pixel = clamp(vec2<i32>(uv * vec2<f32>(size)), vec2(0), size - vec2(1));
    return textureLoad(depth, pixel, 0);
}

fn world_at(uv: vec2<f32>, ndc_depth: f32) -> vec3<f32> {
    let world = view.world_from_clip * vec4(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, ndc_depth, 1.0);
    return world.xyz / world.w;
}

// Distance in front of the camera of an NDC depth.
fn view_distance(uv: vec2<f32>, ndc_depth: f32) -> f32 {
    let position = view.view_from_clip * vec4(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, ndc_depth, 1.0);
    return -position.z / position.w;
}

// Screen texture coordinate and NDC depth of a world position; z below 0 is behind the camera.
fn project(position: vec3<f32>) -> vec3<f32> {
    let clip = view.clip_from_world * vec4(position, 1.0);
    if clip.w <= 0.0 { return vec3(-1.0); }
    let ndc = clip.xyz / clip.w;
    return vec3(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5, ndc.z);
}

fn on_screen(uv: vec2<f32>) -> bool {
    return all(uv >= vec2(0.0)) && all(uv <= vec2(1.0));
}

// The water's surface normal, matching `terrain.wesl`: its waves (`settings.time.y` high on a grid
// of `settings.time.z` cells, as the open sea) and its ripples.
fn water_normal(position: vec3<f32>) -> vec3<f32> {
    let t = settings.time.x;
    var slope = vec2(0.0);
    if settings.time.z > 0.0 {
        // Far off, the waves' tilt would band the reflections into regular stripes.
        let fade = 1.0 - smoothstep(60.0, 260.0, length(position - view.world_position));
        slope = water_wave(position.xz, t, settings.time.y, settings.time.z, 0.0).yz * fade;
    }
    let ripple = water_ripple(position.xz, t, length(position - view.world_position));
    return normalize(vec3(-slope.x + ripple.x * 0.05, 1.0, -slope.y + ripple.y * 0.05));
}

fn reflection(uv: vec2<f32>, color: vec3<f32>, water: f32, calm: bool) -> vec3<f32> {
    let surface_depth = depth_at(uv);
    if surface_depth <= 0.0 { return color; }
    let origin = world_at(uv, surface_depth);
    let view_direction = normalize(origin - view.world_position);
    let normal = select(water_normal(origin), vec3(0.0, 1.0, 0.0), calm);
    let ray = reflect(view_direction, normal);
    let fresnel = 0.02 + 0.98 * pow(1.0 - clamp(dot(-view_direction, normal), 0.0, 1.0), 5.0);
    // A ray that leaves the screen sees the horizon, but keeps a brighter glint of the sun.
    var reflected = max(settings.horizon.rgb, color);
    if ray.y > 0.0 {
        var travelled = FIRST_STEP;
        var stride = FIRST_STEP;
        for (var i = 0; i < MARCH_STEPS; i++) {
            let probe = project(origin + ray * travelled);
            if probe.z <= 0.0 || !on_screen(probe.xy) { break; }
            let scene_depth = depth_at(probe.xy);
            // Reverse depth: the scene is in front of the ray where its depth is larger.
            if scene_depth > probe.z {
                let ray_distance = view_distance(probe.xy, probe.z);
                let gap = ray_distance - view_distance(probe.xy, scene_depth);
                if gap < 1.0 + ray_distance * 0.03 {
                    let edge = min(min(probe.x, 1.0 - probe.x), min(probe.y, 1.0 - probe.y));
                    let hit = textureSampleLevel(screen, screen_sampler, probe.xy, 0.0).rgb;
                    reflected = mix(reflected, hit, smoothstep(0.0, 0.08, edge));
                    break;
                }
            }
            travelled += stride;
            stride *= STEP_GROWTH;
        }
    }
    return mix(color, reflected, fresnel * water * settings.horizon.w);
}

// Fraction of the sun's neighbourhood where only the sky is drawn; the sky writes no depth.
fn sun_visibility(sun: vec2<f32>) -> f32 {
    let size = vec2<f32>(textureDimensions(depth));
    let reach = 6.0 / size;
    var open = 0.0;
    for (var x = -1; x <= 1; x++) {
        for (var y = -1; y <= 1; y++) {
            let uv = sun + vec2(f32(x), f32(y)) * reach;
            if on_screen(uv) && depth_at(uv) <= 0.0 { open += 1.0; }
        }
    }
    return open / 9.0;
}

fn ghost(uv: vec2<f32>, center: vec2<f32>, radius: f32, aspect: f32) -> f32 {
    let offset = (uv - center) * vec2(aspect, 1.0);
    return 1.0 - smoothstep(radius * 0.6, radius, length(offset));
}

fn lens_flare(uv: vec2<f32>) -> vec3<f32> {
    let sun = settings.sun.xy;
    // Fades in as the sun comes within a fifth of the screen of its edge.
    let margin = min(min(sun.x, 1.0 - sun.x), min(sun.y, 1.0 - sun.y));
    let strength = settings.sun.w * smoothstep(-0.2, 0.05, margin) * sun_visibility(clamp(sun, vec2(0.0), vec2(1.0)));
    if strength <= 0.0 { return vec3(0.0); }
    let size = vec2<f32>(textureDimensions(screen));
    let aspect = size.x / size.y;
    let offset = (uv - sun) * vec2(aspect, 1.0);
    let radius = length(offset);
    let glare = exp(-radius * radius * 60.0) * 0.35 + exp(-radius * 5.0) * 0.06;
    let streak = exp(-abs(offset.y) * 400.0) * exp(-abs(offset.x) * 2.5) * 0.3;
    // Ghosts along the line from the sun through the screen's center.
    let axis = vec2(0.5) - sun;
    var ghosts = vec3(0.0);
    ghosts += ghost(uv, sun + axis * 0.7, 0.03, aspect) * vec3(0.10, 0.07, 0.03);
    ghosts += ghost(uv, sun + axis * 1.25, 0.05, aspect) * vec3(0.02, 0.04, 0.06);
    ghosts += ghost(uv, sun + axis * 1.6, 0.02, aspect) * vec3(0.08, 0.05, 0.10);
    ghosts += ghost(uv, sun + axis * 2.1, 0.12, aspect) * vec3(0.02, 0.04, 0.03);
    let color = settings.sun_color.rgb;
    return (color * (glare + streak) + ghosts * color) * strength;
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let source = textureSampleLevel(screen, screen_sampler, in.uv, 0.0);
    var color = source.rgb;
    // The terrain writes one minus half its water weight into the alpha, 0.5 to 1; calm water models
    // (fountains, moats) write zero and reflect fully without ripples.
    let calm = source.a < 0.25;
    let water = select(clamp((1.0 - source.a) * 2.0, 0.0, 1.0), 1.0, calm);
    if water > 0.0 && settings.horizon.w > 0.0 {
        color = reflection(in.uv, color, water, calm);
    }
    if settings.sun.z > 0.0 && settings.sun.w > 0.0 {
        color += lens_flare(in.uv);
    }
    return vec4(color, 1.0);
}

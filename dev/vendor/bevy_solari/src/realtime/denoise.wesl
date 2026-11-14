// Spatio-temporal denoiser for the ReSTIR output; not part of upstream bevy_solari.
//
// The lighting passes leave `albedo * irradiance * exposure` in `view_output`. These passes
// divide the albedo back out, so only the noisy light is filtered and textures stay sharp,
// accumulate it over time with a validated reprojection, filter it with an edge-stopping
// a-trous kernel, and multiply the albedo back in.
//
// Group 2 holds three RGBA16F textures: `history` (the accumulated light, alpha = sample count),
// and the `ping` and `pong` filter targets.
import package::realtime::gbuffer_utils::{gpixel_resolve, pixel_dissimilar};
import package::realtime::bindings::{constants, depth_buffer, gbuffer, load_motion_vector, previous_depth_buffer, previous_gbuffer, previous_view, view, view_output};

enable wgpu_ray_query;

@group(2) @binding(0) var history: texture_storage_2d<rgba16float, read_write>;
@group(2) @binding(1) var ping: texture_storage_2d<rgba16float, read_write>;
@group(2) @binding(2) var pong: texture_storage_2d<rgba16float, read_write>;

// Albedo floor shared by demodulation and remodulation, so black surfaces round-trip.
const ALBEDO_FLOOR: f32 = 0.03;
// The longest history, in frames. A longer history is steadier but reacts slower to changes.
const MAX_HISTORY: f32 = 48.0;
// Relative luminance tolerance of the spatial filter for a one-frame history; it shrinks
// with the square root of the history length.
const LUMINANCE_TOLERANCE: f32 = 2.0;

// A-trous B3 spline weight for a tap offset of 0, 1 or 2 steps.
fn kernel(offset: i32) -> f32 {
    if offset == 0 { return 1.0; }
    if offset == 1 { return 0.6666667; }
    return 0.16666667;
}

fn luminance(color: vec3<f32>) -> f32 {
    return dot(color, vec3(0.2126, 0.7152, 0.0722));
}

fn load_source(pixel_id: vec2<u32>, from_ping: bool) -> vec4<f32> {
    if from_ping {
        return textureLoad(ping, pixel_id);
    }
    return textureLoad(pong, pixel_id);
}

fn store_target(pixel_id: vec2<u32>, value: vec4<f32>, from_ping: bool) {
    if from_ping {
        textureStore(pong, pixel_id, value);
    } else {
        textureStore(ping, pixel_id, value);
    }
}

// Divides the albedo out of this frame's lighting and blends it into the reprojected history.
// Writes the result to `ping`.
@compute @workgroup_size(8, 8, 1)
fn temporal(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let size = view.main_pass_viewport.zw;
    if any(global_id.xy >= vec2u(size)) { return; }

    let depth = textureLoad(depth_buffer, global_id.xy, 0);
    if depth == 0.0 { return; }

    let surface = gpixel_resolve(textureLoad(gbuffer, global_id.xy, 0), depth, global_id.xy, size, view.world_from_clip);
    let lit = textureLoad(view_output, global_id.xy).rgb - surface.material.emissive * view.exposure;
    let irradiance = max(lit, vec3(0.0)) / max(surface.material.base_color, vec3(ALBEDO_FLOOR));

    var accumulated = irradiance;
    var count = 1.0;
    if !bool(constants.reset) {
        let motion_vector = load_motion_vector(global_id.xy);
        let previous_float = round(vec2<f32>(global_id.xy) - motion_vector * size);
        if all(previous_float >= vec2(0.0)) && all(previous_float < size) {
            let previous_id = vec2<u32>(previous_float);
            let previous_depth = textureLoad(previous_depth_buffer, previous_id, 0);
            let previous_surface = gpixel_resolve(textureLoad(previous_gbuffer, previous_id, 0), previous_depth, previous_id, size, previous_view.world_from_clip);
            if previous_depth != 0.0 && !pixel_dissimilar(depth, surface.world_position, previous_surface.world_position, surface.world_normal, previous_surface.world_normal, view) {
                let previous = textureLoad(history, previous_id);
                count = min(previous.a + 1.0, MAX_HISTORY);
                accumulated = mix(previous.rgb, irradiance, 1.0 / count);
            }
        }
    }
    textureStore(ping, global_id.xy, vec4(accumulated, count));
}

// Keeps the unfiltered accumulation as next frame's history, so filtering does not feed back.
@compute @workgroup_size(8, 8, 1)
fn store_history(@builtin(global_invocation_id) global_id: vec3<u32>) {
    if any(global_id.xy >= vec2u(view.main_pass_viewport.zw)) { return; }
    textureStore(history, global_id.xy, textureLoad(ping, global_id.xy));
}

// One edge-stopping a-trous iteration: 5x5 taps spread `spacing` pixels apart. Neighbours on another
// surface (position or normal) or with very different light are ignored, and a long history
// narrows the luminance tolerance so converged areas keep their detail.
fn atrous(pixel_id: vec2<u32>, spacing: i32, from_ping: bool) {
    let size = view.main_pass_viewport.zw;
    if any(pixel_id >= vec2u(size)) { return; }

    let depth = textureLoad(depth_buffer, pixel_id, 0);
    if depth == 0.0 { return; }

    let surface = gpixel_resolve(textureLoad(gbuffer, pixel_id, 0), depth, pixel_id, size, view.world_from_clip);
    let center = load_source(pixel_id, from_ping);
    let center_luminance = luminance(center.rgb);
    let tolerance = LUMINANCE_TOLERANCE / sqrt(max(center.a, 1.0));

    var sum = vec3(0.0);
    var weight_sum = 0.0;
    for (var dy = -2; dy <= 2; dy += 1) {
        for (var dx = -2; dx <= 2; dx += 1) {
            let tap = vec2<i32>(pixel_id) + vec2(dx, dy) * spacing;
            if any(tap < vec2(0)) || any(tap >= vec2<i32>(size)) { continue; }
            let tap_id = vec2<u32>(tap);
            let tap_depth = textureLoad(depth_buffer, tap_id, 0);
            if tap_depth == 0.0 { continue; }
            let tap_surface = gpixel_resolve(textureLoad(gbuffer, tap_id, 0), tap_depth, tap_id, size, view.world_from_clip);
            if pixel_dissimilar(depth, surface.world_position, tap_surface.world_position, surface.world_normal, tap_surface.world_normal, view) { continue; }

            let neighbour = load_source(tap_id, from_ping);
            let neighbour_luminance = luminance(neighbour.rgb);
            let normal_weight = pow(max(dot(surface.world_normal, tap_surface.world_normal), 0.0), 32.0);
            let luminance_weight = exp(-abs(center_luminance - neighbour_luminance) / max(tolerance * 0.5 * (center_luminance + neighbour_luminance), 0.001));
            let weight = kernel(abs(dx)) * kernel(abs(dy)) * normal_weight * luminance_weight;
            sum += neighbour.rgb * weight;
            weight_sum += weight;
        }
    }
    // The centre tap always passes, so the sum is never empty.
    store_target(pixel_id, vec4(sum / max(weight_sum, 0.0001), center.a), from_ping);
}

@compute @workgroup_size(8, 8, 1)
fn atrous_step_1(@builtin(global_invocation_id) global_id: vec3<u32>) {
    atrous(global_id.xy, 1, true);
}

@compute @workgroup_size(8, 8, 1)
fn atrous_step_2(@builtin(global_invocation_id) global_id: vec3<u32>) {
    atrous(global_id.xy, 2, false);
}

@compute @workgroup_size(8, 8, 1)
fn atrous_step_4(@builtin(global_invocation_id) global_id: vec3<u32>) {
    atrous(global_id.xy, 4, true);
}

// Multiplies the albedo back into the filtered light and writes the pixel. The result of the
// three iterations (1, 2, 4) is in `pong`.
@compute @workgroup_size(8, 8, 1)
fn modulate(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let size = view.main_pass_viewport.zw;
    if any(global_id.xy >= vec2u(size)) { return; }

    let depth = textureLoad(depth_buffer, global_id.xy, 0);
    if depth == 0.0 { return; }

    let surface = gpixel_resolve(textureLoad(gbuffer, global_id.xy, 0), depth, global_id.xy, size, view.world_from_clip);
    let irradiance = textureLoad(pong, global_id.xy).rgb;
    let color = irradiance * max(surface.material.base_color, vec3(ALBEDO_FLOOR)) + surface.material.emissive * view.exposure;
    textureStore(view_output, global_id.xy, vec4(color, 1.0));
}

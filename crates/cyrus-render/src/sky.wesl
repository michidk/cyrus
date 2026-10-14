import bevy_pbr::render::mesh_view_bindings::view;
import constants::MATERIAL_BIND_GROUP;
import bevy_render::color_operations::{linear_to_srgb, srgb_to_linear};
import package::glide_fog::{fog_coordinate, fog_table_position, table_density, blend_fog, blend_fog_integer, framebuffer_rgb565, framebuffer_dither, framebuffer_blend_integer};

@group(MATERIAL_BIND_GROUP) @binding(0) var sky: texture_2d<f32>;
@group(MATERIAL_BIND_GROUP) @binding(1) var sky_sampler: sampler;
@group(MATERIAL_BIND_GROUP) @binding(2) var<uniform> plane: vec4<f32>;
@group(MATERIAL_BIND_GROUP) @binding(3) var<uniform> fog_rgb: vec4<f32>;
@group(MATERIAL_BIND_GROUP) @binding(4) var<uniform> scroll: vec4<f32>;
@group(MATERIAL_BIND_GROUP) @binding(5) var sun: texture_2d<f32>;
@group(MATERIAL_BIND_GROUP) @binding(6) var sun_sampler: sampler;
@group(MATERIAL_BIND_GROUP) @binding(7) var<uniform> sun_position: vec4<f32>;
@group(MATERIAL_BIND_GROUP) @binding(8) var<uniform> sun_color: vec4<f32>;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
};

@vertex
fn vertex(@location(0) position: vec3<f32>) -> VertexOutput {
    return VertexOutput(vec4(position, 1.0));
}

fn sky_fog_density(depth: f32) -> f32 {
    let coordinate = fog_coordinate(depth);
    return select(clamp(coordinate - 32.0, 0.0, 30.0) * 8.0,
                  240.0 + (coordinate - 62.0) * 15.0, coordinate >= 62.0);
}

// Preservation sky filtering: encoded palette channels and four-bit fractions.
fn faithful_cloud_sample(uv: vec2<f32>) -> vec3<f32> {
    let size = vec2<i32>(textureDimensions(sky));
    let texel = uv * vec2<f32>(size) - vec2(0.5);
    let fraction = floor(fract(texel) * 16.0) / 16.0;
    let origin = vec2<i32>(floor(texel));
    let p0 = ((origin % size) + size) % size;
    let p1 = (p0 + vec2(1)) % size;
    let a = round(linear_to_srgb(textureLoad(sky, p0, 0).rgb) * 255.0);
    let b = round(linear_to_srgb(textureLoad(sky, vec2(p1.x, p0.y), 0).rgb) * 255.0);
    let c = round(linear_to_srgb(textureLoad(sky, vec2(p0.x, p1.y), 0).rgb) * 255.0);
    let d = round(linear_to_srgb(textureLoad(sky, p1, 0).rgb) * 255.0);
    let top = a + floor((b - a) * fraction.x);
    let bottom = c + floor((d - c) * fraction.x);
    return srgb_to_linear((top + floor((bottom - top) * fraction.y)) / 255.0);
}

// Sun alpha uses preservation's software Voodoo 1 sun alpha filtering.
fn faithful_sun_sample(uv: vec2<f32>) -> u32 {
    let size = vec2<i32>(textureDimensions(sun));
    let texel = uv * vec2<f32>(size) - vec2(0.5);
    let fraction = floor(fract(texel) * 16.0) / 16.0;
    let origin = vec2<i32>(floor(texel));
    let p0 = ((origin % size) + size) % size;
    let p1 = (p0 + vec2(1)) % size;
    let a = round(textureLoad(sun, p0, 0).r * 255.0);
    let b = round(textureLoad(sun, vec2(p1.x, p0.y), 0).r * 255.0);
    let c = round(textureLoad(sun, vec2(p0.x, p1.y), 0).r * 255.0);
    let d = round(textureLoad(sun, p1, 0).r * 255.0);
    let top = a + floor((b - a) * fraction.x);
    let bottom = c + floor((d - c) * fraction.x);
    return u32(top + floor((bottom - top) * fraction.y));
}

// Preservation sky.md: separately truncated terms and RGB565 dither subtraction.


fn cloud_color(ray: vec3<f32>, near: vec3<f32>) -> vec3<f32> {
    if plane.z == 0.0 { return srgb_to_linear(fog_rgb.rgb); }
    if scroll.z != 0.0 {
        let direction = normalize(ray);
        let cloud_plane = direction.xz / max(direction.y, 0.001) * abs(plane.x);
        let uv = (-plane.y * cloud_plane / 65000.0 + scroll.xy) / plane.zw;
        let color = textureSampleLevel(sky, sky_sampler, uv, 0.0).rgb;
        let depth = abs(plane.x) * (-near.z / length(near)) / max(direction.y, 0.001);
        let haze = sky_fog_density(depth);
        return blend_fog(color, fog_rgb.rgb, haze);
    }
    // Preservation's camera-relative 24-unit tilt, applied before the height offset.
    let forward = -view.world_from_view[2].xz;
    let heading = atan2(forward.x, forward.y);
    let horizontal = vec2(sin(heading), cos(heading));
    let tilt = 24.0 * 6.2831852 / 2048.0;
    let normal = vec3(horizontal.x * sin(tilt), cos(tilt), horizontal.y * sin(tilt));
    let denominator = dot(normal, ray);
    if denominator == 0.0 { return srgb_to_linear(fog_rgb.rgb); }
    let distance = cos(tilt) * (-plane.x / 20.0 - view.world_position.y) / denominator;
    let relative_xz = ray.xz * distance;
    let plane_xz = relative_xz + (1.0 / cos(tilt) - 1.0) * dot(relative_xz, horizontal) * horizontal;
    let depth = -near.z * distance;
    if distance <= 0.0 || depth < 0.05 || depth > 3250.0 || any(abs(plane_xz) > vec2(3250.0)) {
        return srgb_to_linear(fog_rgb.rgb);
    }
    // Preservation RGM export shifts Z; sky.md uses the original camera coordinates.
    let camera_z = (view.world_position.z * 5120.0 + 16777215.0) / 256.0;
    let texture_center = vec2(-view.world_position.x * 20.0, -camera_z) * 0.009;
    let uv = (-plane.y * plane_xz * 20.0 / 65000.0 + texture_center + scroll.xy) / plane.zw;
    let color = faithful_cloud_sample(uv);
    if scroll.w != 0.0 { return blend_fog(color, fog_rgb.rgb, sky_fog_density(depth * 20.0)); }
    let position = fog_table_position(depth * 20.0);
    let current = select(f32(max(position.x, 32u) - 32u) * 8.0, 255.0, position.x == 63u);
    let next_index = min(position.x + 1u, 63u);
    let next = select(f32(max(next_index, 32u) - 32u) * 8.0, 255.0, next_index == 63u);
    return blend_fog_integer(color, fog_rgb.rgb, table_density(current, next, position.y));
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let screen_uv = (in.position.xy - view.main_pass_viewport.xy) / view.main_pass_viewport.zw;
    let near_h = view.view_from_clip * vec4(screen_uv * vec2(2.0, -2.0) + vec2(-1.0, 1.0), 1.0, 1.0);
    let near = near_h.xyz / near_h.w;
    let ray = (view.world_from_view * vec4(near, 0.0)).xyz;
    var color = cloud_color(ray, near);
    if scroll.w == 0.0 { color = framebuffer_rgb565(color, in.position.xy); }
    if sun_position.w > 0.0 {
        let center = (view.view_from_world * vec4(sun_position.xyz, 0.0)).xyz;
        let depth = -center.z;
        let uv = (near.xy * (depth / -near.z) - center.xy) * vec2(1.0, -1.0)
            / (2.0 * vec2(sun_position.w, sun_color.w)) + vec2(0.5);
        if depth >= 0.05 && depth <= 3276.75 && all(uv >= vec2(0.0)) && all(uv <= vec2(1.0)) {
            var opacity: f32;
            var alpha = 0u;
            if scroll.z == 0.0 {
                alpha = faithful_sun_sample(uv);
                opacity = f32(alpha) / 255.0;
            } else {
                opacity = textureSampleLevel(sun, sun_sampler, uv, 0.0).r;
            }
            if scroll.z == 0.0 && scroll.w == 0.0 {
                color = framebuffer_blend_integer(color, sun_color.rgb, alpha, framebuffer_dither(in.position.xy));
            } else {
                color = srgb_to_linear(mix(linear_to_srgb(color), sun_color.rgb, opacity));
            }
        }
    }
    if scroll.w == 0.0 { color = framebuffer_rgb565(color, in.position.xy); }
    return vec4(color, 1.0);
}

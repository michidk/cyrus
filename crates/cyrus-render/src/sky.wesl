import bevy_pbr::render::mesh_view_bindings::view;
import constants::MATERIAL_BIND_GROUP;
import bevy_render::color_operations::{linear_to_srgb, srgb_to_linear};
import package::glide_fog::{fog_coordinate, fog_table_position, table_density, blend_fog, blend_fog_integer};

@group(MATERIAL_BIND_GROUP) @binding(0) var sky: texture_2d<f32>;
@group(MATERIAL_BIND_GROUP) @binding(1) var sky_sampler: sampler;
@group(MATERIAL_BIND_GROUP) @binding(5) var sun: texture_2d<f32>;
@group(MATERIAL_BIND_GROUP) @binding(6) var sun_sampler: sampler;
struct SkyParameters {
    plane: vec4<f32>,
    fog_rgb: vec4<f32>,
    scroll: vec4<f32>,
    sun_position: vec4<f32>,
    sun_color: vec4<f32>,
    day_night: vec4<f32>,
    day_night_sun: vec4<f32>,
    modern_sky: vec4<f32>,
};

// Rotates `v` by the unit quaternion `q`.
fn rotate(q: vec4<f32>, v: vec3<f32>) -> vec3<f32> {
    return v + 2.0 * cross(q.xyz, cross(q.xyz, v) + q.w * v);
}
@group(MATERIAL_BIND_GROUP) @binding(2) var<uniform> sky_parameters: SkyParameters;

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
fn faithful_cloud_single(uv: vec2<f32>) -> vec3<f32> {
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
fn faithful_sun_single(uv: vec2<f32>) -> u32 {
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

fn cloud_color(ray: vec3<f32>, near: vec3<f32>) -> vec3<f32> {
    if sky_parameters.plane.z == 0.0 { return srgb_to_linear(sky_parameters.fog_rgb.rgb); }
    if sky_parameters.scroll.z != 0.0 {
        let direction = normalize(ray);
        let cloud_plane = direction.xz / max(direction.y, 0.001) * abs(sky_parameters.plane.x);
        let uv = (-sky_parameters.plane.y * cloud_plane / 65000.0 + sky_parameters.scroll.xy) / sky_parameters.plane.zw;
        var color = textureSampleLevel(sky, sky_sampler, uv, 0.0).rgb;
        if sky_parameters.fog_rgb.w != 0.0 { color = faithful_cloud_sample(uv); }
        let depth = abs(sky_parameters.plane.x) * (-near.z / length(near)) / max(direction.y, 0.001);
        let haze = sky_fog_density(depth);
        return blend_fog(color, sky_parameters.fog_rgb.rgb, haze);
    }
    // Preservation's camera-relative 24-unit tilt, applied before the height offset.
    let forward = -view.world_from_view[2].xz;
    let heading = atan2(forward.x, forward.y);
    let horizontal = vec2(sin(heading), cos(heading));
    let tilt = 24.0 * 6.2831852 / 2048.0;
    let normal = vec3(horizontal.x * sin(tilt), cos(tilt), horizontal.y * sin(tilt));
    let denominator = dot(normal, ray);
    if denominator == 0.0 { return srgb_to_linear(sky_parameters.fog_rgb.rgb); }
    let distance = cos(tilt) * (-sky_parameters.plane.x / 20.0 - view.world_position.y) / denominator;
    let relative_xz = ray.xz * distance;
    let plane_xz = relative_xz + (1.0 / cos(tilt) - 1.0) * dot(relative_xz, horizontal) * horizontal;
    let depth = -near.z * distance;
    if distance <= 0.0 || depth < 0.05 || depth > 3250.0 || any(abs(plane_xz) > vec2(3250.0)) {
        return srgb_to_linear(sky_parameters.fog_rgb.rgb);
    }
    // Preservation RGM export shifts Z; sky.md uses the original camera coordinates.
    let camera_z = (view.world_position.z * 5120.0 + 16777215.0) / 256.0;
    let texture_center = vec2(-view.world_position.x * 20.0, -camera_z) * 0.009;
    let uv = (-sky_parameters.plane.y * plane_xz * 20.0 / 65000.0 + texture_center + sky_parameters.scroll.xy) / sky_parameters.plane.zw;
    let color = faithful_cloud_sample(uv);
    if sky_parameters.scroll.w != 0.0 { return blend_fog(color, sky_parameters.fog_rgb.rgb, sky_fog_density(depth * 20.0)); }
    let position = fog_table_position(depth * 20.0);
    let current = select(f32(max(position.x, 32u) - 32u) * 8.0, 255.0, position.x == 63u);
    let next_index = min(position.x + 1u, 63u);
    let next = select(f32(max(next_index, 32u) - 32u) * 8.0, 255.0, next_index == 63u);
    return blend_fog_integer(color, sky_parameters.fog_rgb.rgb, table_density(current, next, position.y));
}


// Modern sky effect, a tuning choice: a hash for the stars.
fn star_hash(cell: vec2<f32>) -> f32 {
    return fract(sin(dot(cell, vec2(127.1, 311.7))) * 43758.5453);
}

// Brightness of the star field towards `direction`, on a grid over azimuth and height.
fn stars(direction: vec3<f32>) -> f32 {
    if direction.y <= 0.02 || direction.y >= 0.95 { return 0.0; }
    let azimuth = atan2(direction.z, direction.x) / 6.2831852 + 0.5;
    let grid = vec2(azimuth * 720.0, direction.y * 160.0);
    let cell = floor(grid);
    let chance = star_hash(cell);
    if chance < 0.985 { return 0.0; }
    let center = cell + vec2(star_hash(cell + 17.0), star_hash(cell + 31.0)) * 0.6 + 0.2;
    let brightness = (chance - 0.985) / 0.015;
    return (1.0 - smoothstep(0.0, 0.35, length(grid - center))) * brightness
        * smoothstep(0.02, 0.2, direction.y);
}

// Colour of the clouds at night, as a factor on their daytime colour; a Modern tuning choice.
const MOONLIT_CLOUDS: vec3<f32> = vec3(0.16, 0.18, 0.26);

// The modern sky effect: glow around the sun, a warm horizon while it is low, and stars at night.
fn modern_sky(color: vec3<f32>, clouds: vec3<f32>, ray: vec3<f32>) -> vec3<f32> {
    let direction = normalize(ray);
    var result = color;
    var sun = sky_parameters.modern_sky.xyz;
    var sun_tint = vec3(1.0);
    if sky_parameters.sun_position.w > 0.0 {
        sun = normalize(rotate(sky_parameters.day_night_sun, sky_parameters.sun_position.xyz));
        sun_tint = sky_parameters.sun_color.rgb;
    }
    if dot(sun, sun) > 0.25 {
        let glow_color = srgb_to_linear(clamp(sun_tint * sky_parameters.day_night.rgb, vec3(0.0), vec3(1.0)));
        let facing = max(dot(direction, sun), 0.0);
        let glow = 0.2 * pow(facing, 12.0) + 0.25 * pow(facing, 400.0);
        result += glow_color * glow * sky_parameters.day_night.w;
        let low = (1.0 - smoothstep(0.0, 0.35, sun.y)) * smoothstep(-0.15, 0.0, sun.y);
        let toward = max(dot(normalize(direction.xz + vec2(1e-5)), normalize(sun.xz + vec2(1e-5))), 0.0);
        let horizon = pow(1.0 - clamp(direction.y, 0.0, 1.0), 5.0) * toward * toward * low;
        result += vec3(1.0, 0.45, 0.2) * horizon * 0.35;
    }
    // Stars where the day/night cycle darkens the sky, hidden behind bright clouds.
    let daylight = max(sky_parameters.day_night.r, max(sky_parameters.day_night.g, sky_parameters.day_night.b));
    let night = 1.0 - smoothstep(0.2, 0.6, daylight);
    if night > 0.0 {
        let cloud = smoothstep(0.35, 0.7, dot(clouds, vec3(0.2126, 0.7152, 0.0722)));
        result += vec3(0.9, 0.95, 1.0) * stars(direction) * night * (1.0 - cloud) * 0.8;
        // The night tint darkens the clouds with the sky; moonlight keeps them visible.
        result = mix(result, clouds * MOONLIT_CLOUDS, cloud * night);
    }
    return result;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let screen_uv = (in.position.xy - view.main_pass_viewport.xy) / view.main_pass_viewport.zw;
    let near_h = view.view_from_clip * vec4(screen_uv * vec2(2.0, -2.0) + vec2(-1.0, 1.0), 1.0, 1.0);
    let near = near_h.xyz / near_h.w;
    let ray = (view.world_from_view * vec4(near, 0.0)).xyz;
    let clouds = cloud_color(ray, near);
    var color = clouds;
    // The modern day/night cycle tints the encoded sky; neutral leaves it untouched.
    if any(sky_parameters.day_night.rgb != vec3(1.0)) {
        color = srgb_to_linear(linear_to_srgb(color) * sky_parameters.day_night.rgb);
    }
    if sky_parameters.sun_position.w > 0.0 && sky_parameters.day_night.w > 0.0 {
        let sun_center = rotate(sky_parameters.day_night_sun, sky_parameters.sun_position.xyz);
        let center = (view.view_from_world * vec4(sun_center, 0.0)).xyz;
        let depth = -center.z;
        let uv = (near.xy * (depth / -near.z) - center.xy) * vec2(1.0, -1.0)
            / (2.0 * vec2(sky_parameters.sun_position.w, sky_parameters.sun_color.w)) + vec2(0.5);
        if depth >= 0.05 && depth <= 3276.75 && all(uv >= vec2(0.0)) && all(uv <= vec2(1.0)) {
            var opacity: f32;
            var alpha = 0u;
            if sky_parameters.scroll.z == 0.0 || sky_parameters.fog_rgb.w != 0.0 {
                alpha = faithful_sun_sample(uv);
                opacity = f32(alpha) / 255.0;
            } else {
                opacity = textureSampleLevel(sun, sun_sampler, uv, 0.0).r;
            }
            opacity *= sky_parameters.day_night.w;
            color = srgb_to_linear(mix(linear_to_srgb(color), sky_parameters.sun_color.rgb, opacity));
        }
    }
    if sky_parameters.modern_sky.w > 0.0 {
        color = modern_sky(color, clouds, ray);
    }
    return vec4(color, 1.0);
}

fn footprint(dx: vec2<f32>, dy: vec2<f32>) -> vec3<f32> {
    let major = select(dy, dx, dot(dx, dx) >= dot(dy, dy));
    let minor = min(length(dx), length(dy));
    return vec3(major, clamp(ceil(length(major) / max(minor, 1.0)), 1.0, 16.0));
}

fn faithful_cloud_sample(uv: vec2<f32>) -> vec3<f32> {
    let size = vec2<f32>(textureDimensions(sky));
    let axis = footprint(dpdx(uv) * size, dpdy(uv) * size);
    if sky_parameters.fog_rgb.w == 0.0 { return faithful_cloud_single(uv); }
    var sum = vec3(0.0);
    for (var i = 0u; i < u32(axis.z); i++) {
        sum += faithful_cloud_single(uv + axis.xy / size * ((f32(i) + 0.5) / axis.z - 0.5));
    }
    return sum / axis.z;
}

fn faithful_sun_sample(uv: vec2<f32>) -> u32 {
    let size = vec2<f32>(textureDimensions(sun));
    let axis = footprint(dpdx(uv) * size, dpdy(uv) * size);
    if sky_parameters.fog_rgb.w == 0.0 { return faithful_sun_single(uv); }
    var sum = 0.0;
    for (var i = 0u; i < u32(axis.z); i++) {
        sum += f32(faithful_sun_single(uv + axis.xy / size * ((f32(i) + 0.5) / axis.z - 0.5)));
    }
    return u32(round(sum / axis.z));
}

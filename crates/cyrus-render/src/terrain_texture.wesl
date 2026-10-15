// Signed fixed texture planes use low/high u32 words for native and WebGL.
// https://github.com/michidk/redguard-preservation/blob/837f22c/docs/engine/terrain-lighting.md

fn wide_sum(a: vec2<u32>, b: vec2<u32>) -> vec2<u32> {
    let low = a.x + b.x;
    return vec2(low, a.y + b.y + select(0u, 1u, low < a.x));
}

fn full_product(a: u32, b: u32) -> vec2<u32> {
    let low = (a & 65535u) * (b & 65535u);
    let cross_a = (a >> 16u) * (b & 65535u);
    let cross_b = (a & 65535u) * (b >> 16u);
    let middle = (low >> 16u) + (cross_a & 65535u) + (cross_b & 65535u);
    let high = (a >> 16u) * (b >> 16u) + (cross_a >> 16u) + (cross_b >> 16u) + (middle >> 16u);
    return vec2((low & 65535u) | (middle << 16u), high);
}

fn wide_product(a: vec2<u32>, b: vec2<u32>) -> vec2<u32> {
    let low = full_product(a.x, b.x);
    return vec2(low.x, low.y + a.x * b.y + a.y * b.x);
}

fn signed_words(value: i32) -> vec2<u32> {
    return vec2(bitcast<u32>(value), select(0u, 4294967295u, value < 0));
}

fn texture_plane(start: vec2<u32>, dx: vec2<u32>, dy: vec2<u32>, pixel: vec2<i32>) -> vec2<u32> {
    return wide_sum(wide_sum(start, wide_product(dx, signed_words(pixel.x))), wide_product(dy, signed_words(pixel.y)));
}

// The installed terrain clipping range keeps reciprocal-depth accumulators
// positive and below 2^32. Starts and gradients still require all 64 bits.
fn texture_reciprocal(w: u32) -> vec2<u32> {
    let leading = countLeadingZeros(w);
    let normalized = w << leading;
    let index = (normalized >> 22u) & 511u;
    let fraction = (normalized >> 14u) & 255u;
    let a = 2147483648u / (512u + index);
    let b = 2147483648u / (513u + index);
    let value = (a * (256u - fraction) + b * fraction) >> 8u;
    if leading <= 6u { return vec2(value >> (6u - leading), 0u); }
    let shift = leading - 6u;
    return vec2(value << shift, value >> (32u - shift));
}

fn perspective_coordinate(value: vec2<u32>, reciprocal: vec2<u32>) -> i32 {
    let product = wide_product(value, reciprocal);
    return bitcast<i32>((product.x >> 29u) | (product.y << 3u));
}

fn texture_coordinates(a: vec4<u32>, b: vec4<u32>, c: vec4<u32>, d: vec4<u32>, e: vec4<u32>, pixel: vec2<i32>) -> vec2<i32> {
    let s = texture_plane(a.xy, a.zw, b.xy, pixel);
    let t = texture_plane(b.zw, c.xy, c.zw, pixel);
    let w = texture_plane(d.xy, d.zw, e.xy, pixel);
    let reciprocal = texture_reciprocal(w.x);
    return vec2(perspective_coordinate(s, reciprocal), perspective_coordinate(t, reciprocal));
}

fn terrain_texture_uv(a: vec4<u32>, b: vec4<u32>, c: vec4<u32>, d: vec4<u32>, e: vec4<u32>, pixel: vec2<i32>, size: vec2<u32>) -> vec2<f32> {
    let coordinate = texture_coordinates(a, b, c, d, e, pixel);
    // Select the four-bit sample before converting to float. Glide has 256
    // coordinate units and 18 fractional bits; exported images reverse rows.
    let shift = vec2(22u) - (vec2(31u) - countLeadingZeros(size));
    let uv = vec2<f32>(coordinate >> shift) / (vec2<f32>(size) * 16.0);
    return vec2(uv.x, 1.0 - uv.y);
}

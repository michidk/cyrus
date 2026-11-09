// The modern water surface, shared by the terrain shader and the modern screen pass so the waves
// the terrain shades are the waves the reflections bend with. Every wave, ripple and foam shape is
// a Modern tuning choice, not recovered behaviour.

// One travelling wave: direction (unit), wavelength in grid cells (at least four, so the terrain's
// vertex grid, which the waves move, resolves it), speed in cells per second, and
// its share of the amplitude. The shares sum to one half, so the surface stays within
// `amplitude * (0.5 ± 0.5)` around the original's mean height, as `water::modern_displacement`.
struct Wave {
    direction: vec2<f32>,
    length: f32,
    speed: f32,
    share: f32,
};

fn wave(i: u32) -> Wave {
    switch i {
        case 0u: { return Wave(vec2(0.940, 0.342), 11.0, 1.10, 0.20); }
        case 1u: { return Wave(vec2(0.643, -0.766), 7.0, 0.95, 0.13); }
        case 2u: { return Wave(vec2(-0.259, 0.966), 5.0, 0.80, 0.09); }
        default: { return Wave(vec2(0.996, -0.087), 4.0, 0.70, 0.08); }
    }
}

// The height above the base grid and its slope along x and z, in world units, at a grid position
// in world units. `calm` from 0 (open sea) to 1 (at the shore) flattens the waves towards the beach.
fn water_wave(position: vec2<f32>, seconds: f32, amplitude: f32, cell: f32, calm: f32) -> vec3<f32> {
    let tau = 6.283185307;
    var height = 0.5;
    var slope = vec2(0.0);
    for (var i = 0u; i < 4u; i++) {
        let w = wave(i);
        let k = tau / (w.length * cell);
        let phase = dot(position, w.direction) * k - seconds * w.speed * tau / w.length;
        // A sharpened sine: crests narrower than troughs, as wind waves are.
        let s = sin(phase);
        let shaped = s * (1.0 - 0.25 * (1.0 - abs(s)));
        height += w.share * shaped;
        slope += w.share * cos(phase) * (0.75 + 0.5 * abs(s)) * k * w.direction;
    }
    let scale = amplitude * (1.0 - 0.6 * calm);
    return vec3(height * scale, slope * scale);
}

// Small ripples on top of the waves, as a normal perturbation in x and z; several directions and
// sizes so their pattern never visibly repeats. They fade with the `distance` from the camera, in
// world units, before they are finer than a pixel and alias into stripes.
fn water_ripple(position: vec2<f32>, seconds: f32, distance: f32) -> vec2<f32> {
    let near = 1.0 - smoothstep(12.0, 45.0, distance);
    let far = 1.0 - smoothstep(50.0, 220.0, distance);
    // Noise bends the larger ripples' crests, which would otherwise line up into regular stripes.
    let bend = water_noise(position * 0.06) * 9.0;
    let a = sin(dot(position, vec2(1.3, 0.7)) - seconds * 1.7 + bend);
    let b = sin(dot(position, vec2(-0.6, 2.1)) - seconds * 2.3 - bend * 0.7);
    let c = sin(dot(position, vec2(3.7, -2.9)) + seconds * 3.1);
    let d = sin(dot(position, vec2(-4.9, -1.3)) - seconds * 2.7);
    return vec2(a * 0.6, b * 0.6) * far + vec2(c * 0.3 + d * 0.2, d * 0.3 - c * 0.2) * near;
}

fn hash(cell: vec2<f32>) -> f32 {
    return fract(sin(dot(cell, vec2(127.1, 311.7))) * 43758.5453);
}

// Smooth value noise in 0..1.
fn water_noise(position: vec2<f32>) -> f32 {
    let cell = floor(position);
    let f = fract(position);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash(cell), hash(cell + vec2(1.0, 0.0)), u.x),
               mix(hash(cell + vec2(0.0, 1.0)), hash(cell + vec2(1.0)), u.x), u.y);
}

// Foam at the drawn waterline, 0..1, on water `land` texels from it (10 or more: none near);
// `shore` is the grid distance to the shore in cells, positive on water. A broken band that runs
// up and back with the swash, a wash line rolling in to the waterline and fading as it arrives,
// and a faint breaker further out.
fn shore_foam(position: vec2<f32>, land: f32, shore: f32, seconds: f32) -> f32 {
    if land >= 10.0 && (shore < 0.2 || shore > 1.4) { return 0.0; }
    // The phase varies along the coast, so the beach never pulses in step.
    let phase = water_noise(position * 0.12) * 6.283;
    let breakup = water_noise(position * 0.35 + vec2(seconds * 0.15, seconds * 0.05));
    // Two octaves, the second rotated, so the noise grid never shows as blocks.
    let drift = vec2(seconds * 0.08, seconds * 0.3);
    let turned = vec2(0.8 * position.x - 0.6 * position.y, 0.6 * position.x + 0.8 * position.y);
    let fine = 0.6 * water_noise(position * 0.7 - drift) + 0.4 * water_noise(turned * 1.9 + drift.yx);
    // The band runs up the beach and back: wide as a wave arrives, thin as it draws back.
    let swash = 0.5 + 0.5 * sin(seconds * 0.9 + phase);
    let reach = 1.5 + 3.0 * swash;
    let band = (1.0 - smoothstep(reach * 0.5, reach, land)) * smoothstep(0.2, 0.55, fine + 0.3 * breakup);
    // A wash line rolls in from eight texels out and fades as it reaches the waterline.
    let roll = fract(seconds * 0.22 + phase * 0.159);
    let front = mix(8.0, 0.5, roll);
    let wash = (1.0 - smoothstep(0.0, 1.2, abs(land - front))) * (1.0 - roll) * smoothstep(0.35, 0.6, fine);
    // The breaker follows the grid distance, which is smooth enough out in the water.
    let warped = shore + (water_noise(position * 0.11) - 0.5) * 0.4;
    let tide = fract(warped * 0.4 + seconds * 0.15 + breakup * 0.3);
    let line = smoothstep(0.0, 0.04, tide) * (1.0 - smoothstep(0.04, 0.12, tide));
    let breakers = line * smoothstep(0.2, 0.5, warped) * (1.0 - smoothstep(0.6, 1.3, warped))
        * smoothstep(0.45, 0.7, fine);
    return clamp(max(max(band, wash * 0.7), breakers * 0.4), 0.0, 0.85);
}

// How wet the sand is, 0..1, within about a cell of the shore, with the swash running up and back.
// The caller keeps it off water texels.
fn shore_wetness(shore: f32, position: vec2<f32>, seconds: f32) -> f32 {
    if shore > 0.6 || shore < -1.8 { return 0.0; }
    let warped = shore + (water_noise(position * 0.11) - 0.5) * 0.4;
    let swash = 0.95 + 0.15 * sin(seconds * 0.6 + water_noise(position * 0.2) * 6.0);
    return 1.0 - smoothstep(swash, swash + 0.4, -warped);
}

// The terrain water channel's y packs the grid shore distance (within ±4.5) with ten times the
// cell's land-border mask, which is the same at every vertex of the cell (`Shore::channel`).
fn shore_distance(channel: f32) -> f32 {
    return channel - 10.0 * floor((channel + 5.0) / 10.0);
}

fn land_border_mask(channel: f32) -> u32 {
    return u32(max(floor((channel + 5.0) / 10.0), 0.0));
}

// Distance from `uv` to the nearest edge or corner of the cell's tile that borders a land cell, in
// tiles; large where none does. Bits 0 to 3: edges u = 0, u = 1, v = 0, v = 1; 4 to 7: corners
// (0, 0), (1, 0), (0, 1), (1, 1).
fn land_border_distance(uv: vec2<f32>, mask: u32) -> f32 {
    let p = clamp(uv, vec2(0.0), vec2(1.0));
    var d = 10.0;
    if (mask & 1u) != 0u { d = min(d, p.x); }
    if (mask & 2u) != 0u { d = min(d, 1.0 - p.x); }
    if (mask & 4u) != 0u { d = min(d, p.y); }
    if (mask & 8u) != 0u { d = min(d, 1.0 - p.y); }
    if (mask & 16u) != 0u { d = min(d, length(p)); }
    if (mask & 32u) != 0u { d = min(d, length(p - vec2(1.0, 0.0))); }
    if (mask & 64u) != 0u { d = min(d, length(p - vec2(0.0, 1.0))); }
    if (mask & 128u) != 0u { d = min(d, length(p - vec2(1.0))); }
    return d;
}

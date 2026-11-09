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

// Foam on the water side of the shore, 0..1. `shore` is the distance to the shoreline in grid
// cells, positive on water: a broken band along the beach and breaker lines rolling in to it.
fn shore_foam(position: vec2<f32>, shore: f32, seconds: f32) -> f32 {
    if shore < -0.6 || shore > 2.8 { return 0.0; }
    // The shore distance follows the terrain grid; warping it hides the grid's stair steps.
    let warped = shore + (water_noise(position * 0.11) - 0.5) * 0.9;
    let breakup = water_noise(position * 0.35 + vec2(seconds * 0.05, 0.0));
    let fine = water_noise(position * 1.4 - vec2(0.0, seconds * 0.12));
    // A thin, ragged band hugging the beach.
    let band = (1.0 - smoothstep(-0.1, 0.2 + 0.3 * breakup, warped)) * smoothstep(-0.5, -0.1, warped);
    // Breakers: thin lines that travel towards the shore and fade as they arrive.
    let roll = fract(warped * 0.5 + seconds * 0.2 + breakup * 0.4);
    let line = smoothstep(0.0, 0.06, roll) * (1.0 - smoothstep(0.06, 0.18, roll));
    let breakers = line * (1.0 - smoothstep(0.4, 2.2, warped)) * smoothstep(0.45, 0.7, fine);
    return clamp(max(band * smoothstep(0.25, 0.75, fine), breakers * 0.6), 0.0, 0.85);
}

// How wet the beach is, 0..1, on the land side of the shore, with the swash running up and back.
fn shore_wetness(shore: f32, position: vec2<f32>, seconds: f32) -> f32 {
    if shore > 0.8 || shore < -3.5 { return 0.0; }
    let warped = shore + (water_noise(position * 0.11) - 0.5) * 0.9;
    let swash = 1.1 + 0.45 * sin(seconds * 0.6 + water_noise(position * 0.2) * 6.0);
    return (1.0 - smoothstep(swash, swash + 0.9, -warped)) * smoothstep(-0.5, 0.0, -warped + 0.3);
}

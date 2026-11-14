// The terrain surface hook, shared by the terrain shader and the modern screen pass. This built-in
// version keeps the original look: no waves, ripples, foam or wet sand. A mod replaces this whole
// library (`[[shader]]` with `replaces = "embedded://cyrus_render/terrain_surface.wesl"`) and turns
// the effects on through the `terrain-surface` effect slot (`hooks::TERRAIN_SURFACE`), which reaches
// the shaders as `fog.water`. Units are world units and seconds.

// The height above the base grid and its slope along x and z at a grid position, where the terrain
// shader moves the waves (`fog.water.z` is their amplitude, `fog.water.w` the grid cell size).
// `calm` from 0 (open sea) to 1 (at the shore) lets the waves flatten towards the beach.
fn water_wave(position: vec2<f32>, seconds: f32, amplitude: f32, cell: f32, calm: f32) -> vec3<f32> {
    return vec3(0.0);
}

// A normal perturbation in x and z on top of the waves, at `distance` from the camera.
fn water_ripple(position: vec2<f32>, seconds: f32, distance: f32) -> vec2<f32> {
    return vec2(0.0);
}

// Foam at the drawn waterline, 0..1, on water `land` texels from it (10 or more: none near);
// `shore` is the grid distance to the shore in cells, positive on water.
fn shore_foam(position: vec2<f32>, land: f32, shore: f32, seconds: f32) -> f32 {
    return 0.0;
}

// How wet the sand is, 0..1, within about a cell of the shore. The caller keeps it off water texels.
fn shore_wetness(shore: f32, position: vec2<f32>, seconds: f32) -> f32 {
    return 0.0;
}

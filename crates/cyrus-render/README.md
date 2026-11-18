# cyrus-render

Shared native and browser presentation: the exported world scene and its
materials (Glide fog, sky, water, terrain, texture animation), model lighting
and mesh poses, the inspection camera, the HUD, the F12 console and subtitles.
The `cyrus` and `cyrus-web` hosts compose it from the same `PreparedWorld`.
It depends on `cyrus-game`, `cyrus-world`, `cyrus-script`, `cyrus-assets` and
`redguard-preservation`; none of those depends on it, and it must not depend on
`cyrus-audio`, `cyrus-startup`, `cyrus-app` or `cyrus-debug`
(`dev/scripts/check-core-boundaries.sh`). Startup, the book menu and cinematics
live in [cyrus-startup](../cyrus-startup/README.md); sound in `cyrus-audio`; the
follow camera in `cyrus-script` (`script::camera`).

## Modules

| Module | Responsibility |
| --- | --- |
| [lib](src/lib.rs) | `WorldRenderPlugin`: scene spawn per prepared world, `ReadyScene`, Modern anisotropic filtering |
| [camera](src/camera.rs) | View camera, inspection camera and the original's horizontal field of view |
| [fog](src/fog.rs) | Scene materials: Glide fog and clipping, texture animation, environmental effects; Modern lighting and ray tracing submodules |
| [environment](src/environment.rs), [sky](src/sky.rs) | Cached environment inputs; the Glide BSI sky plane and its scrolling |
| [water](src/water.rs), [terrain](src/terrain.rs) | Terrain meshes, wave playback and per-frame terrain preparation |
| [lighting](src/lighting.rs), [sampling](src/sampling.rs) | Authored model lighting inputs; ROB sampling attributes and palette keys |
| [animation](src/animation.rs), [pose_override](src/pose_override.rs) | Mesh frame selection from decoded model frames; exclusive frozen-frame deformation |
| [player_model](src/player_model.rs), [weapon_model](src/weapon_model.rs) | Actor frame-source replacement from `.player-models/`; equipped weapon models |
| [script_transform](src/script_transform.rs), [flat](src/flat.rs) | Script actor poses on bound scene nodes; camera-facing placed flats (MPSF) |
| [hud](src/hud.rs) | `HudPlugin`: candle, compass, logbook icon, notices, power-ups, active item, prompt, inventory screen |
| [hud/artwork](src/hud/artwork.rs) | Ordered RGB565 storage of opaque HUD artwork, including retained world fog for map and inventory artwork |
| [console](src/console.rs) | F12 console paper and bitmap font for the shared editor |
| [subtitles](src/subtitles.rs) | `SubtitlePlugin` (native only): RTX voice-line text in the bottom band |
| [persistence](src/persistence.rs), [presentation_clock](src/presentation_clock.rs) | Snapshots and replacement of presentation state for session restore |
| [hooks](src/hooks.rs) | World facts and named effect slots a native mod may read or fill |

The `.wesl` files in `src/` are the shaders (`scene_shading`, `glide_fog`, `sky`,
`terrain`, `geometry_surface` and others).

## Usage

```rust
app.add_plugins((cyrus_render::WorldRenderPlugin, cyrus_render::hud::HudPlugin));
// native host only:
app.add_plugins((cyrus_render::fog::ModernLightingPlugin, cyrus_render::subtitles::SubtitlePlugin));
#[cfg(feature = "raytracing")]
app.add_plugins(cyrus_render::fog::RayTracingPlugin);
```

Core-only and headless assemblies omit these plugins. The `raytracing` feature
(native builds only) enables Bevy Solari for ray-traced Modern lighting; the
`cyrus` crate forwards it.

Check a world surface crop from a public control-port screenshot with
`python3 dev/scripts/test-world-framebuffer.py SCREENSHOT LEFT TOP RIGHT BOTTOM`
(requires Pillow). Select varied, unobscured Faithful sky, terrain, models or placed
flats. Exclude the HUD. The check rejects full-precision channels and blank crops.
For a failed-before,
passed-after comparison, use `--regression ORIGINAL BEFORE AFTER LEFT TOP RIGHT BOTTOM`
in place of the single screenshot arguments. The original and fixed crop must pass;
the baseline must exceed framebuffer precision.

Compare a held-frame blended-flat crop before and after a rendering change with
`python3 dev/scripts/test-framebuffer-blend.py ORIGINAL BEFORE AFTER LEFT TOP RIGHT BOTTOM`
(requires Pillow). Use the same camera, texture frame and lighting in every image.
The check requires a strict reduction in RGB error against the original screenshot.

## Notes

The sky retires when its requested environment asset is unavailable and rebuilds
from that asset when it becomes available again, including under the same handle.

- Faithful sky, terrain, opaque models and placed flats store fogged colors at the original RGB565
  precision with the documented four-by-four framebuffer dither. Blended flats reconstruct
  the stored destination channels and subtract their dither before alpha blending.
  The Faithful sun first stores its sky destination, then uses that same integer
  destination reconstruction and separately truncated source/destination blend terms.
  The Modern sky projection and lighting-effect branches retain full precision.
- Faithful by default. Deviations run only under their fidelity area
  (`Fog`, `Sky`, `Textures`, `Lighting`, `AnisotropicFiltering`). Modern
  lighting (`fog::modern_lighting`) is a separate PBR branch with optional
  `ModernEffects`; it never changes the Faithful path.
- Rendering rules come from preservation's engine docs, for example
  [sky](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/sky.md),
  [water](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/water.md),
  [HUD](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/hud.md) and
  [console](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/console.md).
  Fix a rule there first, not only here.
- Animated polygon admission uses the current frame face normal. Authored corner
  normals remain separate shading inputs through pose changes and model replacement.
- Loaded actor frame-source replacements become visible even when the selected pose
  is already present on the model node. Settled replacements preserve explicit
  visibility while the selected pose is unchanged, including visibility adopted
  from a checkpoint.
- Presentation never advances script stages: logical poses, health, inventory
  and dialogue are owned by `cyrus-game`/`cyrus-script`; this crate presents them.
- Stationary model colors consume in-place source prelighting, material and local-light
  table edits in the same frame, before asset notifications publish. Missing source
  meshes, source materials or runtime light tables retire the prepared draw until the
  inputs return, including restoration under the same handles. A lost prepared draw mesh
  is rebuilt from the retained source without replacing that source.
- Captured sky synchronization waits for bound sky materials, then applies once without
  replaying the requested offsets on later sky ticks.
- Camera-dependent terrain, clipping, animation meshes and effect assets update
  before Bevy extracts assets, so camera and geometry reach the GPU in the same frame.
- Material and shader changes often change the export schema: re-export worlds
  (`cargo run -p cyrus -- --export-all-worlds`) after updating them. See
  [SETUP](../../docs/SETUP.md#sky-fog-and-water) and
  [DIFFERENCES](../../docs/DIFFERENCES.md) for export prerequisites and known gaps.
- Mods extend the renderer through `hooks::EffectSlots`, shader library slots
  (`geometry_surface.wesl`, `terrain_surface.wesl`) and `WaterPlayback::source_meshes`;
  mod-specific logic (grass, for example) stays in `mods/`.

## Testing

```sh
cargo test -p cyrus-render
just verify render
```

Tests that need installed data report a skip without an install. Rendering
fidelity also needs a native and browser playtest or a matched capture against
the original (see [METHOD](../../docs/METHOD.md) and the `cyrus-playtest` skill);
compilation alone does not establish it.

Model preparation subtracts the camera origin before applying the view basis,
including backface tests, to preserve camera-relative precision far from the world origin.
See preservation’s [view projection contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/projection.md).

Faithful held-torch lights consume the script world's grip-derived tip, recovered
radius and encoded RGB strengths through the model local-light shader. Transient
emissions bypass authored map-light region selection. After matched TAVERN
readbacks, run `python3 dev/scripts/test-held-torch-light.py before.png after.png`
(with Pillow) to assert positive brightening on the wall crop.

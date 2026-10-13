# cyrus-render

Native/browser presentation: scene graphics, environment, mesh poses, inspection
camera, HUD and subtitles.
The game camera keeps the original horizontal field of view when its viewport
resizes. **Verified:** preservation's [Glide projection](https://github.com/michidk/redguard-preservation/blob/6520206/docs/engine/projection.md)
defines its scale. The original follow camera and matched-view pixel parity
remain unsupported and unverified, respectively.
Effects, music and speech playback live in `cyrus-audio`.
Depends on game/world/assets; none of those crates depends on presentation.

`WorldRenderPlugin` composes the shared scene-spawn path, environment,
`CameraPlugin` and `AnimationRenderPlugin`. Native and browser hosts submit the
same `PreparedWorld`. Core-only and native headless assemblies omit this plugin.
Script-driven actor poses retain the same scene origin as exported world placements and markers.
HUD remains a separate plugin; startup is composed by the host. Hosts install `cyrus-audio` separately;
headless assembly keeps sound control state without Bevy's audio backend.

Menu edits persist before applying. Audio preference application precedes
`AnimationSystems`, which precedes cue consumption in the same Update. Mesh poses
apply in PostUpdate. Cinematic presentation is owned by cyrus-startup.

See [SETUP](../../docs/SETUP.md) for entry points, controls, export prerequisites,
settings and fidelity limitations. Run `cargo test -p cyrus-render` for graphics
and mixed presentation contracts. Tests requiring installed data report skips.
Runtime verification also requires native and browser playtests; compilation alone
does not establish fidelity.

Re-export worlds after updating the sky renderer. Both native and browser builds
render the Glide BSI sky in Faithful graphics. **Verified:** source textures,
matching palettes, and the static plane projection follow preservation's
[sky documentation](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/sky.md).
`SYSTEM.INI`'s `sky_disable=1` suppresses the sky. Indoor worlds remove it.
**Verified:** the Glide background uses `world_fogrgb`, including beyond the
finite sky plane. Re-export old sidecars to replace their legacy background
colors. The recovered background fact is [published for preservation review](https://github.com/michidk/redguard-preservation/compare/main...fix/glide-background-fill).

**Verified:** water uses preservation's [water documentation](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/water.md):
selected-corner mask, terrain-centered phase, quarter-shifted rotation table,
and half-amplitude bias. Playback uses real elapsed time at the BIOS timer rate, preserving its clock across scenes.
It deforms private copies of exported terrain meshes and recomputes shared
normals across materials. Re-export worlds to replace old water masks.

Absolute DOS clock phase is not synchronized. Terrain placement and base heights
retain the preservation export's existing limitations.

**Verified:** terrain ambient/directional inputs, coordinate conversion and
quantized shading normals follow preservation's
[terrain-lighting contract](https://github.com/michidk/redguard-preservation/blob/feat/terrain-lighting-inputs/docs/engine/terrain-lighting.md).
Native export retains the per-world inputs in `.environment`; native and browser
playback shade private terrain meshes, including displaced water. Terrain uses
screen-linear vertex RGB and texture modulation before the existing scene fog,
without Bevy PBR or tone mapping.

**Verified:** model ambient/directional shading follows preservation's
[model-lighting rules](https://github.com/michidk/redguard-preservation/blob/feat/model-lighting/docs/engine/model-lighting.md).
Models use the installed world RGB, authored face brightness and normals, and
solid-color tint. RGB is calculated at vertices, interpolated in screen space
and combined with encoded texture colors before fog. The fixed Bevy directional
light and model PBR response are removed. Mesh transforms and explicitly selected
animation frames feed the same shader on native and browser builds.

Re-export worlds and player models for the `rgpre_lighting` primitive metadata
and required environment model inputs. Invalid mesh lighting metadata hides the
unsupported mesh and logs the world/mesh context with re-export instructions.
**Verified:** static MPSO instances bake retained RGB when the world has admitted
map lights, following preservation's [world prelighting rules](https://github.com/michidk/redguard-preservation/blob/ddbe29f/docs/engine/model-lighting.md).
Private instance meshes retain signed local-light contributions, quantized falloff,
authored matrix/bound selection and nearest-even channel rounding. Palette tint
precedes rounding and is not applied twice. Map lights accumulate in the original
spatial-cell order and are not reapplied every frame. Re-export worlds for the
required environment placement records and version-3 model lighting metadata.
**Unknown:** actor-specific prelighting eligibility, dynamic registered lights,
actor lighting overrides and complete original-game pixel parity.

The Lighting fidelity area defaults to Faithful. Modern lighting is a placeholder
that reuses Faithful results. Existing Water and Fog switches remain independent.
**Unknown:** full pixel parity, texture-filtering and combiner precision, and exact
water-edge dirty-normal retention. A captured original water update retained two
older normals that differ from a full recomputation. Cyrus recomputes the grid.

Sky tilting and the sun disc follow the documented plane transform and billboard
inputs; full original-game visual comparison remains pending. Scrolling advances texture coordinates
on the documented BIOS timer, using the per-world speed and a random initial
direction. `SkyScrollSync` and MCP `set_sky_scroll` accept a captured direction
and texture offsets. The original session's shared random stream is not reproduced.

Scene rendering uses the installed near/far planes, including a nonzero
`world_back_plane` override. F1 exposes a render-distance override in game units;
editing it switches Fog to Modern. Faithful uses the original range and fog
tables. Modern Fog defaults to four times the far distance and a gradual distant
fade. Modern Sky is camera-centered; Modern Water adds smooth crossing waves,
fine texture ripples, and view-dependent sheen while retaining the installed
textures. Water, Fog, and Sky are independently switchable and default to
Faithful. Re-export environments after this change.
The fog curve follows preservation's [fog documentation](https://github.com/michidk/redguard-preservation/blob/fix/glide-background-fill/docs/engine/fog.md).
Faithful fog uses the Voodoo 1 integer lookup and channel blend, with RGB565
framebuffer dithering for the sky and opaque/cutout scene materials. Vulkan
shader arithmetic checks cover the dither matrix and fog boundaries.
Faithful sky filtering uses encoded palette bytes, four-bit subtexel fractions,
and horizontal then vertical integer rounding, following preservation
[sky filtering facts](https://github.com/michidk/redguard-preservation/blob/e604d9c/docs/engine/sky.md).
Sun alpha rounding, other texture filtering, lighting, and physical-hardware
pixel parity remain unverified.

Startup, the book menu, settings, cinematic playback and world-entry loading live in
[cyrus-startup](../cyrus-startup/README.md).

**Verified:** preservation's [HUD evidence](https://github.com/michidk/redguard-preservation/pull/28)
establishes one candle update per rendered frame with a 60 Hz Glide retrace limit.
Cyrus advances the candle at that rate independently of rendering, without
capping the application. `normal_frame_rate` is a simulation-tick input, not
the candle cadence. Original in-world timing still needs a paired comparison.
Every documented overlay element is implemented, but only the F8, Tab and F7
toggles reach it from play; health, sword, lockout, heading, scripts and the
logbook do not feed it yet (see the roadmap).

Re-exported animated i16 poses use absolute coordinates reconstructed by rgpre.
The `animated_i16_absolute` encoding distinguishes these from older raw-delta
caches, which require re-export for nonzero-frame playback. Installed Cyrus
frames verify decoded export; projection has a separate contract test. Live
visual parity remains unchecked.

`PlayerModelPlugin` consumes explicit `AnimationPose` components on the player.
It loads the matching textured GLB from the world's `.player-models/` cache,
applies the requested frame, and replaces the model when the source changes.
Removing the pose or player removes its model. The model stays hidden until its
pose reaches the loaded node. Native and browser use the same loader path.
This adds no animation interpreter, actor activation, or simulation clock.

`ScriptTransformPlugin` presents explicitly prepared logical script poses through
existing `ScriptBinding` nodes. It preserves the stored affine matrix, converts
original axes to the shared scene coordinates, and updates descendant global
transforms after ordinary propagation. Removing or expiring a binding restores
the authored local transforms. Presentation never advances script stages.

Script flat sounds retain logical world/actor ownership. `EndSound` matches the
requested effect, `EndMySound` matches effect and receiver, and `EndMySounds`
matches the receiver. Bound animation channels share that receiver identity.
World replacement drops old logical channels and queued browser starts; pending
browser flat starts also honor selective cleanup. These handlers follow
[preservation's sound rules](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/sound.md).

The HUD consumes `ScriptWorld` activation state when a matching `ActivationInputs`
resource supplies caller-established conditions. It has no separate prompt owner
or activation timers. Missing inputs retain pending text without displaying it;
stale world inputs are ignored. Control `hud.prompt` reports the last drawn text,
while script inspection reports pending text and ownership. Lockout retains 16 bits.
Re-export HUD caches for the installed RTX label catalog, including text-only entries.
Automatic actor eligibility and production interaction scheduling remain unsupported.

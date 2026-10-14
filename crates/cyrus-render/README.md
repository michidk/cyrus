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
The framed inspection camera supplies `ScriptView` before ordinary actor updates.
Script visibility flags hide or restore bound scene nodes independently of actor
update eligibility. Static transforms continue to use the logical actor matrix.
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
without Bevy PBR or tone mapping. Terrain sampling uses encoded palette channels,
four-bit subtexel fractions and integer interpolation, following preservation
[terrain filtering](https://github.com/michidk/redguard-preservation/blob/1ded24c/docs/engine/terrain-lighting.md#texture-combination)
and [software Voodoo filtering](https://github.com/michidk/redguard-preservation/blob/e604d9c/docs/engine/sky.md).
**Verified:** preservation's [terrain texture export](https://github.com/michidk/redguard-preservation/commit/9ac35c15c66b9a5671320e42951e0a53e05ae39e)
retains palette-zero RGB and opaque alpha for every terrain frame. Re-export
older worlds for this correction; shoreline pixel parity remains unverified.

**Verified:** world nodes retain their selected ROB segment attributes through
[preservation's export contract](https://github.com/michidk/redguard-preservation/blob/7b050a3/docs/engine/model-lighting.md#export-metadata).
Models with ROB provenance select point or encoded bilinear sampling from those
attributes; non-chroma models are opaque. Instances sharing a source material
retain separate flag settings. Re-export worlds for required version-1
`rgpre_render` node metadata. Standalone and generated models explicitly retain
absent ROB provenance and their previous sampling. **Unknown:** their original
sampling defaults and texture-specific key conversion. Chroma-enabled models
still use alpha masking, so filtered chroma rejection remains unsupported.

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
Authored object lights are selected around the inspection camera and added to
model RGB each frame, including retained static colors, using preservation's
[runtime-light rules](https://github.com/michidk/redguard-preservation/blob/e0529f1/docs/engine/model-lighting.md).
Their signed colors, quantized falloff and original static transforms survive
native/browser preparation. Re-export environments for the required object-light
records. Active light scripts are explicitly unsupported during preparation.
**Verified:** [actor prelighting eligibility](https://github.com/michidk/redguard-preservation/blob/d1cbf68/docs/engine/actor-prelighting.md)
requires nonzero `attr_non_moving` and the runtime static-model path. Actor baking
remains unsupported until script initialization exposes the resolved model kind
and prelighting-time eligibility and pose. **Unknown:** scripted light changes,
actor lighting overrides and complete original-game pixel parity.
**Verified:** matched BELLTOWR original/native/browser captures at 640×480 compare
static wall and table lighting. Color/filtering differences remain; the candle
appearance also differs. The inspection camera supplies region selection until
the original follow-camera target is implemented.

Terrain prepares private triangles after wave updates and camera projection.
It clips camera-space position, RGB and UV together, snaps projected vertices,
and sends each fan triangle's fixed color coefficients to the shared shader.
Camera movement, viewport changes and clipping-range edits rebuild these meshes;
shared source assets remain unchanged. The encoded texture combiner consumes
integer lighting channels before scene fog, following preservation's
[terrain preparation rules](https://github.com/michidk/redguard-preservation/blob/449ba2a/docs/engine/terrain-lighting.md).
Fog changes update materials referenced by live scene entities; retained assets
from departed worlds do not trigger new GPU uploads.

The Lighting fidelity area defaults to Faithful. Modern lighting is a placeholder
that reuses Faithful results. Existing Water and Fog switches remain independent.
**Unknown:** full pixel parity and remaining texture-filtering and combiner precision.
**Verified:** preservation's [dirty-normal admission rules](https://github.com/michidk/redguard-preservation/blob/ebd7d9e/docs/engine/water.md#dirty-normals-and-retained-lighting)
explain both retained edge normals in the captured original water update.
Faithful water now replaces only admitted vertex normals and derives lighting
from the retained or replaced values. The omitted diagonal retains its base
normal and RGB unless another selected water vertex admits it. World changes,
camera-cell changes and Modern-to-Faithful switches restore base normals before
admission; shared source meshes remain unchanged.
**Verified:** the [terrain window](https://github.com/michidk/redguard-preservation/blob/0913cd1/docs/engine/water.md#terrain-window)
uses nearest-even camera-cell rounding, admits the inner 32×32 vertices and
submits their 31×31 cells. Tests cover row reversal, boundary ties, admission
changes without clock advancement, and retained lighting. Wave lookup rounds
the angle to single precision before sampling.
**Verified:** matched original/native/browser ISLAND shoreline captures at 640×480
use zero-speed waves to hold phase constant. Edge texture/color differences
remain; default-speed timing and complete pixel parity are **Unknown**. Candidate face normals are still
calculated globally before selective vertex replacement.

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
Sun alpha uses the same staged integer interpolation and wrapped texture loads,
following preservation's [sun filtering](https://github.com/michidk/redguard-preservation/commit/306bdf59a3a968dcec73848f24db6c7cab7bda9d).
With Faithful Sky and Fog, the sun reads truncated RGB565 destination channels,
subtracts dithering and blends separately truncated source/destination terms,
following the [sun blend contract](https://github.com/michidk/redguard-preservation/commit/421912a01adfbce8c6116fc4b1892cbdc77551ea).
Modern Sky retains hardware alpha filtering; either Modern Sky or Modern Fog
retains continuous blending. Modern Fog continues to bypass framebuffer
quantization. **Verified:** original/native/browser ISLAND captures use the same
viewpoint and 640×480 render size; the native/browser build sources match.
Cloud phases are unsynchronized and visible terrain/sky differences remain.
Other texture filtering, lighting, menu-to-world transitions and physical-hardware
pixel parity remain unverified.

Ordinary archive-model and terrain texture playback use preservation's
[shared frame and admission rules](https://github.com/michidk/redguard-preservation/blob/af411516a872f56afc3b164a384ab782f824a6fe/docs/engine/texture-animation.md).
**Verified:** renderer tests cover shared deadlines, late admission advancing only
once, retained hidden frames, authored facing and clipping before raster area.
Terrain requests use the camera window and each triangle's face normal, then
join the same image timeline as models. Their admission survives frames that
reuse prepared geometry and does not depend on visible raster area.
Materials share image state by canonical image name while retaining their own
exported frame handles. The GLTF loader retains all authored frames during its
initial decode; frame selection does not reload the source asset. World replacement
clears that world's playback records.
Only known ordinary ROB draw attributes participate. Script-bound actor admission,
explicit frame overrides and generated billboards remain
unsupported. **Verified:** original/native/browser ISLAND river captures share the
camera pose at 640×480; successive native/browser samples change the water-rock
foam textures. Separate zero-wave-speed native/browser samples also show terrain
texture advancement with fixed geometry. Native/browser builds share their source snapshot. **Unknown:**
synchronized animation cadence, transition parity and complete pixel parity;
visible terrain/sky differences remain. This does not complete authored texture playback.

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

The F12 console renders installed `SYSTEM/PAPER.GXA` and the configured bitmap
font through the shared game editor. **Verified:** presentation inputs, palette
ink selection and editing rules follow the published [console contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/console.md).
The header identifies cyrus instead of displaying original-engine memory totals.
Exact pixel parity and unimplemented diagnostic views remain outside current coverage.

The public `ReadyScene` resource identifies the prepared world whose render instance has finished spawning. Capture consumers compare its animation handle with the current `PreparedWorld`; this does not establish script or gameplay readiness.

Console rendering consumes CPF paper, GUI color and shadow settings and the
shared cursor clock. `show world`, `fxskyscale`, `fxskylevel`, `fxwaveamp`,
`fxwavespeed` and `fxwavefreq` use the current environment. Edits are retained
for world revisits within the session, outside the read-only install. Wave
changes rebuild the private terrain meshes and bounds. Other environment/light
editor commands remain explicitly unsupported. Re-export older console artwork
caches to provide the complete world palette and GUI color mapping.

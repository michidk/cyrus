# cyrus-render

Native/browser presentation: scene graphics, environment, mesh poses, inspection
camera, HUD and subtitles.
The game camera explicitly disables MSAA. Sky materials retain encoded-color
filtering and fog without adding a software RGB565 framebuffer dither pass.
The game camera keeps the original horizontal field of view when its viewport
resizes. **Verified:** preservation's [Glide projection](https://github.com/michidk/redguard-preservation/blob/6520206/docs/engine/projection.md)
defines its scale. The original follow camera and matched-view pixel parity
remain unsupported and unverified, respectively.
**Verified:** the current default START camera shares the player's transform and
intersects the animated CYRUS mesh, exposing a moving outline above the ship rail.
An explicit inspection view outside the player avoids that symptom; it does not
implement the original follow camera.
Effects, music and speech playback live in `cyrus-audio`.
Depends on game/world/assets; none of those crates depends on presentation.
Production rendering has no `cyrus-audio` dependency. Native host composition
translates speech requests into `subtitles::SubtitleRequest` before
`UpdateSubtitles`; the renderer owns text timing, slots and drawing. Game hosts
select the view camera for audio listener updates. The isolated consumer check
rejects audio or startup dependencies in rendering.

`WorldRenderPlugin` composes the shared scene-spawn path, environment,
`CameraPlugin` and `AnimationRenderPlugin`. Native and browser hosts submit the
same `PreparedWorld`. Core-only and native headless assemblies omit this plugin.
Script-driven actor poses retain the same scene origin as exported world placements and markers.
The framed inspection camera supplies `ScriptView` before ordinary actor updates.
Script visibility flags hide or restore bound scene nodes independently of actor
update eligibility. Static transforms continue to use the logical actor matrix.
HUD remains a separate plugin; startup is composed by the host. Hosts install `cyrus-audio` separately;
headless assembly keeps sound control state without Bevy's audio backend.
HUD artwork and rasterized text explicitly use nearest sampling when the
640×480 overlay is enlarged, preserving its pixel edges instead of inheriting
the image plugin's linear sampler. World materials retain their own sampling rules.
Model lighting, compositing, palette-key and texture parameters share one uniform
buffer; sky parameters share another. This keeps both materials within the Windows
WebGL fragment binding budget.

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
colors. The recovered background fact is [published in preservation](https://github.com/michidk/redguard-preservation/pull/24).

**Verified:** water uses preservation's [water documentation](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/water.md):
selected-corner mask, terrain-centered phase, quarter-shifted rotation table,
and half-amplitude bias. Playback uses real elapsed time at the BIOS timer rate, preserving its clock across scenes.
It deforms private copies of exported terrain meshes and recomputes shared
normals across materials. Re-export worlds to replace old water masks.

Absolute DOS clock phase is not synchronized. Terrain placement and base heights
retain the preservation export's existing limitations.

**Verified:** terrain ambient/directional inputs, coordinate conversion and
quantized shading normals follow preservation's
[terrain-lighting contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/terrain-lighting.md).
Native export retains the per-world inputs in `.environment`; native and browser
playback shade private terrain meshes, including displaced water. Terrain uses
screen-linear vertex RGB and texture modulation before the existing scene fog,
without Bevy PBR or tone mapping. **Verified:** terrain texels use the world's six-bit palette conversion before
filtering, following the [published COL upload rule](https://github.com/michidk/redguard-preservation/commit/395e6693514482fb4a8c8881d5998843efee3542).
Generic exported images retain their original colors. Ordinary prepared models
apply the same world palette conversion in their sampling path. Standalone-model palette handling remains **Unknown**.
**Verified:** measured ISLAND original/native/browser captures with frozen sky
and waves and held per-image frames reduce shoreline mean RGB error from 1.281
to about 0.096 and land error from 1.405 to about 0.030. A control restoring only
the old sampler reproduces the previous error. Native/browser region differences
stay below 0.003. Full pixel parity and real-time cadence remain **Unknown**.
Terrain sampling uses encoded palette channels,
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
absent ROB provenance and their previous sampling. **Unknown:** standalone-model
sampling defaults. Placed flats select point sampling.
**Verified:** known ROB and placed-flat paths compare filtered encoded RGB with
the effective palette-zero RGB before vertex lighting, following preservation's
[texture-key contract](https://github.com/michidk/redguard-preservation/commit/7d7c0f38a7fc30dcfd93c4f045df581673e42902).
They do not use interpolated texture alpha as a cutoff. Solid-color faces bypass
the texture-key test. Required version-1 `rgpre_texture` material metadata retains
the selected key; missing or malformed metadata rejects the affected mesh with
re-export instructions. **Verified:** preservation's [draw-flag ownership](https://github.com/michidk/redguard-preservation/commit/2292a631df72c5dd80625340b39d93400a4638e3)
distinguishes animated actor group initialization from static archive submission.
A missing first-group archive entry preserves initialized actor flags; ordinary
static submission refreshes flags from its selected archive record. Applying
actor flags requires authoritative host model-path and first-group state, rather
than treating every absent export attribute as zero. Later actor overrides and
generated-model defaults remain **Unknown**.

**Verified:** model ambient/directional shading follows preservation's
[model-lighting rules](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/model-lighting.md).
Models use the installed world RGB, authored face brightness and normals, and
solid-color tint. File-model brightness uses preservation's
[stored-to-runtime conversion](https://github.com/michidk/redguard-preservation/commit/87a19a3c3a07f7b44d91fdcc808255201b4cd9de).
Version-3 caches contain unconverted brightness and require re-export, including
player models. Version-5 exports also retain [all referenced authored corner normals](https://github.com/michidk/redguard-preservation/commit/a2ea54e358679cbe35c555aad3fb9518068d41dc), including normals beyond the position count. Re-export version-4 caches to replace substituted face normals. Generated-flat brightness remains a direct runtime input. RGB is calculated at vertices, interpolated in screen space
and combined with encoded texture colors before fog. The fixed Bevy directional
light and model PBR response are removed. Mesh transforms and explicitly selected
animation frames feed the same shader on native and browser builds.

**Verified:** ordinary ROB models clip lit, unweighted RGB and UV in camera
space before projection, following the published
[model clipping rule](https://github.com/michidk/redguard-preservation/commit/dd8c5f8b10a0cdaeaaa33f0aedcb5a177a98ed2a).
Separate draw meshes preserve the source topology used by animation and texture
admission. Generated flats retain their separate rendering path.
Matched 640×480 native/browser ISLAND captures reduce shoreline-region mean RGB
error from 3.86 to 2.05 against the saved original viewpoint and prior-version
control. Sky phase and the selected foam frame are held; the original water
field is not synchronized. The original capture follows a book-menu return;
these native/browser captures enter the world directly.

Ordinary clipped-model draw meshes use the published
[projection quantization rule](https://github.com/michidk/redguard-preservation/commit/c91863c44fafaf212e7b9928a3331c8242049cc1).
Prepared screen coordinates lie on a 1/16-pixel grid and are recomputed when
the physical viewport changes. Authored source geometry remains available to
animation and polygon admission. Generated flats retain their separate path.
**Verified:** the shared projection calculation matches original-instruction
outputs for synthetic clipped polygons. Prepared ordinary models also use the published
[fixed color-gradient setup](https://github.com/michidk/redguard-preservation/commit/8e504c11309a9daf5cb33ac743d602ce4baf875e),
including screen-Y anchor selection, subpixel adjustment and integer encoded
texture modulation. Fixed texture-coordinate sampling is described below;
full pixel parity remains **Unknown**.

Re-export worlds and player models for the `rgpre_lighting` primitive metadata
and required environment model inputs. Invalid mesh lighting metadata hides the
unsupported mesh and logs the world/mesh context with re-export instructions.
**Verified:** static MPSO instances bake retained RGB when the world has admitted
map lights, following preservation's [world prelighting rules](https://github.com/michidk/redguard-preservation/blob/ddbe29f/docs/engine/model-lighting.md).
Private instance meshes retain signed local-light contributions, quantized falloff,
authored matrix/bound selection and nearest-even channel rounding. Palette tint
precedes rounding and is not applied twice. Map lights accumulate in the original
spatial-cell order and are not reapplied every frame. Re-export worlds for the
required environment placement records and version-5 model lighting metadata.
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

Placed MPSF flats use private, centered camera-facing meshes sized from exported
source-image dimensions and scale. **Verified:** preservation's
[flat drawing and export contract](https://github.com/michidk/redguard-preservation/blob/2a44319/docs/engine/model-lighting.md)
supplies the geometry and alpha-dependent face brightness. Re-export worlds for
`rgpre_flat` metadata. Centered flags 1 and 3 support source images without a 64-texel size cutoff,
following preservation's [flat dimension rules](https://github.com/michidk/redguard-preservation/commit/812dfb7).
Configured texture reduction remains unsupported; the installed 3dfx factors `1,1`
retain full-resolution textures. Rejected flats are hidden;
preparation groups matching failures instead of logging each placement. Re-exporting
valid data does not enable unsupported renderer features.
**Verified:** matched 640×480 BELLTOWR native/browser captures replace the oversized
inverted triangles with a small upright flame at the original sample's location;
the previous geometry fails the measured flame-region comparison. Constant-alpha compositing uses a framebuffer copy per admitted flat, strict
depth testing and retained depth writes. It follows preservation's
[integer blend rule](https://github.com/michidk/redguard-preservation/blob/abd8576/docs/engine/model-lighting.md#flat-framebuffer-blending).
Fog precedes the blend, and RGB565 quantization follows it. Billboard-local normals
and light coordinates follow the [generated-flat lighting contract](https://github.com/michidk/redguard-preservation/commit/999c1e73ba07f86dc2dbbd8c0a72cd754a32cdba).
Ordinary flats use the [spatial-list and alpha-partition order](https://github.com/michidk/redguard-preservation/blob/a9e6b62/docs/engine/model-lighting.md#flat-submission-order),
with the inspection camera selecting the region. Excluded retained draws return
when admitted again and still participate in material removal/respecialization.
Scripted/translucent model ordering, synchronized phase and full pixel parity
remain **Unknown**. Placed flats use the recovered
[quantized sprite bound and first-polygon admission](https://github.com/michidk/redguard-preservation/blob/4b1f28b/docs/engine/texture-animation.md#placed-flat-admission)
to request automatic frames from the shared image timeline. They bypass model
face-normal rejection, retain point sampling and require at least three corners
after clipping. Their frame requests do not depend on rasterized pixel coverage.
**Verified:** matched BELLTOWR original/native/browser views, twelve native samples
and twelve browser samples show all five authored flame states.
The original was paused after loading artwork cleared; its texture index and
synchronized cadence remain **Unknown**.

Terrain prepares private triangles after wave updates and camera projection.
It clips camera-space position, RGB and UV together, snaps projected vertices,
and sends each fan triangle's fixed color and texture coefficients to the shared
shader. Original texture coordinates survive clipping before perspective weighting;
the shader evaluates signed fixed-point planes using pairs of 32-bit words.
Terrain fog uses the same stepped reciprocal-depth plane as texture sampling,
following preservation's [fog interpolation rule](https://github.com/michidk/redguard-preservation/blob/e8588b5/docs/engine/fog.md).
Camera movement, viewport changes and clipping-range edits rebuild these meshes;
shared source assets remain unchanged. The encoded texture combiner consumes
integer lighting channels before scene fog, following preservation's
[terrain preparation rules](https://github.com/michidk/redguard-preservation/blob/449ba2a/docs/engine/terrain-lighting.md).
Fog changes update materials referenced by live scene entities; retained assets
from departed worlds do not trigger new GPU uploads.

The Lighting fidelity area defaults to Faithful. Modern lighting is a placeholder
that reuses Faithful results. Existing Water and Fog switches remain independent.
**Verified:** Faithful terrain maps the export's logical tile corners to the
original [inset texture range](https://github.com/michidk/redguard-preservation/blob/aa70734/docs/engine/terrain-lighting.md#texture-coordinates-and-setup)
before clipping. U spans `1/256` to `254/256`. The export's vertically flipped
images require V endpoints `2/256` and `255/256`. Modern lighting shares this
terrain preparation.

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
inputs. **Verified:** matched coastal ISLAND captures cover the complete sun disc
with frozen sky phase. Scrolling advances texture coordinates
on the documented BIOS timer, using the per-world speed and a random initial
direction. `SkyScrollSync` and MCP `set_sky_scroll` accept a captured direction
and texture offsets. Browser [capture mode](../cyrus-web/README.md) accepts the
same measured phase after world entry. The original session's shared random
stream is not reproduced.

Faithful cloud coordinates undo the scene export's Z-origin shift before using
the original camera position. This combines preservation's
[RGM position conversion](https://github.com/michidk/redguard-preservation/blob/main/docs/formats/RGM.md)
with its [sky texture mapping](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/sky.md).

Scene rendering uses the installed near/far planes, including a nonzero
`world_back_plane` override. F1 exposes a render-distance override in game units;
editing it switches Fog to Modern. Faithful uses the original range and fog
tables. Modern Fog expands the camera-centered terrain and water window to cover the
selected far frustum. Faithful Fog retains the original terrain window.
Modern Fog defaults to four times the far distance and a gradual distant
fade. Modern Sky is camera-centered. Fog and Sky are independently switchable
and default to Faithful. Water has two mod hooks instead of a mode: `WaterWaves`
takes per-vertex heights in place of the original wave for each frame a provider
answers, and the `water_surface.wesl` library (texture offset and final colour)
keeps the original look until a mod replaces its source. `CameraTakeover`
drives the view camera for a frame and suppresses player input. Re-export environments after this change.
The fog curve follows preservation's [fog documentation](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/fog.md).
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
The coastal comparison freezes cloud phase and follows an original book-menu
return; native/browser use direct world entry. Sampled sun and cloud errors
average below one RGB level. Terrain/model shading, remaining texture sampling,
complete cross-world menu transitions and physical-hardware pixel parity remain
**Unknown**.

Ordinary archive-model, placed-flat and terrain texture playback use preservation's
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
Archive models require known ordinary ROB draw attributes; prepared MPSF flats
use their separate first-polygon path. Script-bound actor admission and explicit
frame overrides remain unsupported. **Verified:** original/native/browser ISLAND river captures share the
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
F8, Tab and F7 preferences survive native restart and browser reload. Placed-player
heading/world and supplied script-player health/type and activation lockout feed
the overlay. Installed bitmap/logbook script handlers feed shared game-owned
slots and accepted notification requests. New logbook notifications draw frame 0
first; entries accepted during playback do not restart the icon. Lockout hides
power-up slots and leaves the logbook icon visible. Owned compass use consumes the
installed Activate/Attack binding once per hold, with supplied actor
gates. Ordinary use derives them from the logical player; missing actor state
rejects use. Script acquisition, selection, item cycling, counts and compass
ownership share game-owned session inventory. The display flag and inventory
survive HUD/world replacement. Acquisition side effects, save restore and
new-game/death resets remain unsupported.
Automatic weapon-draw admission and gameplay walk-mode/auto-defend behavior remain unwired. Native
health-number placement matches the original at 640×480; complete pixel and timing parity remain open.

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
`fxwavespeed`, `fxwavefreq` and `fxskyspeed` use the current environment. Edits are retained
for world revisits within the session, outside the read-only install. Wave
changes rebuild the private terrain meshes and bounds. Other environment/light
editor commands remain explicitly unsupported. Re-export older console artwork
caches to provide the complete world palette and GUI color mapping.

**Verified:** `fxskyspeed` follows the published [signed live-speed contract](https://github.com/michidk/redguard-preservation/commit/8cf451e8c3bd0ed02d57855de1544b7e63348ce3).
It consumes one shared random draw even at zero, preserves current offsets and
timer state, and retains the low byte for later world loads. Initial sky creation
still chooses an independent random direction; complete session random ordering
remains unverified.

**Verified:** terrain and model filtering preserve original four-bit vertical
weights after the export reverses image rows, following the published
[filter orientation rule](https://github.com/michidk/redguard-preservation/commit/e4408c6a886c2336cd960ff2a4bc5eb4f6200098).
The production sampler matches compiled-original interpolation on synthetic
wrapped texture samples. Prepared terrain uses the published
[fixed-plane and reciprocal contract](https://github.com/michidk/redguard-preservation/blob/837f22c/docs/engine/terrain-lighting.md)
and selects four-bit texture samples in integers before float normalization and
row reversal. A water-surface mod's texture offset applies after this selection.
Original-instruction comparisons cover CPU setup; Vulkan checks cover the
production shader arithmetic. Full pixel parity remains unverified.

`fog::TextureAnimations` exposes loaded shared frame state and validated
inspection holds for matched captures. Holding a frame suppresses automatic
requests for that image until resumed; selection preserves its deadline.
World replacement clears the hold with the image timeline. Ordinary playback
has no hold. Explicit gameplay frame requests remain unsupported.

Texture and terrain playback expose opaque replacement snapshots for session
assembly. Capture follows completed binding and deferred render writes, together
with the presentation clock. Texture replacement validates the prepared image
census and retains frames, deadlines and holds. Terrain replacement validates
environment and mesh inputs, retains historical normals and admission, and
returns private mesh replacements with explicit fresh entity bindings. Construction
does not change live assets or advance playback. The session owns complete source
validation and coordinated publication. **Verified:** headless continuation and
rejection contracts compare retained state and visible mesh attributes across
clock rebasing and camera changes. Native/browser save integration remains open.

The HUD reads current logical-player health before advancing its candle animation.
Candle animation uses a 60 Hz presentation timeline independently of host frame
rate, following the published Glide retrace ceiling. Candle visibility, notices and activation prompt/item countdowns use
the shared simulation clock's nominal ticks, including its startup warmup.
The console suspends HUD animation, timers and toggle input.

The HUD reads dialogue text selected by the game-owned record update.
It uses installed REDSEL data, font_norm alpha for player record zero,
font_sel alpha for other records, and the fixed 640×480 subtitle band,
without wrapping again or advancing lifetime. The sound subtitle preference
controls visibility. Automatic dialogue update
and source scheduling remain separate game/audio work.

**Verified:** ordinary prepared models retain source-pixel UVs through clipping and submit fixed
texture planes. Their texture samples use the converted world COL palette before
filtering, including black index-zero texels and allocation padding, following
[preservation palette ownership](https://github.com/michidk/redguard-preservation/commit/fe530cc).
Their shader samples the original reduced allocation with
palette-zero padding and wrapping, using preservation's
[texture layout](https://github.com/michidk/redguard-preservation/commit/85afd92)
and [address selection](https://github.com/michidk/redguard-preservation/commit/955cc9f)
rules. Source images and shared animation frame handles remain unchanged.
Re-export environments for texture reduction settings and model GLBs for
`rgpre_texcoords` metadata. Missing metadata hides the affected model with a
contextual error. Generated flats retain their separate coordinate path.

**Verified:** matched 640×480 START captures use native New Game and browser menu
Load entry, the measured camera, frozen sky/waves and a held water frame. The
browser Load action starts the same fresh world; it does not restore a save.
Converted model palette samples reduce lower-deck mean RGB error from 1.179 to
0.038 against an isolated old-sampler control. Native/browser deck and rail
differences stay below 0.002. Native BELLTOWR controls improve sampled wall and
table colors; the measured ISLAND terrain regions are unchanged. Full pixel
parity and original cadence remain **Unknown**.

**Verified:** native and browser START New Game preserve the measured inspection
view after the book menu and intro. At zero sky/wave speed, measured sky phase
and held foam frame, browser deck, rail and sky pixels match direct entry.
The original reference returned from its book menu. Residual shading, original
water-field synchronization and complete transition parity remain **Unknown**.

**Verified:** saved native/browser builds at `084466d` match the measured original
ISLAND dry-ground viewpoint at 640×480 with frozen sky/wave speed. Two ground
regions average 1.489 and 1.215 RGB levels of error against the original; an
isolated incorrect-filter control raises them to 3.257 and 3.446. Native/browser
pixels match throughout the lower region and differ by three channel levels in
total in the larger region. HUD and water are excluded. Residual terrain error,
original animation cadence and complete transition parity remain **Unknown**.

**Verified:** saved `084466d` native ISLAND entry from the ready book menu, via
scene control, preserves the measured coastal sun/cloud view. Against the original
menu-return reference, sun error averages 0.023 RGB levels and cloud error 0.300;
a zero-alpha sun control raises sun error to 25.035 while leaving clouds identical.
Matching browser direct entry averages 0.022 and 0.299. The browser debug world
picker reloads the page and clears the inspection pose, so it does not establish
an in-process menu transition. Browser coastal menu-entry parity remains **Unknown**.

**Verified:** a retained zero-speed ISLAND shoreline view separates texture-state
differences from backend differences. Holding all four visible animated terrain
images to the same recorded frame gives native/browser mean RGB differences
below 0.003 in water, edge and land regions, with maximum channel difference one.
An eight-frame browser sweep changes original-relative water error from 1.715 to
10.088 while dry-land error stays 1.404. Original per-image indices were not
recorded; selecting the lowest error does not establish matching phase or parity.

**Verified:** placed flats point-sample the converted world palette and reject
black index-zero texels using the published [palette ownership rule](https://github.com/michidk/redguard-preservation/commit/fe530cc).
All five held BELLTOWR flame frames improve against the retained original and
an isolated old-sampler control. Frame 2 mean RGB error is 0.733 native and
0.725 browser, versus 0.837 for the control. Wall/table pixels are unchanged
by this correction. Original flame phase, cadence and full parity remain
**Unknown**. Standalone actor sampling eligibility remains separate.

**Verified:** world archive solid-color faces require `rgpre_solid_palette` version 1 from
[preservation's indexed export](https://github.com/michidk/redguard-preservation/commit/395b1b3).
Their palette conversion precedes retained lighting and unbaked corner lighting;
index zero becomes black. Missing metadata requires re-export. Generic model
exports keep their existing colors.
**Verified:** measured 640×480 BELLTOWR original/native/browser captures cover the
upper tower windows and a close tower ring. Against an isolated old-conversion
control, mean RGB error drops from 0.810 to zero in the sampled window band and
from 0.29–0.33 to at most 0.002 in four ring regions. Native/browser ring regions
match exactly; sampled textured wall and beam pixels are unchanged by the fix.
All five held candle frames preserve the previous browser flame/wall/table pixels.
These static views do not establish full-scene parity, actor preparation rules or
synchronized animation cadence.

Ordinary actor poses that select another frame source use the same model replacement
owner as the player. Map-local poses retain their original geometry. Replacement
hides the original primitives and restores their visibility when the actor returns
to its map-local source or loses its pose. Shared source meshes remain immutable.
Model snapshots require settled mesh frames. Replacement candidates compute private
frame geometry off-live and publish it with the pose, without waiting for the next
render update. Installed rendered continuation is still awaiting acceptance.

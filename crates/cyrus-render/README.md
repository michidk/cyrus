# cyrus-render

Native/browser presentation: scene graphics, environment, mesh poses, inspection
camera, startup/book menu, HUD, effects, music, speech, subtitles and cinematics.
Depends on game/world/assets; none of those crates depends on presentation.

`WorldRenderPlugin` composes the shared scene-spawn path, environment,
`CameraPlugin` and `AnimationRenderPlugin`. Native and browser hosts submit the
same `PreparedWorld`. Core-only and native headless assemblies omit this plugin.
Sound, HUD and startup remain separate plugins. Native headless assembly keeps
sound control state without installing Bevy's audio backend.

Menu edits persist before applying. Audio preference application precedes
`AnimationSystems`, which precedes cue consumption in the same Update. Mesh poses
apply in PostUpdate. Native media workers and browser cooperative decoding retain
their existing cancellation, seeking and menu-return behavior.

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
without Bevy PBR or tone mapping. Models retain the existing fixed Bevy light.
Re-export browser worlds to include the new lighting inputs.

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

Default windowed startup plays the installed Windows Glide swoosh animation and decoded
640×480 title image, then opens the book menu once required assets are ready.
Native menu/HUD export begins after the title becomes visible. The full
parser-coverage scan remains in direct/headless inspection launches.
Loading stays asynchronous; skipping the splash retains required preparation.
The title uses the configured FNT font and documented bottom-label positions.
The loading label says “Loading...” rather than reporting original map IDs.
**Verified:** the splash resource supplies 90 timed frames at 30 fps, followed by
the authored fade. The cache regenerates when replacing the older DOS shield
splash. The [preservation contribution](https://github.com/michidk/redguard-preservation/compare/main...fix/gog-splash-resource)
documents the driver variants and DLL format. Splash rasterization uses
supersampling; Glide edge coverage, floating-point approximations and pixel
quantization need a hardware comparison before claiming pixel-exact rendering.

The book menu derives entries, layout, texture slots, widgets, and action IDs
from `MENU.INI` through rgpre, following preservation's [book-menu rules](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/menu.md).
Its portable `cache/menu/book.menu` retains all authored model frames and topology,
textures, indexed fonts, previews, and defaults. Native and browser builds project
the current book pose and redraw neighboring-page text during camera entry, page
turns, and Quit closing. Input waits through the final settling update.
Navigation skips disabled/hidden entries and wraps. Options opens Display, Sound,
and Controls; Movies browses unlocked definitions and previews. Native Quit exits
after the authored close. Browser Quit requests tab closure and leaves the menu
running if the host refuses.
Native Movies playback reads installed Smacker files or the GOG CD image via
rgpre and decodes them in-process with the `smk` crate. It retains menu selection, consumes original
skip points and timed subtitle definitions, and exposes progress through menu
control state. Browser Movies uses locally exported compressed video, first-track
WAV audio, subtitle definitions, and the configured font, downloaded on demand.
Native and browser playback share skip points, subtitles, interlace and menu return.
New Game enters the installed starting world and marker. Load temporarily starts
the same fresh world. Shared world entry waits for preparation, required asset dependencies and
spawned scene instances behind the destination's installed GXA artwork, with preservation's
red bar and REDSEL percentage label. Progress counts completed loading stages.
The [preservation contribution](https://github.com/michidk/redguard-preservation/compare/main...feat/menu-game-entry)
supplies the entry parser and loading layout. Saves, full story/gameplay
initialization and progression unlocks remain unavailable.

`MenuPreferences` stores original action units and fifteen binding codes in
native `cyrus-config/menu.json` or browser localStorage. It validates local state,
saves before applying edits, and leaves malformed files intact. Existing player
input uses edited bindings, including direct scene launches. Subtitle and interlace
settings apply to cinematics; saved Sound preferences apply to native
effects, speech, and CD music. Book motion and direction/capture feedback dispatch
installed effects through the shared native/browser sound path, following
preservation's [cue rules](https://github.com/michidk/redguard-preservation/pull/34).
**Verified:** the original default menu has no pointer hover/click selection; mouse/joystick
rebinding, lighting, camera-plane clipping and gameplay thumbnails remain unsupported. See [SETUP.md](../../docs/SETUP.md) for controls and limitations.

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

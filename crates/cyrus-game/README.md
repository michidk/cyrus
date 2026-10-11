# cyrus-game

The game as Bevy plugins, with no assumption about a window or renderer.

| Module | Role |
|---|---|
| `install` | Locate and validate the player's GOG install; read-only. |
| `startup` | Shared splash/title and data-defined book menu; native exporter renders the installed Windows Glide splash DLL into a local image cache. |
| `loading` | Startup state: scans the install and parses every known file through `rgpre` behind a progress bar; the failures are the format-coverage report. |
| `scene` | Resolve a world from `WORLD.INI` (map, terrain, palette), export it to a GLB in `cyrus-config/cache/`, spawn it. Native only. |
| `player` | Held keyboard input from `KEYS.INI`, marker-based initial player placement, and a separate debug inspection camera. Locomotion and the original follow camera are unsupported. |
| `fidelity` | The switch between `Faithful` and `Modern` per `Area`; `in_mode` is the run condition. Defaults are faithful. |
| `environment` | Glide BSI sky scrolling and terrain waves in both graphics modes. Loads exported `.environment` sidecars. |
| `fog` | Original scene clipping, Glide fog tables, and a Modern debug distance override. |
| `sky` | Glide sky playback, material and shader, projected behind scene geometry. |
| `water` | Private terrain meshes, BIOS-rate waves and shared shading normals. |
| `lighting` | Original terrain vertex RGB and quantized normal calculation. |
| `animation` | Loads versioned `.animations` assets and applies explicitly activated forward groups and literal authored animation tasks to private meshes using supplied nominal ticks. No automatic actor activation or wall-clock driver. |
| `animation_export` | Retains RGM animation/script inputs, typed task descriptors and referenced model frames through rgpre. |
| `environment_export` | Native extraction of per-world sky pixels, terrain lighting inputs, wave parameters, and water masks. |
| `hud` | In-game overlay from exported `.hud` data, following preservation's [HUD documentation](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/hud.md): SNUFF candle with its show timer and health number, compass, logbook icon, power-up slots, active item, count and activation prompt, walk-mode and auto-defend notices, in the original draw order. REDSEL.FNT text through `cyrus-text`. F8, Tab and F7 toggles persist in `cyrus-config/hud.json` (browser localStorage `cyrus-hud`). Gameplay inputs (`set_health`, `set_sword_drawn`, `activate`, `show_bitmap`, `logbook_entry`, lockout, player type, heading, world, compass flag) have no caller yet. |
| `hud_export` | Native extraction of HUD positions, timers, toggles, starting inventory, WORLD.INI compass offsets, RTX notice texts, REDSEL.FNT and the candle, compass, logbook, power-up and item frames into `cyrus-config/cache/hud/`. |
| `settings` | `cyrus-config/settings.toml` and the cache directory. |
| `sound` | `SYSTEM.INI` and saved Sound preferences; installed effects, book/menu cues and authored animation cues through the original mixer gains on native and browser. Animation surfaces other than a known zero remain unsupported. RTX speech remains native only. |
| `subtitles` | Dialogue subtitles for voice clips through `cyrus-text`: REDSEL glyphs, 500 px wrapping, bottom band, black shadow, lowest active slot. Native only. |
| `music` | Plays the `world_redbook` CD track of each entered world from GOG's `game.ins`/`game.gog`, repeating it and keeping it across worlds that share it; the book menu plays track 2; the Music slider sets the drive level (preservation `docs/engine/music.md`). Native only. |
| `sound_bank` | Portable `SoundCue` message, `SoundSurface`, and the browser's exported effects bank; `sound` plays cues on native and browser. |
| `sound_export` | Exports MAIN.SFX to WAV files, bank metadata, and a browser audition page when exporting the menu or loading a native scene. |
| `control` | TCP control server speaking `cyrus-control`; used by tests and the MCP server. Native only. |

`startup` presentation, `animation`, `environment`, `fidelity`, `hud`, `player` and `settings` compile on wasm and are reused by
`cyrus-web`. All FNT text (book menu, title, movie and dialogue subtitles, HUD)
is laid out and rasterised by the `cyrus-text` crate; everything that reads the install, spawns threads or opens
sockets is gated with `cfg(not(target_arch = "wasm32"))`.

Tests in `tests/` use only the public API; the install-backed ones skip when
no install is linked and say so.

World exports include `.animations` sidecars containing actor animation groups,
RAAN model references, and decoded model frame positions and normals. Missing or
unsupported model references remain explicit in the asset's `unsupported` map.
Original animation payloads and the RASC, RAST, RASB and RAVA script inputs are
retained too. Preservation supplies typed literal task/End decoding; cyrus exports
portable program descriptors with names from the installed SOUP386.DEF.
Embedded ROB models are retained when matching archive payloads agree; conflicting
payloads remain unsupported rather than selecting an archive without evidence.
Native and browser scene loading retain this data. Re-export worlds for schema 2
and the GLB source vertex/face mapping. Older sidecars are rejected with a
re-export error. Explicit model poses update private meshes, normals, and bounds;
removing a pose or changing worlds restores the original mesh.

`AnimationPlayback` consumes an explicitly selected forward group and finite
loop count. `AnimationTick` supplies the original whole nominal-tick increment;
render updates alone do not advance playback. The interpreter follows
preservation's [animation facts](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/animation.md).
BreakPoint markers retain the documented walking-translation gate until the
cursor advances or a taken loop/exit jump clears it. DelayCounter holds the prior
pose while its byte delay consumes supplied nominal ticks. Zero continues
immediately and byte 255 waits until replacement. A waiting group remains current
and does not complete its PushAnimation task. These rules are
[published in preservation](https://github.com/michidk/redguard-preservation/blob/515e62f/docs/engine/animation.md#forward-animation-delays).
Unsupported cues stop playback with a contextual error.

`ActorAnimation` is the separate request core for forward types zero and one. The caller supplies the default group
and nominal ticks. Ordinary requests
replace one pending group by the original numeric priority; forced requests
bypass that priority. End selects a pending or default group in the same call,
reusing its tick input. `set_loop_count` supplies the finite count when a
PushAnimation task becomes current. Returning to the same default keeps the
group identity, so a finite count alone does not prove the waiting task completed.
Type-one groups keep their exit tail when the pending group is the default.
After an exit marker, a pending non-default can replace them on a later advance,
with delays checked first. These rules are
[published in preservation](https://github.com/michidk/redguard-preservation/blob/75cba23/docs/engine/animation.md#forward-type-one-transitions).
`request_with_sounds`, `force_request_with_sounds`, and `advance_with_sounds` accept a synchronous handler for
signed effect requests, rate offsets in hertz, End stops and replacement handle
resets. The handler owns the original surface remapping, positional gains and
channel allocation. Without it, sound commands still fail explicitly. Audible
original walking cues and automatic runtime scheduling remain unverified. The
[sound facts](https://github.com/michidk/redguard-preservation/blob/a0e7b16/docs/engine/sound.md#animation-playsound)
define this boundary.

`AnimationScript` supports authored literal `PushAnimation` tasks and `End`
continuations. Its explicit stages preserve the request, count-start and wait
phases; task completion can start the next instruction in the same stage. End
stores its target and yields, so a later eligible stage can restart the authored
sequence. Unsupported operands, tasks, interrupt entries and synchronization
channels fail explicitly.
`AnimationScript::advance_with_sounds` carries the same actor handler through
immediate task requests. Callback failures retain their script context and stop
later invocations from retrying the cue.

`AnimationPlayback::new_scripted` connects these cores to private rendered meshes.
Active group and scripted playback send authored cues through the shared native
and browser sound adapter in the same update. Without the adapter, sound-bearing
commands fail explicitly. The ground caller must supply the actor's known
`SoundSurface`; no surface or actor activation is inferred.
Supplied ticks run ordinary advancement/default request, then the script stage;
immediate script requests reuse the same ticks. This does not activate world
objects. Render updates alone do not advance either core.

Animated i16 frame
interpretation, mesh replacement across models, automatic actor activation,
complete script scheduling, state-dependent default selection, other group types,
cancellation requests and the host-clock driver remain
unsupported. These APIs do not establish matched original-game playback.

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

The debug flycam toggle moves only the view camera. Turning it off returns the
view to the player pose; it does not enable gameplay. Faithful
keyboard actions preserve held and opposing inputs. Modern input samples WASD,
Space, and held Shift run. Camera-relative movement and the mouse camera remain
pending. The deterministic `player::turn` and `player::walk` cores are
tested against original heading and animation-gated startup observations.
Walking supports actor step type 1, advances startup during suppressed
translation, and resets after an update without accepted forward movement.
An explicit ordinary grounded update now composes animation advancement/default
requests, forward support admission, walking translation, support-height assignment,
ordered wall response, residual separation, and startup reset. It preserves
separate animation invocations with the same caller-supplied nominal ticks.
Callers still supply eligibility, default group, oriented probe, candidates,
surface policy, and shared support state. Falling, water/lava transitions and
special movement states remain unsupported. Gameplay does not yet drive this
update; production timing, actor initialization and player mesh binding remain
incomplete.

**Verified:** the independent polygon-segment and ground-support selection cores
follow [preservation PR #27](https://github.com/michidk/redguard-preservation/pull/27).
They retain distinct terrain/model rank scales and replace earlier equal-ranked
contacts. Callers still supply admission, transforms and traversal order; these
cores do not activate gameplay collision. Nearest-even segment conversion follows
the documented default rounding evidence; a live halfway-boundary comparison
remains unchecked.

World exports also write a `.player` sidecar containing installed movement
settings, named CYRUS attributes and bounds, decoded terrain heights, and the
water mask.
The optional terrain texture grid retains packed image/rotation bytes for material
queries using the [published coordinate rules](https://github.com/michidk/redguard-preservation/blob/b2ddb1b/docs/engine/player.md#terrain-material-lookup).
Old caches require re-export for these queries. Explicit selected-support surface
queries apply the [generated terrain blend mask and water overrides](https://github.com/michidk/redguard-preservation/blob/94ea3ae/docs/engine/player.md#terrain-material-blend-coverage).
They do not assign actor surface state or the shared movement veto. Terrain water
height and buoyancy remain unsupported. Schema 2 also retains original static
placement order, model polygons, ROB collision flags and ordered subobject face
lists. Per-face texture set/image
identity is retained for surface lookup. The cache includes the installed
SURFACE.INI assignments, and textured contacts resolve their surface code.
Version-4/5 solid-color contacts resolve to surface zero.
Older caches without face textures or assignments cannot supply surface-dependent
behavior. Missing model-version metadata requires re-export.
Models resolve from the WORLD.INI-selected world archive in authored segment order. Missing models remain
explicitly unsupported. Authored version-4 subobjects are retained in the cache
but discarded during runtime preparation, matching the original loader.
Native, headless, and browser entry load the same validated asset.
Missing or unsupported actor definitions retain a contextual unavailable reason.
Loading also prepares static collision meshes and their spatial registry.
Explicit queries preserve view-centered cell order, separate actor-style
candidates, transform original polygons, and retain segment contact order.
Explicit static body probes test actor spheres against authored model faces and
return ordered contacts for response. Coincident subgroup centers and saturated
body-contact buffers remain unsupported.
Archive selection follows [preservation PR #38](https://github.com/michidk/redguard-preservation/pull/38);
segment admission and traversal follow [PR #41](https://github.com/michidk/redguard-preservation/pull/41).
Sphere model admission and forward slope/support selection follow the
[published player facts](https://github.com/michidk/redguard-preservation/blob/2810574/docs/engine/player.md).
The caller supplies the retained surface veto after support-state updates;
geometric support alone does not authorize forward movement.
When primary support is absent, the selected-player fallback uses the caller's
support-history state and retains the actor's runtime Y at the first accepted
non-side contact. This follows the [published fallback rules](https://github.com/michidk/redguard-preservation/blob/3c5be4a/docs/engine/player.md#selected-player-fallback-support).
Explicit selected-player queries also commit shared support history. Misses retain
metadata, so they can reset the fallback timer; temporary forward probes retain
those changes when position is restored. The caller supplies initial state and
nominal ticks, following the [support-state rules](https://github.com/michidk/redguard-preservation/blob/ed296ed/docs/engine/player.md#support-query-misses).
Ground support reads only face-model candidates. Actor-style obstacle response
remains unsupported.
This data does not activate ground settling or locomotion. Re-export cached
worlds after updating.

Scene inspection places the player at available marker zero once the world's
`.animations` data loads, using preservation's exact map coordinates. `MarkerEntry`
selects a different marker and heading for explicit entries. `PlayerPose` retains
runtime coordinates and the wrapped heading; the transform uses the existing
scene export coordinates. Heading-to-render rotation and ground settling remain
unsupported. The marker height is the initial height, not the settled ground
height. Re-export older assets that lack `map_position`.

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
640×480 title image, then stops at the book menu. Preload runs concurrently.
Timing is presentation policy; the original map-loading backend is not reproduced.
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
and Controls; Movies browses unlocked definitions and previews. Quit exits.
Native Movies playback reads installed Smacker files or the GOG CD image via
rgpre and decodes them in-process with the `smk` crate. It retains menu selection, consumes original
skip points and timed subtitle definitions, and exposes progress through menu
control state. Browser Movies uses locally exported compressed video, first-track
WAV audio, subtitle definitions, and the configured font, downloaded on demand.
Native and browser playback share skip points, subtitles, interlace and menu return.
New Game, saves, and progression unlocks remain unavailable.

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

# Accepted differences

Deliberate, user-visible differences between cyrus's defaults or options and a
particular way of running the original. Each entry names the original setup it
differs from, the reason it is accepted, and how to select the alternative.
Engine facts behind each entry live in
[redguard-preservation](https://github.com/michidk/redguard-preservation).
Unintended differences are bugs; report them in [GitHub issues](https://github.com/michidk/cyrus/issues), not here.

For normal controls and settings, use [Playing cyrus](PLAYING.md). This reference explains deviations and their evidence; it is not a setup checklist.

| Topic | Selected entries |
| --- | --- |
| Startup and menus | [Default startup](#default-startup-opens-the-book), [splash](#3dfx-splash), [title artwork](#title-artwork), [book rendering](#book-logbook-and-examine-are-drawn-by-the-gpu) |
| Timing and input | [Gameplay timing](#stable-gameplay-timing), [gamepad dead zone](#gamepad-dead-zone), [rope behavior](#rope-attachment-shortcuts) |
| Graphics | [Modern lighting](#modern-lighting), [effects](#modern-effects), [ray tracing](#ray-traced-lighting), [texture filtering](#optional-anisotropic-filtering) |
| Sound and movies | [Movie skipping](#skipped-movies), [CD music](#cd-data-noise-at-the-start-of-track-2), [resampling](#audio-resampling) |
| Optional mods | [Bundled mods and settings](../mods/README.md) |

## Stable gameplay timing

**Verified:** the original clock estimates frame scale from measured FPS, smooths
that estimate, and emits one nominal tick per update during its twelve-update
warmup, as documented in preservation's
[FPS-derived frame scale](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/player.md#fps-derived-frame-scale).
Cyrus uses elapsed active time for automatic gameplay at the installed nominal
rate. Scene entry, menu returns and resource preparation exclude suspended time;
FPS changes do not require a speed adjustment period. This applies by default in
both fidelity modes, as requested for consistent gameplay speed. The installed
minimum FPS still bounds movement during a single long host stall. Explicit
original-clock inspection and book animation retain the original clock API.

## Ambient sound loops at high frame rates

**Inferred** from the AmbientSound task contract and the clock above: the task forgets its
remembered sample slot, without stopping the sample, whenever its elapsed input
(`runs * frame scale >> 8`) reads zero. At the original's 12 to 20 FPS only a new task's
first run reads zero, and a new task has no slot yet. At higher frame rates several early
runs read zero and each would start another copy of the loop, so ambient water on the
opening ship stacked up. Cyrus forgets the slot only on an explicit sound reset, which
keeps the one loop per task that the original plays. This applies in both fidelity modes.

## 3dfx splash

**Verified:** the GOG Windows launch plays the metallic `3DfxSpl2.dll` splash. Emulated Voodoo hardware,
such as DOSBox-X with `glide=emu`, runs the DOS Glide overlay and shows an older shield animation instead
([startup facts](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/startup.md#splash-variants)).

cyrus plays only the Windows splash, because it is what GOG players see, and has no option for the shield.
Comparisons against DOSBox-X therefore show a different splash. The live meshes render at the
physical resolution of the window's fitted 4:3 viewport. Glide's edge-antialiasing path is not reproduced.

## Title artwork

The original shows the 640×480 `STARTUP2.GXA` picture after the splash. cyrus shows a higher-resolution,
16:9 redraw of it that ships with cyrus (`crates/cyrus-assets/assets/title.png`) and is not read from the
install. It scales to cover the window, so a window narrower than 16:9 crops the sides and a wider one crops
the top and bottom. The "Version" and status labels still use the installed font and palette, placed as in the
original on the artwork's centred 4:3 region; a window much wider than 16:9 can crop them.

## Skipped movies

**Verified:** New Game plays the introduction movie before entering the world, and scripts play
movies during the game. `--skip-movies` (browser `?skip-movies=true`) enters the world without the
introduction and resumes the world after a script movie without playing it. Movies opened from the
Movies menu and the ending movie still play. The default plays everything.

## CD data noise at the start of track 2

**Verified:** the GOG image places CD-ROM data-sector bytes at the start of
audio track 2. Played as audio, they make a short loud noise when the book menu
opens ([music facts](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/music.md#the-cd)).

Faithful music plays every installed byte. `--music modern` (browser
`?music=modern`, or the debug menu's Music fidelity) mutes the leading
sync-marked data sectors and keeps the track's length and timing.
**Unknown:** whether the original's playback path makes the noise audible.

## Modern lighting

The original lights models and terrain with vertex-driven shading. The `lighting`
fidelity area, Modern only (set by `--mode modern` or the debug menu; it has no flag of its own), switches
the world's own materials to physically based shading through Bevy's lights: a cascaded shadow-mapped sun,
the ambient colour as an environment light and the object lights as point lights, all read from the world's
installed lighting data. The lights are calibrated so a fully lit surface shows its texture colour, and
tonemapping is off so the sky, water and flats keep the original's colours. Models and terrain draw their
full world-space meshes instead of the original's screen-space prepared geometry, and they cast and
receive shadows (each caster through a shadow-only twin that shares one material, so
the cascades batch their draws, and static models with equal modern material values share one
material). The camera adds screen-space ambient occlusion, contact shadows, SMAA anti-aliasing and
bloom. The sun's shadows, the object lights, glossy water, ambient occlusion, contact shadows,
anti-aliasing and bloom each have a switch in the Lighting group of the debug menu's effects panel
(and the control port's `set_modern_effect`); all are on by default. Brightness, shadow distances, the point lights' power and the effects are tuning choices, not
measured matches. Flats keep their original shading.

Modern fog (`fog` area) thickens smoothly with distance and reaches the fog colour at the far plane,
which moves out to 50,000 engine units (thirteen times the island's installed 3,800), or to the world's
installed clipping distance where that reaches farther.

## Optional death presentation

The native `ragdolls` Wasm mod requests articulated physical presentation after a
committed death. Its authored rigs, gravity, joint limits and impact strength are
mod preferences. Enable it in the Mods window; it is disabled by default.
The host retains script state, gameplay collision and the player death controller.
Unsupported geometry retains the installed animation and reports a diagnostic.
See [mod authoring](../mods/README.md#death-observations-and-owned-physics) for
source-geometry, ownership and resource limits.

## Day/night cycle

**Verified:** the original has one fixed sun per world, read from its lighting data, and night is a
separate world variant. Under Modern lighting, the debug menu's day/night panel (and the control
port's `set_day_night`) can turn on a time of day. Noon reproduces the world's installed sun; the sun
moves on an arc through it and sets at 6 and 18 o'clock, and its colour, the ambient light, the fog and
the sky darken and warm with the hour, with a dim moonlight at night. The background beyond the scenery
darkens with the fog, and moonlit clouds stay visible at night. Worlds without an installed sun,
such as interiors, are not affected. The cycle is off by default, so the game keeps its own times of
day; once on, it runs one in-game minute per real second, so a day lasts 24 minutes. The night sky and
background are nearly black. While the sky is dark, the player carries a candle: a faint, flickering warm light follows
the player, the stronger the darker it is, and is out by day. Every colour, the speed and the direction of the arc are Modern tuning
choices.

## Modern effects

**Verified:** the original has none of these; they exist only under Modern lighting, are switched
one by one in the debug menu's effects panel (and the control port's `set_modern_effect`), and
every value is a Modern tuning choice:

- tonemapping (Khronos PBR Neutral by default) with exposure, saturation and contrast grading;
- automatic exposure, limited to one stop darker and one and a half stops brighter;
- god rays: the sun scattering in a thin volumetric haze around the camera;
- a lens flare and sun glare while the sun is on screen and not covered;
- a sky glow around the sun, a warm horizon while it is low, and stars when the day/night cycle
  darkens the sky;
- glossy water with screen-space reflections, which reflect the scenery on screen, else the sky
  drawn in the reflected direction, and the horizon colour where neither is on screen; open water
  reflects at most half its colour, so a sea seen at a grazing angle does not turn into the sky,
  while calm water models reflect fully;
- depth of field focused on the player, and subtle motion blur with a shutter
  exposure of 15% of a frame.
- the map's object and baked lights (lamps and fires) shine as point lights. Map light colours are
  signed and the original subtracts the negative channels; point lights only add, so each light
  shines with its positive channels and darkening-only lights are left out. Each lights a facing
  surface fully at a sixteenth of its radius, where the original's attenuation reaches one. Small
  models that hold a light glow on their bright texels;
- calm water: models whose texture SURFACE.INI marks as water reflect like the sea, without its
  ripples, where they face up; a waterfall does not mirror;
- with the `modern-water` mod, the sea moves in several crossing, sharp-crested waves that calm towards the
  shore, with ripples whose crests bend so they never line up into stripes. Foam breaks along the
  shore, running up and back with the swash while wash lines roll in, and the beach behind is darker
  and glossier where the swash wets it. The foam follows the waterline the terrain textures draw:
  the beach tiles SURFACE.INI marks as water draw it across the cell, and elsewhere it is the
  border between a water-textured cell and a land cell. The still water out to it reflects like the
  sea. The waves, ripples, foam and wet sand are tuning choices.

All of them start on.

## Unfiltered textures

The original renders through 3dfx Glide with bilinear texture filtering, which Faithful reproduces.
The `textures` fidelity area, Modern only (set by `--mode modern` or the debug menu), point-samples
world model and terrain textures instead, so the pixel art stays sharp. The sky keeps its filtering.

## Ray-traced lighting

The original has no ray tracing. Builds with the `raytracing` cargo feature (off by default, native
only: `cargo run -p cyrus --features raytracing`) add a **Ray tracing** switch beneath the lighting row
of the debug menu. It starts on and works only under Modern lighting, so it is greyed out in Faithful;
`--ray-tracing on|off` and the control `ray_tracing {enabled}` set it. On a GPU with ray queries it
lights the world with Bevy Solari instead of the raster shading above. Solari traces only plain PBR meshes, so each opaque
world mesh gets a traced stand-in in that layout while the original draw stops; flats, water, sky and
chroma-keyed meshes keep their original shading. The lights use the modern lighting's calibration, and
object and baked lights become small emissive spheres of the same power. **Verified:** the original adds each
face's brightness byte to its ambient light ([model
lighting](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/model-lighting.md)); the
drawn stand-ins give off their albedo times that brightness over 255 where they are seen. As Modern
tuning choices, lit windows and lamps (faces with brightness at least 96) also emit light into the traced
scene, 64 times what they are drawn with at night and eight times by day, while the walls' dimmer
visible glow follows the day/night sky tint. The traced image receives the
same distance fog as the meshes that keep their original shaders. It needs a GPU with ray-query support and is greyed out without one;
where it is unavailable, the `lighting` area applies. Traced sky light is occluded by the surroundings,
so the ambient light is raised while Solari traces it to keep shaded streets readable; the scale is a
tuning choice. Solari's output is noisy by design, so it runs with ReSTIR and raised sample counts and a
denoiser of our own, in the vendored `bevy_solari` under `dev/vendor`: it divides the albedo out,
accumulates the light over time against the previous frame's depth and normals, filters it with
edge-stopping a-trous passes and multiplies the albedo back in. It works on any ray-query GPU and needs
no NVIDIA SDK, but it treats all light as diffuse. History is dropped on camera jumps and world changes.
The vendored copy also fixes a compaction queue that prepared a replaced mesh twice. On an RTX 4070 at
1600x900 in ISLAND the denoiser cuts the mean frame-to-frame luminance change from about 2.4 to 0.3. The
debug menu's **Path-traced reference** switch (control `ray_tracing_reference`) replaces the real-time
lighting with Solari's path tracer, which converges while the camera is still, as a reference for judging
the real-time image; it draws over sky, water and flats.

## Credits scroll pace

**Verified:** the original scrolls the credits one pixel per buffer swap with swap interval one, and opens
its Glide window with the 60 Hz refresh setting
([credits facts](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/menu.md#credits-scroll)).
cyrus scrolls one pixel per 1/60 s on any display, which is the pace of a display that honours that
setting. **Inferred:** other displays and emulated Glide swaps pace differently (a DOSBox-X run scrolled
faster); the original's pace there is not a target.

## Rope attachment shortcuts

**Verified:** the attach test, the controls, the swing and the release follow preservation's rope sections.
**Inferred or simplified:** climb up repeats its group-82 request only while the animation's exit marker is clear, as preservation documents; the helpers' returned movement-suppression flag has no consumer here. The rope owner's downward step limit is the selected player's hand-height link choice computed by its controls on the same update; **Unknown:** whether the original recomputes it later in the frame, and a 16-unit lower bottom pose than the original's. The
implementation emits the swing sound owned by the first attached actor at channel index 6, as
preservation documents; native/browser audibility has not been compared. The hand-height link choice clamps against the floor probe as decoded; **Inferred:** the terrain sample
truncates like the queued clearance's, and the clamp has no live comparison with the original (the shipped
rope's room was not reached in the original). A host without collision data skips the clamp.
A hit on the hanging selected player lets go through
the same detach routine as the jump release, with no vertical key held; **Unknown:** whether the original reads
the held keys there.

## Ground prevalidation with one missing surface

**Verified:** when an actor's current and previous positions find support, or both miss, cyrus follows
preservation's ground prevalidation. **Unknown:** when exactly one of them misses, the original reads the
missing side's surface class from uninitialised stack memory, so its result is not defined. cyrus treats the
missing side as neither class 2 nor class 4: a missing current surface clears the results, and a class 2
current surface after a missing previous one runs the heading comparison.

## Island map screen details not recovered

**Verified:** the island map screen (map key or using item 83) follows preservation's
[island map screen](https://github.com/michidk/redguard-preservation/blob/main/docs/config/item-ini.md): the
gates, the marker position and scale per world, one pass over the cross frames at 100 ms and a key press to
close. **Unknown:** the original also switches the music mode while the screen is up (the routines it calls were
not decoded), accepts six game-controller button codes as keys (controller input is #71), and decodes both images
with the palette read last; cyrus decodes each image with its own palette. cyrus keeps the simulation clock
frozen for the screen's duration like the inventory screen does and does not switch the music mode.

**Verified:** map artwork includes the current world's zero-density integer fog step before RGB565 storage.
The original leaves framebuffer column 639 in the world fog-clear colour; cyrus covers the full viewport
with the installed map artwork. This single-column coverage difference is retained instead of adding an
empty edge to the native screen.

## Native catalogue and script coverage

**Verified:** `dev/scripts/native-audit.py` reads native declarations and decoded calls from
script exports. Its source and test matches are pointers for review, not proof that a handler
executes correctly. The historical 27-map export corpus contains decoded calls to 151 of the
367 catalogue names. Those counts describe that corpus; unsupported actor bodies and older export
schemas prevent an exhaustive claim about calls in the current install.

**Unknown:** a zero decoded-call count alone does not establish that a native is unused,
unrecovered or unimplemented. Some zero-occurrence names have documented contracts and supported
paths. Use `--behavior-matrix --include-unused` for explicitly reviewed scopes, owners, assertions
and acceptance limits. A reviewed row does not imply complete behavior or original/native/browser
acceptance. Remaining scope and acceptance belong to [the native audit](https://github.com/michidk/cyrus/issues/5)
and [SOUP tracking](https://github.com/michidk/cyrus/issues/4).

The completion requirement remains the behavior required by original scripts. A modified or added
script that needs an unsupported native first needs its behavior recovered in redguard-preservation.
Unsupported calls must fail with a contextual error naming the function; a source mention or static
absence cannot establish that failure contract for every catalogue entry.

## Killed created objects keep their record

**Verified:** objects created after world loading (`shoot`, `shootPlayer`, `DragonBreath`) are standalone
records with runtime identifier zero, outside the reusable actor pool, so no pool capacity limits them
([allocation facts](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md)). Each
world-loop iteration the original removes every created object whose killed flag is set from its lists and
frees the record. cyrus keeps a killed object's record in the actor table for the rest of the session: it
has no runtime, runs no script, draws no model and holds no light, sound or task, and updates skip it. The
actor table, spatial state and snapshots are index-parallel, so removing a record would shift every later
identity for no player-visible gain. **Inferred:** the observable result (no update, no model, no light, no
sound) is the same; only memory and one skipped table entry per update remain, a technical limit the
faithful mode does not reproduce. Direct actor handles and dereferencing script targets reject references to a created
object after cleanup with an explicit unsupported error; the retained diagnostic
record is not a live receiver. **Verified:** `address` reads return a logical actor
reference without dereferencing it, including a retired object's retained namespace.
Locals preserve that reference instead of converting it to `MyId`, and world restore
rebinds its generation. **Unknown:** the result of dereferencing the original's freed
record and reference aliasing after reusable-pool allocation, which remains unsupported.

## Book picture after the ending

**Verified:** after the credits the original's book shows its left-page picture frame filled with a flat
pink-red rectangle, with no gameplay snapshot. cyrus shows the book's empty parchment frame in the same
state (it takes no snapshot when the ending starts). **Inferred:** the flat fill is the original's
unpopulated picture buffer drawn through the book palette, not designed artwork, so cyrus does not
reproduce the colour.

## Save entry after the ending

**Verified:** in the same book the original leaves SAVE enabled (the selection starts on it, and Down moves
to LOAD, leaving SAVE black) and greys only CONTINUE; LOAD, NEW, OPTIONS, MOVIES and QUIT are enabled. cyrus
greys both SAVE and CONTINUE, because the ended world is discarded when the ending starts and the book opens
in its startup context, so the selection starts on LOAD. **Unknown:** what the original's SAVE stores or
reloads from that state; it was not exercised. New Game from this book starts the opening movie and then the
opening ship duel in both builds.

## Script movies run over the in-game book

**Inferred:** the original plays a gameplay script's movie directly from the main loop. cyrus enters the
in-game book context, plays the movie over it and resumes the world when it ends, so the book can show
for the moment before the first frame and effect sounds stop as they do for any book entry.

## Book, logbook and examine are drawn by the GPU

**Verified:** the original draws the book menu, the logbook scrolls and the examine view through Glide
with bilinear texture filtering. cyrus draws them as meshes through a perspective camera with the same
placements and camera paths. The book renders at the window's physical resolution, with the
authored 4:3 composition kept visible and the 3D view extended into the remaining window area.
The logbook and examine retain their 640 by 480 frame, scaled to the window.
Light levels follow the original's per-channel combine
(`floor(T * (L + 1) / 256)`, interpolated in screen space), applied in a shader. Page text, widgets and
pictures are painted into the page textures on the CPU, as the original does.

**Verified:** at 640 by 480, against the previous CPU renderer the settled book pages differ by a mean
of about 1.5 out of 765 per pixel (roughly 0.5 % of pixels visibly) and the logbook by about 0.3. The remainder is the
GPU's texture sampling and rounding, not different geometry. **Unknown:** whether the original's own
sampling matches either to the last bit; it was not compared pixel by pixel.

Dialogs over the book (delete confirmation, game description) are drawn on a transparent layer above the
scene, where the original composited them into the same frame. **Unknown:** whether the result is pixel-identical;
these dialogs were not compared against the previous renderer.
The Backtick flycam also flies the book's camera in the menu, which the original has no equivalent of; it is
off by default and a debug aid only.

The 3dfx splash is likewise drawn live on the GPU, as the original's Glide splash is, instead of
playing frames baked at install time. **Verified:** against the baked frames the best-matching live frames
differ by a mean of about 2 out of 765 per pixel. A draw with an alpha below 255 is
blended, in linear colour space rather than the original's 8-bit space; the installed splash draws every face
opaque, so this has no visible effect.

## Gamepad dead zone

**Verified:** the original polls a PC game port, reduces each axis to a loop count, compares it against the
centre read once at startup plus or minus the installed `joy_tolerance` (60 counts), and reads the four
buttons from the port. Each action reads its keyboard code or its joystick code, the latter from the second
`KEYS.INI` slot. cyrus maps a connected gamepad onto the same digital codes: the left stick or D-pad for
codes 136 to 139 and the South, East, West and North face buttons for buttons 1 to 4. **Inferred:** the loop
count depends on the stick's resistance, which a modern gamepad does not have, so cyrus converts
`joy_tolerance` to a fraction of the stick's range using the nominal 100 kOhm centre count of 574, a dead
zone of about 10 %. The Controls page capture accepts a gamepad face button as joystick button 1 to 4, as the original's capture accepts joystick buttons. The seven other
rebindable actions (next and previous item, inventory, log, quick sword, quick health, map) read their code the
same way, so a mouse or joystick code bound on the Controls page works for them. **Verified:** the original
reads every action through one shared input table (keyboard scancodes, mouse releases 128 and 129, joystick
codes 132 to 139). **Inferred:** cyrus treats a mouse release as a one-update press and a joystick code as
held, with its press edge taken from the previous poll. **Verified:** the inventory screen polls input once
per update before waiting for the next BIOS tick, so a mouse binding there reads the release between two
successive updates; cyrus does the same. **Unknown:** the dead zone a particular original
joystick showed, and whether the original's menus read the joystick; no joystick read was found in the menu
code.

A gamepad's other controls are an addition the game port could not offer, the same in every fidelity mode:
they press the keys the game already reads. In the world, Start presses Escape (the book) and Back the
installed inventory key. Outside the world, and over the logbook or the inventory, the D-pad or left stick
holds the arrow keys (pressing past half its tilt and releasing below three eighths), South holds Enter, and
East and Start hold Escape. The pad is read only while the window has focus.

## Optional anisotropic filtering

Backtick's **Texture filtering** row enables the Modern `anisotropic_filtering` fidelity
area independently of lighting. Like every area it defaults to Faithful; `--mode modern` and **All
modern** turn it on, and **All Faithful** restores the original sampling. World terrain, water,
models, flats, clouds and sun then average up to sixteen bilinear samples along the longer texture
footprint, which also replaces the Modern `textures` point sampling. Whether a chroma-keyed texel
is cut out is still decided by the original sample at the pixel centre, and keyed samples are left
out of the average, so palm fronds and other keyed models keep their holes. Other colour images use linear
samplers with a hardware anisotropy limit of sixteen. The indexed world paths integrate their
existing source textures without mipmaps, which costs GPU time. Turning it off restores the image
samplers.

## Optional blood particles

The native `blood` mod adds a brief spray of red particles when health services reduce an actor's
health. It defaults to inactive. Spray size, color, gravity and lifetime are mod preferences,
with amount and lifetime exposed in the Mods window. Particles fade without collision or surface
marks. See the [mod authoring example](../mods/README.md#damage-driven-particles).

## Optional periodic autosave

The native `autosave` mod requests a reserved-slot save after a configurable amount of
eligible play, default 300 seconds. It defaults to inactive. This is a mod preference.
See [the mod service contract](../mods/README.md#save-service).

## Optional swaying plants mod

The bundled `mods/sway` mod, disabled by default, tilts placed static models whose installed
name contains `PALM`, `TREE` or `PLANT` about their base in a wind, with Faithful or Modern
lighting. The original draws them still; collision keeps the installed placement. This is a
mod preference, formerly a Modern lighting effect.

## Optional terrain grass mod

**Verified:** PR #255's grass policy is ported to the bundled native Wasm guest
`mods/grass`, disabled by default. It reads terrain UVs and installed texture
pixels through bounded host hooks. Seeded blades, distance bands, density and wind
are guest policy. This optional appearance is not original engine behavior.
The host supplies scene-lit owned meshes and cleanup. Under Modern lighting they take
the scene's lights; under Faithful lighting they take the world's ambient and
directional model light, as original models do. Ray-traced grass stand-ins are
unavailable. The shared geometry shader
library follows the existing first-active-mod shader replacement rule.

## Audio resampling

**Unknown:** the original game's reconstruction filter. Faithful keeps the
existing linear backend conversion without claiming waveform parity. Modern
`audio_resampling` uses Rubato's 24-tap Hann-windowed sinc when increasing a source
rate to the output rate. It filters the reconstructed signal while retaining its
source duration and channel ordering. It cannot recover detail absent from the
recording. Filter length, window and phase resolution are Modern design choices.

Select Audio resampling independently of Music in the backtick debug menu, or set
`audio_resampling` through the fidelity control port. New source spans use the
selection; already playing spans retain their filter. The option covers native
backend audio and browser streaming audio. Direct WebAudio CD music retains the
browser's conversion. Equal-rate playback and downsampling keep their existing
backend paths.

## Default startup opens the book

**Verified:** cyrus opens the book after the splash and title on native and browser launches.
Installed debug flags and the absence of an autosave do not automatically start a
new game. This is an intentional launcher choice. Explicit New Game, movie, save
and scene shortcuts retain their requested destinations.

## Retained original saves and moving-actor acceptance

**Verified:** cyrus loads its own session documents, not original SAV/TSG saves.
This accepted compatibility limit prevents loading a retained original manual
save directly for a death/restart comparison. The retained-save investigation
verified one 800,468-byte original slot, but accepted zero paired death/restart
sequences. Its final recorder attempt failed the contiguous-update check after
one baseline at update 13, health 100. No death/restart visual residual was
measured. See [the retained-save decision](https://github.com/michidk/cyrus/issues/320).

**Unknown:** moving-actor contact parity. The accepted validation residual is
zero matched contact pairs, not a measured collision offset. The earlier
GUARD01 encounter matched 36 of 36 selected-player positions over 6,225 consumed
clocks, but the native guard baseline differed by 147.1992 map units after
warmup, so that run does not establish contact parity. The final attempt stopped
at update 36, health 100, then rejected a missing other-actor observation before
recording contact. Both traversal orders remain
unverified. See [the contact decision](https://github.com/michidk/cyrus/issues/306).
Neither residual changes Faithful defaults or authorizes a guessed response.

## Accepted visual residuals

**Verified:** the loading-fade raster reproduces five sampled original
intermediate frames exactly. The remaining opaque loading-artwork comparison
has RGB MAE 1.312669891 and mean-channel bias 1.104227431 at 640×420.
The incoming 172/256-opacity pair has MAE 0.474791667. These are accepted
artwork residuals; no unsupported color correction is applied.
See [the fade decision](https://github.com/michidk/cyrus/issues/312).

**Verified:** held-torch emission fixes the missing wall brightening. In the
22,500-pixel TAVERN wall crop, the native lit/unlit delta is 18.118370,
compared with the original's 19.178044. The lit pair has RGB MAE 3.110489;
the unlit pair has MAE 0.001481. **Unknown:** how much of the remaining
lit difference comes from unmatched hand pose and random-color phase.
This is an accepted visual residual, not a reason to tune the recovered
emission constants. See [the torch decision](https://github.com/michidk/cyrus/issues/314).

**Unknown:** browser transition visual parity and actor prelighting parity.
The bounded browser fixture admitted zero ISLAND→TAVERN→ISLAND pairs.
The CATACOMB/PALACE prelighting fixture admitted zero of two pixel pairs;
two native displayed-frame matches did not establish visible matching actors.
No RGB error is available for these accepted validation limits. Earlier
unmatched-pose actor errors of 4.282308355 and 37.819504902 are rejected as
lighting measurements. See [browser transitions](https://github.com/michidk/cyrus/issues/313)
and [prelighting](https://github.com/michidk/cyrus/issues/316).

**Unknown:** matched-state ISLAND water cadence and wave-phase parity.
The final direct-entry recording admitted 43 original presentations over
6,218 game-clock units, but zero matched native water-state pairs.
The earlier coast recording measured three vertex ranges of 42.2119,
43.5659 and 42.0251 engine units. Those ranges do not measure cross-host
wave error. No matched water-region RGB MAE is available, and no timing
formula is changed on this evidence. This accepted validation limit is
recorded in [the water decision](https://github.com/michidk/cyrus/issues/315).

### Logbook scrolls and viewport edges

**Verified:** the settled ISLAND style-7 logbook pair at offset zero has
whole-frame RGB MAE 1.340038, with 28,607 of 307,200 pixels differing after
the paper fog correction. The paper crop matches exactly, with zero of
162,400 pixels differing. The top scroll has MAE 0.652786 and 9,516 of
37,701 pixels differing; the bottom has MAE 0.741643 and 12,821 of 37,701.
Both scroll crops have maximum channel error 9. The text crop has MAE
0.060099, with 166 of 6,750 pixels differing and maximum channel error 9.
The union of viewport rows 0–5 and 479 and column 639 has MAE 70.815129,
with 3,108 of 4,953 pixels differing and maximum channel error 255.
**Unknown:** the remaining interpolation, glyph arithmetic and edge coverage.
These measured differences are accepted; the encoded-blend substitution was
rejected because it increased differing foreground pixels from 166 to 189.
See [the logbook decision](https://github.com/michidk/cyrus/issues/310).

### Sparse lower-sky sampling

**Verified:** the synchronized ISLAND pair has upper-crop RGB MAE zero,
with zero of 6,000 pixels differing. The lower crop has MAE 0.101944,
with 113 of 2,400 pixels differing and maximum channel error 9. All 113
errors lie in its 1,938 sky pixels; its 462 occluded pixels agree.
The horizon crop has MAE 0.026667, with six of 400 pixels differing and
maximum channel error 8; those six pixels show geometry.
**Inferred:** texture-plane setup explains much of the sparse sky error.
A diagnostic reduced lower-sky differing pixels from 113 to 26, but its
triangle anchors remain **Unknown**, so it is not shipped. These measured
residuals are accepted. See [the sky decision](https://github.com/michidk/cyrus/issues/309).

### Sabre examination

**Verified:** the settled ISLAND/ISWORD pair after integer filtering and
RGB565 storage has model-mask RGB MAE 19.640764, with 3,217 of 4,208
pixels differing and maximum channel error 247. Before that correction,
MAE was 20.234474 and 4,003 pixels differed. The 2,953-pixel common interior
has MAE 16.241224; coverage includes 86 original-only and 49 native-only
pixels. Description foreground has MAE 1.418287 over 1,954 pixels, with
maximum channel error 8. **Unknown:** the remaining model and description
error causes. These measured visual residuals are accepted without tuning
unsupported constants. See [the examine decision](https://github.com/michidk/cyrus/issues/319).

### World-entry static surfaces

**Verified:** the marker-zero CATACOMB wall crop has RGB MAE 6.498651,
with 20,986 of 25,200 pixels differing. Independent original and native
repeats reproduce that wall exactly within each implementation. A later
original camera pose, reproduced in native, reduces the same screen rectangle
to MAE 0.006944, with 74 pixels differing. The installed wall material is
not animated. **Unknown:** the cause of this view-dependent static raster
residual. It is not attributed to moving-content timing.

**Verified:** the CAVERNS fixed-wall crop has RGB MAE 5.244014, with
34,296 of 41,600 pixels differing. Its mean native-minus-original RGB bias
is (+0.013317, +0.075264, +0.019231) channel levels. **Unknown:** the
remaining static raster cause. These two measured residuals are accepted
without an unsupported lighting or texture-coordinate correction.

**Verified:** the START deck crop has MAE 0.005213 over 14,450 pixels;
the HIDEINT cave-wall crop has MAE 0.001436 over 6,500 pixels. Independently
advancing actors, ship poses and clouds prevent whole-frame errors from
measuring static rendering alone. Other sampled fixed regions in the
29-entry sweep have MAE at most 0.134238. NECRISLE's first marker shows
only sea and sky, so no fixed-region measurement is available. World 99
admits no original pair: direct entry remains at zero actors and zero updates
for 120 seconds; the final in-game path fails interactive-console admission
before a load command is sent. World 29 is a separate entry.
See the [world sweep and ranked measurements](https://github.com/michidk/cyrus/issues/323).

**Verified:** a per-face installed-texture diagnostic isolates 1,274 CATACOMB
wall pixels and 915 CAVERNS wall pixels. Both selected textures are 256×256
and static, with unit texture reduction. Fitting only channel gains to the
installed texture gives original errors of 6.610158 and 1.430399 MAE.
Allowing a texture-coordinate translation reduces them to 2.321618 and
0.611683; the fitted offsets are (+0.125, +2.000) and (-2.500, -2.875)
source texels. Native fits select zero offset, with errors 2.291650 and
0.561001. **Inferred:** texture-coordinate registration contributes to these
entry-view differences. **Unknown:** which projection, clipping or texture-plane
stage causes it. The fit approximates lighting with channel gains and is a
diagnostic, not a parity correction. No fitted offset is applied to rendering.

**Verified:** a second CAVERNS sample isolates 1,158 pixels on the brown wall,
using its installed 128×256 texture. The same diagnostic reduces the original
fit error from 6.914668 to 2.465891 MAE at (-2.125, -2.125) source texels;
the native fit selects zero offset at 2.265434 MAE. Its fitted channel gains
are close to the original's translated fit. **Inferred:** texture registration
also contributes on this higher-error face. The exact rendering stage remains
unknown.

**Verified:** the refined VILE trunk sample has original/native MAE 0.002222
over 1,200 pixels. The earlier rectangle labeled as a trunk sampled water;
the earlier HIDEINT rectangle also included water along the cave boundary.
Those rectangles remain scene measurements and do not establish static-surface
parity. The refined trunk and cave-wall samples both match the browser exactly.

**Verified:** a WebGL sky direction regression mapped the positive forward axis
to the opposite direction through the `atan2`/`sin`/`cos` round trip. The shared
shader now normalizes the horizontal forward vector directly. In the controlled
headless-browser shader comparison, START sky MAE falls from 11.151208 to
0.013767 over 79,200 pixels, with differing pixels falling from 77,092 to 39.
The uploaded sky texture matches the prepared image byte for byte. The browser
regression exercises signed-zero, axis-aligned and oblique directions against
the shared shader calculation, and fails with the previous calculation. See the
[browser sweep](https://github.com/michidk/cyrus/issues/326).

# Accepted differences

Deliberate, user-visible differences between cyrus's defaults or options and a
particular way of running the original. Each entry names the original setup it
differs from, the reason it is accepted, and how to select the alternative.
Engine facts behind each entry live in
[redguard-preservation](https://github.com/michidk/redguard-preservation).
Unintended differences are bugs; report them in [GitHub issues](https://github.com/michidk/cyrus/issues), not here.

## 3dfx splash

**Verified:** the GOG Windows launch plays the metallic `3DfxSpl2.dll` splash. Emulated Voodoo hardware,
such as DOSBox-X with `glide=emu`, runs the DOS Glide overlay and shows an older shield animation instead
([startup facts](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/startup.md#splash-variants)).

cyrus plays only the Windows splash, because it is what GOG players see, and has no option for the shield.
Comparisons against DOSBox-X therefore show a different splash. Glide edge antialiasing is approximated by
supersampling.

## Skipped introduction movie

**Verified:** New Game plays the introduction movie before entering the world. `--skip-movies`
(browser `?skip-movies=true`) enters the world without it, from the menu entry as well as from
`--new-game`. The default plays it. The ending movie is not affected.

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
bloom. Brightness, shadow distances, the point lights' power and the effects are tuning choices, not
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
- glossy water with screen-space reflections, which reflect only what is on screen and the
  horizon colour elsewhere;
- swaying plants: placed models whose installed name contains `PALM`, `TREE` or `PLANT` tilt
  about their base, while collision keeps the installed placement;
- depth of field focused on the player, and motion blur.
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
projection, placements and camera paths, into the original's 640 by 480 frame, which is then scaled to
the window like the other startup pictures. Light levels follow the original's per-channel combine
(`floor(T * (L + 1) / 256)`, interpolated in screen space), applied in a shader. Page text, widgets and
pictures are painted into the page textures on the CPU, as the original does.

**Verified:** against the previous CPU renderer the settled book pages differ by a mean of about 1.5 out
of 765 per pixel (roughly 0.5 % of pixels visibly) and the logbook by about 0.3. The remainder is the
GPU's texture sampling and rounding, not different geometry. **Unknown:** whether the original's own
sampling matches either to the last bit; it was not compared pixel by pixel.

Dialogs over the book (delete confirmation, game description) are drawn on a transparent layer above the
scene, where the original composited them into the same frame. **Unknown:** whether the result is pixel-identical;
these dialogs were not compared against the previous renderer.
The F11 flycam also flies the book's camera in the menu, which the original has no equivalent of; it is
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
zone of about 10 %. The Controls page capture accepts a gamepad face button as joystick button 1 to 4, as the original's capture accepts joystick buttons; menus and the book do not otherwise read the gamepad. The seven other
rebindable actions (next and previous item, inventory, log, quick sword, quick health, map) read their code the
same way, so a mouse or joystick code bound on the Controls page works for them. **Verified:** the original
reads every action through one shared input table (keyboard scancodes, mouse releases 128 and 129, joystick
codes 132 to 139). **Inferred:** cyrus treats a mouse release as a one-update press and a joystick code as
held, with its press edge taken from the previous poll. **Verified:** the inventory screen polls input once
per update before waiting for the next BIOS tick, so a mouse binding there reads the release between two
successive updates; cyrus does the same. **Unknown:** the dead zone a particular original
joystick showed, and whether the original's menus read the joystick; no joystick read was found in the menu
code, so menu navigation by gamepad is not provided in Faithful mode.

## Optional anisotropic filtering

F11's **Anisotropic filtering (up to 16×)** enables the Modern `anisotropic_filtering` fidelity
area independently of lighting. Like every area it defaults to Faithful; `--mode modern` and **All
modern** turn it on, and **All Faithful** restores the original sampling. World terrain, water,
models, flats, clouds and sun then average up to sixteen bilinear samples along the longer texture
footprint, which also replaces the Modern `textures` point sampling. Other colour images use linear
samplers with a hardware anisotropy limit of sixteen. The indexed world paths integrate their
existing source textures without mipmaps, which costs GPU time. Turning it off restores the image
samplers.

## Optional blood particles

The native `blood` mod adds a brief spray of red particles when health services reduce an actor's
health. It defaults to inactive. Spray size, color, gravity and lifetime are mod preferences,
with amount and lifetime exposed in the Mods window. Particles fade without collision or surface
marks. See the [mod authoring example](../mods/README.md#damage-driven-particles).

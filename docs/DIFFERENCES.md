# Accepted differences

Deliberate, user-visible differences between cyrus's defaults or options and a
particular way of running the original. Each entry names the original setup it
differs from, the reason it is accepted, and how to select the alternative.
Engine facts behind each entry live in
[redguard-preservation](https://github.com/michidk/redguard-preservation).
Unintended differences are bugs; report them in [GitHub issues](https://github.com/michidk/cyrus/issues), not here.

## 3dfx splash variant

**Verified:** the GOG Windows launch plays the metallic `3DfxSpl2.dll` splash.
Emulated Voodoo hardware, such as DOSBox-X with `glide=emu`, runs the DOS Glide
overlay and shows the older shield animation
([startup facts](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/startup.md#splash-variants)).

cyrus plays the Windows variant by default, because it is what GOG players see.
`--splash dos` (browser `?splash=dos`) plays the overlay's shield animation from
the installed `glide2x_emu.ovl` instead. Comparisons against DOSBox-X must
select the DOS variant. **Verified:** a side-by-side capture against DOSBox-X
running the GOG overlay matches the shield, logo, tints and projected shadow.
Glide edge antialiasing is approximated by supersampling.

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

## Ray-traced lighting

The original has no ray tracing. The debug menu's **Ray tracing** switch (the `ray_tracing` fidelity
area, Modern only, native builds only) lights opaque world meshes with Bevy Solari instead of the original's
vertex-driven shading. It needs a GPU with ray-query support and is greyed out without one. The sun,
the ambient colour and the object lights come from the world's installed lighting data, but their
brightness is a tuning choice, not a measured match. Flats, water, sky, alpha-masked or chroma-keyed
meshes and the HUD keep their original shaders and draw over the lit scene; Glide fog and clipping do not
apply to traced meshes. **Unknown:** the result on ray-query hardware, since it was written without a
GPU to run it.

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

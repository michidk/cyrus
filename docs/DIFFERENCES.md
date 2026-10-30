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

## Credits scroll pace

**Verified:** the original scrolls the credits one pixel per buffer swap with swap interval one, and opens
its Glide window with the 60 Hz refresh setting
([credits facts](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/menu.md#credits-scroll)).
cyrus scrolls one pixel per 1/60 s on any display, which is the pace of a display that honours that
setting. **Inferred:** other displays and emulated Glide swaps pace differently (a DOSBox-X run scrolled
faster); the original's pace there is not a target.

## Rope attachment shortcuts

**Verified:** the attach test, the controls, the swing and the release follow preservation's rope sections.
**Inferred or simplified:** the climb helpers do not read the animation record's progress bytes. The
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

## Natives no installed script calls

**Verified:** the native catalogue has 367 entries; the installed scripts of all 27 maps call 151 of
them, and `dev/scripts/native-audit.py` shows that every one of those is handled by a source module. The
other entries (script messaging and broadcasts, the `pipe*` family, the `spare*` parameters, slave and
hook management, and debug helpers, among others) are called by no installed script and have no recovered
contract, so they are outside the SOUP completion scope, which asks for what an original script contract
requires. cyrus does not implement them: a script that reaches one stops with a contextual native error
naming the function, never a panic or a silent default. A modified or added script that needs one first
needs its behaviour recovered in redguard-preservation.

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
faithful mode does not reproduce. A script holding a reference to a killed created object reaches a dead
record, where the original reaches freed memory, which has no defined behaviour to match.

## Book picture after the ending

**Verified:** after the credits the original's book shows its left-page picture frame filled with a flat
pink-red rectangle, with no gameplay snapshot. cyrus shows the book's empty parchment frame in the same
state (it takes no snapshot when the ending starts). **Inferred:** the flat fill is the original's
unpopulated picture buffer drawn through the book palette, not designed artwork, so cyrus does not
reproduce the colour.

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

## CD data noise at the start of track 2

**Verified:** the GOG image places CD-ROM data-sector bytes at the start of
audio track 2. Played as audio, they make a short loud noise when the book menu
opens ([music facts](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/music.md#the-cd)).

Faithful music plays every installed byte. `--music modern` (browser
`?music=modern`, or the debug menu's Music fidelity) mutes the leading
sync-marked data sectors and keeps the track's length and timing.
**Unknown:** whether the original's playback path makes the noise audible.

## Credits scroll pace

**Inferred:** the original scrolls the credits one pixel per buffer swap with swap
interval one, which waits for one display refresh. cyrus scrolls one pixel per 1/60 s
on any display. The original's refresh rate setting was not recovered, so its visible
pace is not verified.

## Rope attachment shortcuts

**Verified:** the attach test, the controls, the swing and the release follow preservation's rope sections.
**Inferred or simplified:** the hand-height link choice (grab and downward climb) skips its clamp against
the floor probe near the ground, the climb helpers do not read the
animation record's progress bytes, and the swing sound plays without its channel start index (owned by the
first attached actor; not heard in a running world).

## Ground prevalidation with one missing surface

**Verified:** when an actor's current and previous positions find support, or both miss, cyrus follows
preservation's ground prevalidation. **Unknown:** when exactly one of them misses, the original reads the
missing side's surface class from uninitialised stack memory, so its result is not defined. cyrus treats the
missing side as neither class 2 nor class 4: a missing current surface clears the results, and a class 2
current surface after a missing previous one runs the heading comparison.

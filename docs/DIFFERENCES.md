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

**Verified:** the original scrolls the credits one pixel per buffer swap with swap interval one, and opens
its Glide window with the 60 Hz refresh setting
([credits facts](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/menu.md#credits-scroll)).
cyrus scrolls one pixel per 1/60 s on any display, which is the pace of a display that honours that
setting. **Inferred:** other displays and emulated Glide swaps pace differently (a DOSBox-X run scrolled
faster); the original's pace there is not a target.

## Rope attachment shortcuts

**Verified:** the attach test, the controls, the swing and the release follow preservation's rope sections.
**Inferred or simplified:** the climb helpers do not read the animation record's progress bytes, and the
swing sound (owned by the first attached actor; not heard in a running world) starts at channel index 6 as
preservation documents. The hand-height link choice clamps against the floor probe as decoded; **Inferred:** the terrain sample
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

## Natives no installed script calls

**Verified:** the native catalogue has 367 entries; the installed scripts of all 27 maps call 151 of
them, and `dev/scripts/native-audit.py` shows that every one of those is handled by a source module. The
other entries (script messaging and broadcasts, the `pipe*` family, the `spare*` parameters, slave and
hook management, and debug helpers, among others) are called by no installed script and have no recovered
contract, so they are outside the SOUP completion scope, which asks for what an original script contract
requires. cyrus does not implement them: a script that reaches one stops with a contextual native error
naming the function, never a panic or a silent default. A modified or added script that needs one first
needs its behaviour recovered in redguard-preservation.

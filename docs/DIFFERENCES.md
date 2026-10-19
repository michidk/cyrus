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

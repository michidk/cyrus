# cyrus-audio

Native/browser effects, speech and CD music playback. `SoundPlugin` and
`MusicPlugin` retain the shared request, cue and playback-state interfaces.
Logical cues stay in `cyrus-world`; subtitles and cinematic presentation stay
in presentation. This crate has no rendering or startup dependency.

Hosts supply an optional `AudioListener` in Bevy world space. Positional starts
require that listener. `AudioReadiness { prepare }` defaults to true; startup
holds it false until the title appears. Startup copies committed menu preferences
to `SoundSettings` before `ApplyPreferences`. The Update order is
`ApplyPreferences` → world `AnimationSystems` → `PlayCues`.
`CinematicAudio` marks cinematic playback and pauses CD music. Browser startup
can inspect `SoundBankHandle` and its asset dependencies for loading failures.

Animation cues use installed SURFACE.INI remaps on native and browser. The actor's
`SoundSurface` selects the row; known zero bypasses it, and missing ground state
remains unsupported. Playback validates the remapped effect, while cleanup retains
the authored source ID. Script, combat and speech starts bypass these tables.
Actor surface updates belong to the ground/SOUP host, not the sound plugin.

Headless hosts omit Bevy's audio backend and retain requests/control state.
Channel ownership, actor cleanup, world expiry and browser queued starts use the
same rules as windowed playback. Installed audio facts and units are documented
in [preservation](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/sound.md).

Run `cargo test -p cyrus-audio`; installed decoder tests report skips when the
install is absent. Check the browser target and native headless assembly for
platform boundaries. Audible parity requires a separate original-game comparison.

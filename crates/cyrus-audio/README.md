# cyrus-audio

Sound effects, RTX speech and CD music for the native and browser hosts.
`SoundPlugin` plays the logical cues that `cyrus-world` and the script runtime
emit; `MusicPlugin` plays the CD tracks. The crate depends on `cyrus-assets`,
`cyrus-game`, `cyrus-script` and `cyrus-world`, and has no rendering or startup
dependency: subtitles and cinematic presentation live in the presentation
crates. Installed audio facts and units are documented in preservation's
[sound](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/sound.md)
and [music](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/music.md)
pages.

## Modules

| Module | Responsibility |
| --- | --- |
| [lib](src/lib.rs) | Plugins, `AudioListener`, `follow_listener`, `AudioReadiness`, `CinematicAudio` |
| [sound](src/sound.rs) | `SoundPlugin`: MAIN.SFX effects, RTX voices, cue playback, preferences |
| [sound/samples](src/sound/samples.rs) | The shared sample-slot pool and its allocation order |
| [sound/script](src/sound/script.rs) | `ScriptSounds`: synchronous SOUP sound services, dialogue and taunts |
| [sound/dialogue](src/sound/dialogue.rs) | Per-label dialogue assets (`cache://dialogue/<key>.dialogue`) |
| [sound/transport](src/sound/transport.rs) | `Panned`: the gain/pan source decoder and its checkpoint |
| [sound/persistence](src/sound/persistence.rs) | Sample-pool snapshots for session restoration |
| [music](src/music.rs) | `MusicPlugin`: CD track selection, volume, pause, browser output |
| [music/checkpoint](src/music/checkpoint.rs) | `MusicTransport`, `MusicSnapshot` and off-live replacement |
| [sound_bank](src/sound_bank.rs) | The browser's effect bank (`SoundBank`) |
| [resampling](src/resampling.rs) | Backend upsampling filter for the `audio_resampling` fidelity area |
| [persistence](src/persistence.rs) | `AudioRestoreBindings`: saved-to-replacement owner mappings |

## Usage

Hosts add `SoundPlugin` and `MusicPlugin`, supply an `AudioListener` (game hosts
schedule `follow_listener::<ViewCamera>` before `ApplyPreferences`) and order
Update as `ApplyPreferences` → world `AnimationSystems` → `PlayCues`. Startup
holds `AudioReadiness { prepare }` false until the title appears. Headless hosts
omit Bevy's audio backend and keep request and ownership state.

## Notes

- Script commands, ambient polls, animation cues and speech share one sixteen-slot
  sample pool. Browser starts that wait for the effect bank belong to that pool,
  so stops, world teardown and restoration cancel them like live samples.
  SOUP `StopAllSounds` stops the pool but not CD music.
- Original PCM width is kept through preparation; gain follows preservation's
  8-bit table / 16-bit scalar rule. The vendored fixes are explained in
  [`dev/vendor/rodio`](../../dev/vendor/rodio/README.md) and
  [`dev/vendor/cpal`](../../dev/vendor/cpal/README.md).
- `audio_resampling` is its own fidelity area: Faithful keeps Rodio's linear
  conversion, Modern uses a Rubato windowed sinc. The original reconstruction
  filter is **Unknown**; browser CD playback uses WebAudio's own conversion.
- Animation cues capture their surface and position when emitted; SURFACE.INI
  remaps apply to them, not to script, combat or speech starts.
- `Panned` and `Music` checkpoints restore exact decoder progress but exclude
  resampler and device buffers, so restoration is not a gapless splice. Capture
  rejects in-flight preparation, including starts still waiting for the browser bank.
- World teardown retires the whole sample pool, including unowned control voices.

## Testing

```sh
cargo test -p cyrus-audio
bun test ./crates/cyrus-audio/tests/music_browser.test.js
```

Installed tests (for example `tests/installed_audio_decode.rs`) report a skip
without an install. Unit tests do not show audible parity; that needs a paced
audio run (`dev/docker/dev/Dockerfile.sound-acceptance`) or a browser graph
capture, plus an original-game comparison.

The [examples](examples) are isolated acceptance scenes. Each pairs with a driver
in `dev/scripts/` that checks actual mixer or WebAudio PCM; the browser variants
use the pages in `dev/fixtures/`. Fixtures exported from the install contain
game data and stay under ignored local state.

| Example | Checks | Drivers |
| --- | --- | --- |
| `surface_audio` | Ground surface and animation cue selection against a JSON fixture | `compare-surface-audio.py`, `test-surface-audio.py` |
| `sound_callers` | SOUP stop functions and owner isolation | `test-sound-callers-native.py`, `test-sound-callers-browser.mjs` |
| `positional_emitters` | Attached and positional emitter gain/pan; `--export` writes the fixture | `test-positional-native.mjs`, `test-positional-browser.mjs`, `run-positional-browser.py` |
| `speech_teardown` | Voice ownership across world replacement and subtitle skip | `native-speech-teardown.py` |
| `music_settings` | Restored CD ownership through preference changes | `native-music-settings.py` |
| `deferred_sample_capture` | Browser bank deferral during capture (browser only) | `test-deferred-sample-capture.mjs` |
| `animation_binding` | Script sound routing before scene binding | `test-animation-binding-native.py`, `test-animation-binding-browser.mjs` |
| `natural_animation` | Installed draw/sheath command sounds | `prepare-natural-animation.py`, `test-natural-animation.py` |
| `replacement_lifecycle` | Animation replacement through the deferred binding queue | `run-animation-replacement-browser.py` |

Install-free controls run directly, for example
`node dev/scripts/test-positional-controls.mjs`,
`python3 -B dev/scripts/test-surface-audio.py` and
`node dev/scripts/test-browser-music-output.mjs CDP_URL` against a Chromium
DevTools endpoint.

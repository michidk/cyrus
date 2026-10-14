# cyrus-audio

Native/browser effects, speech and CD music playback. `SoundPlugin` and
`MusicPlugin` retain the shared request, cue and playback-state interfaces.
Logical cues stay in `cyrus-world`; subtitles and cinematic presentation stay
in presentation. This crate has no rendering or startup dependency.

Loaded sample playback entities carry `PannedGains`, shared with their active
decoder. A volume/pan update affects subsequent stereo frames without restarting
the sample. One-shots retain their start gains unless explicitly updated.

Hosts supply an optional `AudioListener` in Bevy world space. Positional starts
require that listener. `AudioReadiness { prepare }` defaults to true; startup
holds it false until the title appears. Startup copies committed menu preferences
to `SoundSettings` before `ApplyPreferences`. The Update order is
`ApplyPreferences` → world `AnimationSystems` → `PlayCues`. Ordinary updates
also drain earlier cues before lending `ScriptSounds`; both drains retain the
same message cursor, so commands are consumed once. Immediate ordinary script
commands and ambient polling use one live sixteen-slot channel pool. Backend
completion identifies an individual start; animation and ambient handles identify
a reusable slot and control its current occupant.
`CinematicAudio` marks cinematic playback and pauses CD music. Browser startup
can inspect `SoundBankHandle` and its asset dependencies for loading failures.

Animation cues use installed SURFACE.INI remaps on native and browser. The cue's
captured surface selects the row; known zero bypasses it, and missing state
remains unsupported. Logical SOUP cues also capture their own local signed Q8 position, without the
linked or attached substitutions used by script Sound. Later actor changes
cannot alter either snapshot. Presentation-only cues may omit the position and
use their scene transform. Stop and
Forget do not require surface state. Playback validates the remapped effect, while cleanup retains
the authored source ID. Script, combat and speech starts bypass these tables.
Actor surface updates belong to the ground/SOUP host, not the sound plugin.

`ScriptSounds::with_driver` lends synchronous `AmbientSoundDriver` feedback and
immediate cue dispatch for one logical-world update. The caller owns timing and
reset inputs. Missing automatic reset/menu/dialogue/navigation ownership remains
unsupported; the adapter does not assume a clear periodic-reset flag.

Headless hosts omit Bevy's audio backend and retain requests/control state.
Channel ownership, actor cleanup, world expiry and browser queued starts use the
same rules as windowed playback. Installed audio facts and units are documented
in [preservation](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/sound.md).

Run `cargo test -p cyrus-audio`; installed decoder tests report skips when the
install is absent. Check the browser target and native headless assembly for
platform boundaries. Audible parity requires a separate original-game comparison.

## Surface-audio comparison

`cargo run -p cyrus-audio --example surface_audio -- INSTALL PLAYER_CACHE RGM_OR_ANIMATION_CACHE FIXTURE_JSON`
runs the production static ground query, CYRUS animation interpreter and mixer
without a window or audio device. It reads the install and cache without writing
them. CYRUS uses the player channel range. Supply a `.player` cache exported
from the same installed RGM and world.
The report goes to stdout; keep installed fixtures and reports under `magic/`.

The JSON fixture has `world` and a nonempty `cases` array. Each case has a unique
`name`, `initial_surface`, installed `collision_attribute`, animation `group`
and nonempty `steps`. Each step supplies `position_q8`, `view`, `heading`, `ticks`
and optional `later_surface`. Positions retain original axes and eight fractional
bits; view coordinates are integer map X/Z units. Heading uses 1/2048 turns;
ticks are whole nominal animation ticks, capped at 1024 per step. A case retains
its surface, support history and animation state across steps. The first step
activates the requested group after querying support. Later steps advance it.
`later_surface` changes ECS state after each cue is queued, to check captured
surface playback. This is an isolated support/animation probe, not a full player
movement replay; it does not drive body clearance, dynamic support or keyboard input.

Each report step records the position, prior and assigned surface, ground veto,
support source and ordered animation events. Each event records its captured
surface, authored effect/rate offset for Play, newly started PCM target candidates
and playback speed, plus remaining channels after cleanup. Target identification
compares the actual mixer PCM with every installed bank entry. Identical samples
remain ambiguous rather than selecting an arbitrary effect ID.
For admitted grounded movement, pass a complete `.animations` cache from the same
world instead of the RGM, and set the case's `grounded` field to true and `group`
to zero. Optional `entry_ticks` supplies nominal ticks for player animation
initialization, defaulting to zero. Each step also requires
`movement: {"frame_scale": 256, "walk": true}`. Movement scale retains eight
fractional bits and is independent of nominal `ticks`; `walk` admits forward
movement. Supply measured inputs rather than deriving them from host durations.
The first step supplies the starting position and heading. Every subsequent step
must supply the exact position and heading left by the previous update; a mismatch
reports the case and step instead of teleporting the player.

Grounded cases initialize player animation, advance the ordinary animation stage,
select candidates with the current model's installed radius, then run the public
grounded movement and body/ground response. Reports include input and final pose,
timing inputs, frame/group state and ordered cues tagged by emitting stage. Each
cue retains its surface at that stage. This exposes the distinction between
ordinary animation before support assignment and a forward request after it.
Like the player runtime, these cues omit a position snapshot and use the final
scene transform; `captured_position_q8` is null. The listener stays at the case's
starting position. This tests PCM selection and requested rate, not original
camera/listener placement, audible output or automatic keyboard admission.
Dynamic support, airborne/weapon policy and remaining actor-update stages are
outside the admitted grounded path.

The omitted audio backend cannot finish samples naturally. Keep cases short enough
to avoid filling the channel pool; remaining-channel counts test explicit cleanup,
not sample-duration parity. Candidate selection uses the documented no-active-model
radius in the default static probe. Grounded cases require current-model radii.

`python3 dev/scripts/compare-surface-audio.py CYRUS_REPORT ORIGINAL_REPORT` compares
version-1 reports, including ordered events and cleanup. The original report must
use the same fields and add `evidence: {"kind":"original-runtime","record":"..."}`
pointing to an independent observation record. The comparator rejects ambiguous
PCM, empty Play coverage and reports with no actual playback starts. It does not
establish the provenance of a supplied record; retain original runtime inputs,
binary hashes and observations alongside it.
File-table predictions do not establish audible or gameplay parity. Synthetic
negative controls run with `python3 -B dev/scripts/test-surface-audio.py` and in the gates.

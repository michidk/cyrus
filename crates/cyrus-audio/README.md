# cyrus-audio

Native/browser effects, speech and CD music playback. `SoundPlugin` and
`MusicPlugin` retain the shared request, cue and playback-state interfaces.
Logical cues stay in `cyrus-world`; subtitles and cinematic presentation stay
in presentation. This crate has no rendering or startup dependency.

Loaded sample playback entities carry `PannedGains`, shared with their active
decoder. A volume/pan update affects subsequent stereo frames without restarting
the sample. One-shots retain their start gains unless explicitly updated.
Original PCM width is retained through native and browser preparation. The decoder
applies preservation's 8-bit lookup-table or 16-bit scalar gain rule, including
integer rounding before backend resampling. Rodio queue conversion starts after
the first source sample establishes its channels and effective rate. Exact decoder
size hints retain the complete resampling span, including a pending right channel.
The vendored dependency fix is documented in `dev/vendor/rodio/README.md`.
Volume and pan updates keep the sample's original width and apply together at stereo-frame boundaries.

Hosts supply an optional `AudioListener` in Bevy world space. Native and browser
game hosts schedule `follow_listener::<ViewCamera>` before `ApplyPreferences`;
the audio crate does not import the rendering camera marker. Positional starts
require that listener. `AudioReadiness { prepare }` defaults to true; startup
holds it false until the title appears. Startup copies committed menu preferences
to `SoundSettings` before `ApplyPreferences`. The Update order is
`ApplyPreferences` → world `AnimationSystems` → `PlayCues`. Ordinary updates
also drain earlier cues before lending `ScriptSounds`; both drains retain the
same message cursor, so commands are consumed once. Immediate ordinary script
commands and ambient polling use one live sixteen-slot channel pool. Backend
completion identifies an individual start; animation and ambient handles identify
a reusable slot and control its current occupant.
`PlayerAnimationSource` admits the player sample range for a separately initialized
player role without making that entity the controlled player. Scene entry uses it
for GREMLIN's temporary selection during setup; its remembered slot stays separate
from CYRUS.
SOUP `StopAllSounds` stops the shared pool in command order, across owners and
unowned effects. It retains remembered slot handles and does not stop CD music.
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
Logical `ScriptSound::AnimationState` Capture/Restore commands save the bound
actor's remembered slot in cue order, including an absent slot. Restore neither
stops newer samples nor restarts old ones. Subsequent animation cleanup uses the
restored reusable slot. Snapshots expire with the bound actor.

`ScriptSounds::maintain_sound` executes the explicit game-owned maintenance
counter against the live RTX cache, dialogue records, shared sample pool and
walk-node reservations in preservation order. Cleanup failure remains sticky in
the counter. The caller owns player-controller inhibition and world-entry reset;
this adapter does not enable automatic maintenance or controller admission.

RTX preparation returns an explicit pending result while an entry or its audio
dependency loads. RTXp retains its task without replacing dialogue records or
allocating a sample, then starts the prepared line once. A cache hit reuses the
retained entry without requesting another asset. Native per-label preparation
runs off the update thread; browser preparation uses the existing worker cache. Automatic sample reset, cache aging and dialogue
record updates still require integration.

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
and optional `later_surface`, `listener_position_q8` and `listener_yaw`. Listener
position uses signed original Q8 coordinates; yaw uses 1/2048 turns in the range
0..2048. Supply the render-camera state at cue emission for gain comparisons.
Positions retain original axes and eight fractional
bits; view coordinates are integer map X/Z units. Heading uses 1/2048 turns;
ticks are whole nominal animation ticks, capped at 1024 per step. A case retains
its surface, support history and animation state across steps. The first step
activates the requested group after querying support. Later steps advance it.
`later_surface` changes ECS state after each cue is queued, to check captured
surface playback. This is an isolated support/animation probe, not a full player
movement replay; it does not drive body clearance, dynamic support or keyboard input.

Each report step records the position, prior and assigned surface, ground veto,
support source and ordered animation events. Each event records its captured
surface, signed Q8 position for Play, authored effect/rate offset, newly started
PCM target candidates, playback speed and fitted decoded channel gain ratios, plus
remaining channels after cleanup. Gain ratios compare actual decoder output with
the source samples; a silent prefix reports null. Target identification
compares the actual mixer PCM with every installed bank entry. Identical samples
remain ambiguous rather than selecting an arbitrary effect ID.
For admitted grounded movement, pass a complete `.animations` cache from the same
world instead of the RGM, and set the case's `grounded` field to true and `group`
to zero. Optional `entry_ticks` supplies nominal ticks for player animation
initialization, defaulting to zero. Optional `idle_ticks` is an array of whole
nominal tick counts used to advance idle animation before the first compared
step, with position and surface fixed. It defaults to an empty array and requires
grounded mode. Each tick count is limited to 1024. Supply observed tick counts
to reconstruct the starting idle cursor. Each step also requires
`movement: {"frame_scale": 256, "walk": true}`. Movement scale retains eight
fractional bits and is independent of nominal `ticks`; `walk` admits forward
movement. Supply measured inputs rather than deriving them from host durations.
The first step supplies the starting position and heading. Every subsequent step
must supply the exact position and heading left by the previous update; a mismatch
reports the case and step instead of teleporting the player.
Gain ratios fit actual decoder output against up to 65,536 source frames, so
quiet-sample integer rounding does not stand in for the whole clip's scaling.
They are measurements of that PCM window, not exact waveform parity.

Grounded cases initialize player animation, advance the ordinary animation stage,
select candidates with the current model's installed radius, then run the public
grounded movement and body/ground response. Reports include input and final pose,
timing inputs, frame/group state entering and leaving each step, and ordered cues
tagged by emitting stage. Priming cues use the `idle_animation` stage and appear
before the first step's cues. Each
cue retains its surface at that stage. This exposes the distinction between
ordinary animation before support assignment and a forward request after it.
Each cue also retains the position of its emitting stage, before subsequent
movement. Coordinates must fit the transport's signed 32-bit Q8 range; an
out-of-range coordinate reports an error. Stop and Forget need no position and
report a null snapshot. Without listener snapshots, grounded cases keep the listener at the
case's starting position; other cases use the default origin. Supplied snapshots update it before that step's cues;
all cues in a step must share the supplied camera state. Coordinates use the
production camera origin conversion. This tests PCM selection, requested rate
and decoded gains. It does not establish native audible output, sample lifetime
or automatic keyboard admission.
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

`ScriptSounds::with_combat_taunt_playback` lends a prepared RTX clip and the
shared sample allocator to the game-owned taunt presenter. It validates requested
speech dependencies before releasing old ownership, restarts cached samples,
and formats text only when requested. Speech can start without a text record;
text-only presentation requires no audio dependency. This synchronous adapter
does not admit combat or preload candidates for script functions.
`with_combat_taunt_source` lends a prepared clip catalogue to synchronous SOUP
combat dispatch. The game selects the label once through shared RNG, then the
source checks its payload before releasing existing ownership. An absent label
fails without stopping the retained voice. Ordinary dispatch prepares nonzero
type-zero taunt labels before traversal and clears that catalogue when the world
generation changes. Pending dependencies defer traversal; failed preparation
reports a contextual error. Dialogue and taunts share the cache and sample pool,
borrowing them per operation so synchronous animation cues can use the same pool.
Combat admission belongs to the game host.

Per-label dialogue assets use `sound::dialogue::DialogueClip` and the shared
`cache://dialogue/<key>.dialogue` loader, with an optional generated WAV dependency.
The browser worker prepares a requested entry from the original byte source;
native hosts prepare the same entry into their local cache. Loading preserves
unformatted RTX text, original lifetime counters and header metadata.
`ScriptSounds` starts prepared voices through the shared sample channels at
header volume and centre pan from slot zero, then restores the general start
index to six. Volume refresh uses the logical speaker's local Q8 position and
retains centre pan. Disabled speech, text-only entries and channel exhaustion use
text lifetime. Missing voice dependencies and looping voices report unsupported
playback.

The ordinary host retains a twenty-entry cache and lends `DialogueSource` to
RTXp tasks. A cache miss returns pending until the entry and its audio dependency
are ready; pending preparation does not allocate a dialogue record or start a
voice. Ready text uses the installed HUD REDSEL font for wrapping. After object
updates, the host advances game-owned dialogue records through synchronous sample
feedback, then ages the RTX cache. The game selects the subtitle record and owns
speaker inhibition; rendering only reads that selection. The configured keyboard
key_a signal is sampled before Modern movement interpretation. Joystick input
remains unsupported.

The complete periodic reset and its selected-player inhibition stage remain
unconnected. Automatic RTXp and ambient tasks therefore still report missing
reset input; these adapters do not establish story or full dialogue acceptance.

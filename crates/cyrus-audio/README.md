# cyrus-audio

Native/browser effects, speech and CD music playback. `SoundPlugin` and
`MusicPlugin` retain the shared request, cue and playback-state interfaces.
Logical cues stay in `cyrus-world`; subtitles and cinematic presentation stay
in presentation. Deferred browser starts belong to the channel pool, so public
sample stops, periodic maintenance and saved-world restoration cancel them
through the same operation as live samples. Owner/effect filters apply to both
pending and live playback. This crate has no rendering or startup dependency.
Native credits music waits for `GameInstall` before its update runs, so the
install picker can stay open without initializing CD playback.

Loaded sample playback entities carry `PannedGains`, shared with their active
decoder. A volume/pan update affects subsequent stereo frames without restarting
the sample. One-shots retain their start gains unless explicitly updated.
The browser sound bank retains the install's speech and subtitle switches before
saved book preferences are applied. Speech mode controls dialogue record lifetime
maintenance; disabling it uses nominal text countdowns on both hosts.

Original PCM width is retained through native and browser preparation. The decoder
applies preservation's 8-bit lookup-table or 16-bit scalar gain rule, including
integer rounding before backend resampling. Rodio queue conversion starts after
the first source sample establishes its channels and effective rate. Exact decoder
size hints retain the complete resampling span, including a pending right channel.
The vendored dependency fix is documented in `dev/vendor/rodio/README.md`.
Volume and pan updates keep the sample's original width and apply together at stereo-frame boundaries.

`Panned::with_repetitions` validates the source and owns finite or unbounded
repetition before gain scaling. `with_playback` selects once or unbounded playback;
hosts use its `playback_settings()` so later loops observe live gain changes.
Preparation rejects invalid WAV rates, mismatched PCM widths and truncated data
before creating the backend decoder.
`Panned::snapshot` and `Panned::from_snapshot` retain exact interleaved source
progress, pending stereo output, gains, remaining repetitions and speed.
Transport checkpoint version 2 requires an explicit repetition count; older
boolean-loop checkpoints reject. Finite effects finish after the installed number
of passes on both native and browser hosts. Reconstruction
validates the supplied source identity and PCM width without starting playback.
Multiple decoder consumers reject capture. These checkpoints exclude downstream
resampler/device buffers; complete session audio restoration remains unavailable.

`Music::snapshot` retains the CD track, linear drive gain, actual pause state and
decoder progress. Capture rejects in-flight track preparation or an inconsistent
player census. `Music::from_snapshot` validates caller-prepared installed bytes
and returns an off-live `MusicCandidate` for coordinated session publication.
The published owner consumes the next scene-request and prepared-world
notifications without selecting world music again. This preserves a stopped CD
as well as a saved active track; a later ordinary world entry selects normally.
`MusicTransport` reuses the source cursor at unity PCM gain. CD volume stays on
the sink, and queued starts receive the current drive gain and cinematic pause.
Browser CD playback requests the browser's playback latency policy and uses one
retained WebAudio buffer per live player, so loading
and rendering on the main thread do not interrupt the track. Its checkpoint maps
the audio render clock to the original interleaved source cursor. Preparation
retains channel ordering even when a diagnostic checkpoint falls between stereo
samples. Browser effects continue through the shared streaming backend. After a main-thread
underrun, that backend rebases new buffer starts against the audio clock so
expired timestamps do not make successive buffers overlap. Audio production can
still pause while the main thread is busy. The vendored CPAL change and its
removal condition are documented in `dev/vendor/cpal/README.md`.

A successful diagnostic session replacement retires the old browser CD source
at the validated publication boundary. Candidate construction prepares the
rotated PCM, WebAudio buffer and silent source before publication. Publication
starts the prepared source at its saved cursor and schedules retirement of the
old source on the same audio clock, without waiting for another audio update.
Rejected candidates discard their silent output and leave the playing source
alone. Original-style saved-game loads keep live CD playback
until ordinary world selection changes the track.

The CD node stops and disconnects at retirement. Already rendered device frames
cannot be recalled. Effects and speech share the backend's mixed output queue:
submitted mixed frames can drain. Removing the old sinks requests a stop on
the mixer's normal source poll. Restore starts at each saved source cursor,
without saving or replaying mixed output buffers or clearing the shared device
stream. This does not promise
a gapless device-level splice.

`node dev/scripts/test-browser-music-output.mjs http://127.0.0.1:9227` uses an
existing Chromium DevTools endpoint to render synthetic stereo audio through
the production CD JavaScript. It compares output before retirement, during one
render quantum before the replacement starts, and through the restored buffer's
wrap. The delayed retirement control emits old audio during that quantum. The
check does not require the install or a physical output device.

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
Taunt services expose the shared cache's remembered sample before clip preparation;
this query does not depend on the channel's current playback status.
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
retains centre pan. Verified: ordinary RTX lookup still starts an available voice
when speech mode is disabled and retains its duration for the line's nominal
lifetime, following preservation's
[dialogue contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/sound.md#soup-functions).
Speech mode selects subsequent record maintenance. Text-only entries and channel
exhaustion use text lifetime. Missing voice dependencies and looping voices report
unsupported playback.

The ordinary host retains a twenty-entry cache and lends `DialogueSource` to
RTXp tasks. Before ordinary simulation, the host prepares literal RTX-family
labels from the world's scripts and their audio dependencies. Preparation does
not occupy the twenty runtime cache entries or start voices. Calls still allocate
cache entries and samples when executed, so file readiness adds no intervening
ordinary updates between prepared lines. Unused script labels can be absent;
their retained preparation error is reported only if the script calls them.
Dynamic label operands retain demand-driven preparation and can return pending.
Ready text uses the installed HUD REDSEL font for wrapping. After object
updates, the host advances game-owned dialogue records through synchronous sample
feedback, then ages the RTX cache. The game selects the subtitle record and owns
speaker inhibition; rendering only reads that selection. The configured keyboard
key_a signal is sampled before Modern movement interpretation. Joystick input
remains unsupported.

The ordinary host advances game-owned sound maintenance before model
presentation and passes its reset state to ambient and dialogue tasks. Missing
maintenance ownership remains an error. These services do not establish full
story, dialogue or gameplay acceptance.

`persistence::AudioRestoreBindings` validates the host's saved-to-replacement actor,
world-generation and presentation-entity mappings once for audio reconstruction.
Channel and dialogue candidates share those bindings. Missing owners and aliased
or inconsistent mappings fail before playback or live replacement. The host still
owns asset preparation, entity existence and the coordinated session commit.

`SamplePlaybackSnapshot` composes logical sample ownership with each `Panned`
source-decoder checkpoint. Capture requires a complete playback census after cue
dispatch and deferred commands. Reconstruction validates installed source bytes,
explicit owner/entity bindings and fresh transport tokens before returning a
candidate. The candidate queues its changes into the aggregate commit buffer;
it does not start playback during validation. Pause and mute state are retained.
Backend resampler and device queues remain outside this checkpoint.

`RuntimeDialogue::snapshot/from_snapshot` retains the prepared RTX cache, taunts,
request failures and ordinary-update state. `RuntimeDialogueSnapshot::required_labels`
provides installed asset preparation hints before validation. The host commits this
replacement together with logical dialogue records and the sample pool.

Sample checkpoints retain the control/examine voice stop slot as well as the
shared allocator and decoder owners. Active control voices contribute installed
RTX label hints independently of the script dialogue cache. Hosts prepare those
labels before validating the exact saved source identities. Old documents without
these optional hints remain readable; an unavailable source still rejects the
replacement. The existing mixed-buffer drain policy is unchanged.

Global control/book stops drain after gameplay cues already waiting at that audio
boundary. A later update may start new samples. `native-sound-reset.py` checks
public cancellation and continuation against a paced Pulse monitor;
`browser-sound-reset.js`, evaluated after `session-sample-graph.js`, checks real
WebAudio quanta around the book's production input. The installed
`test-browser-sound-reset-live.mjs` driver records cancellation, later starts
and executed artifact hashes through CDP. Already mixed output buffers
may drain before the silent browser observation window.

The isolated `sound_callers` example drives public `ScriptWorld::update_shells`
and `update_separate` into `SoundPlugin` and the real audio backend, without a
renderer or game assets. Its synthetic looping PCM distinguishes active output
from silence. Two independent callers exercise `EndMySound`, `EndMySounds`,
`KillMe`, statement `StopAllSounds`, later playback, numeric-result rejection,
and a second update that must not repeat the failed effect. Public integration
tests additionally assert exact channel counts and owner isolation.

Build with `cargo build -p cyrus-audio --example sound_callers` through the
repository's Cargo wrapper. Run in the paced-audio acceptance image with a
host-mounted `CYRUS_CONFIG_DIR`, then run
`python3 dev/scripts/test-sound-callers-native.py CONTAINER OUTPUT_DIR` to check
actual Pulse monitor PCM. Each caller kind admits nine nominal updates at
three-second observation intervals and terminates automatically.

For browser acceptance, build the same example for `wasm32-unknown-unknown`
with `--features bevy/web --profile wasm-dev`, package with the matching
`wasm-bindgen --target web`, and load it from a loopback HTTP page that calls
its generated `init`. Enable browser autoplay for this isolated test host.
Run `node dev/scripts/test-sound-callers-browser.mjs CDP_URL PAGE_URL WASM_FILE`.
The driver taps the actual CPAL WebAudio graph, waits for already mixed buffers
to drain, and asserts active/silent output for every phase. This is controlled
backend acceptance, not an installed-game or physical-device timing comparison.
Place `dev/fixtures/sound-callers.html` beside the generated bindings as
`index.html`. Serve only that test output directory on a separate non-preview
loopback test endpoint. The browser driver also checks the executed WASM bytes
against `WASM_FILE`; a stale artifact cannot satisfy the acceptance.
Use a secure browser origin: AudioWorklet is unavailable on an ordinary Docker
host URL. For container Chrome, a loopback-only `socat` bridge inside that test
container can forward its `127.0.0.1` test port to the host server. Load the
browser-loopback URL, not the Docker host URL. The fixture rejects an unavailable
AudioWorklet before starting the application, so the observer cannot panic the
production CPAL buffer-start callback.
`test-sound-callers-browser-controls.mjs CDP_URL INSECURE_PAGE LOOPBACK_PAGE`
checks early rejection of an insecure origin and mismatched executed WASM.
`node dev/scripts/test-browser-sound-reset.mjs` retains the output-oracle controls
for stale sound after stop, absent continuation and missing output quanta.

`positional_emitters` exports a local controlled fixture from installed ISLAND
DR_SPIN6, its DR_POL6 attachment master and its authored looping effect. Run
`cargo run -p cyrus-audio --example positional_emitters -- --export INSTALL FIXTURE_JSON`
through the repository wrapper, then run the example with `FIXTURE_JSON` as its
sole argument in the paced-audio runtime. The export retains installed family
placements, attributes, sound settings and PCM. It replaces the script bodies
with explicit diagnostic `Sound` and master `SetPosition` calls. Keep the export
under ignored local state; it contains game data.

The thirteen bounded nominal stages compare an attached start, movement without
restarting that snapshot, a fresh start after movement, a cleared slave attribute,
re-enabled attachment, listener yaw, pitch/roll invariance and local-position
playback. Silent stages separate starts. `test-positional-native.mjs CONTAINER OUTPUT_DIR`
checks actual paced Pulse stereo PCM. Build the same example for the browser,
package with the matching wasm-bindgen version, and place
`dev/fixtures/positional-emitters.html` beside the bindings as `index.html`, with
an explicitly exported `fixture.json`. Use a separate non-preview browser-loopback
test endpoint, as for `sound_callers`.
`test-positional-browser.mjs CDP_URL PAGE_URL WASM_FILE OUTPUT_DIR` checks executed WASM
identity and actual WebAudio graph quanta. Both drivers share the independent
preservation formula oracle in `test-positional-pcm.mjs`; run
`node dev/scripts/test-positional-controls.mjs` for literal gain/pan values and
negative controls. Gain assertions allow finite-window RMS variation; pan uses
the shared mono signal in both output channels. This fixture checks public
caller/backend behavior, not the complete authored room sequence or rendered
host camera-publication ordering.

`python3 dev/scripts/run-positional-browser.py /absolute/host/rig` recovers the
isolated browser acceptance rig from retained `web/` artifacts without compiling.
It pulls only its pinned Chrome image when absent, waits for HTTP/CDP readiness,
uses a host-mounted config directory and per-run lock, and removes its own
container/server on exit. Rerunning after a reset retains `browser-final/result.json`.
The driver validates checkpoint WASM identity and replays the fixture, comparing
previous stage state before observing the remaining phases. Every new PCM result
is checkpointed only after executed bytecode identity is verified. The bounded
replay keeps the production caller's stage progression; it does not treat host
elapsed time as a game update count.

Verified: synchronous script sound services retain exact signed-Q8 camera position
and yaw separately from the rendered transform. Camera-controller and successful
camera-native publication reach the sample owner before subsequent synchronous
sound work; positional starts and ambient refreshes use that publication. Model-presentation
phase entry replaces or clears the previous world's listener before cue draining.
The fallback transform remains available to standalone hosts without a logical
camera. This follows preservation's [listener contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/sound.md#listener).

The `cyrus-web --example listener_publication` integration probe consumes the explicitly exported local
`positional_emitters` fixture and supplies diagnostic player/camera roles. Its
seven stages run the public cameraController native hook, camera glide APIs and script sound services
against the installed emitter/settings/PCM, retaining logical/render poses in the
trace and checking the sample owner's publication before playback. The generic
browser/native drivers accept an optional stage-count argument (default thirteen);
use seven for this probe. The recoverable browser launcher accepts `--stages 7`.

The native driver accepts an optional executable path after its stage count.
With that path it binds saved observations to the executable SHA256, validates
retained levels with the same oracle, and replays public stage state before
capturing missing phases. A different executable or changed stage state rejects
the checkpoint. The listener fixture moves the initialized player through public
placement restore before invoking cameraController; it does not initialize the
player twice.

World teardown retires the entire shared sample pool, including unowned
presentation/control voices and effects, and clears browser-bank deferred starts.
The observed world generation belongs to the pool, so a validated diagnostic
session replacement does not inherit the old pool's teardown on its first drain.
This follows preservation's `engine/sound.md` world-teardown contract.

`native-ambient-lifecycle.py PORT OUTPUT --container NAME --binary PATH
--emitter NAME --effect INDEX` drives an explicitly selected installed room
emitter through public placement, camera reset, console inhibition, scene load and dormant
revisit. It captures actual Pulse stereo output, isolates a looping unowned
control sample before teardown, requires silence after retirement and checks a
new start/stop plus authored ambient continuation. The default scene pair is
ISLAND/START; emitter/effect are supplied from the install. Run it in the paced
runtime with host-mounted config and a per-run lock. `test-native-ambient-lifecycle.py`
checks audible/silent/incomplete-window failure controls. No install data is tracked.

Diagnostic sample capture rejects starts still waiting for the browser effect
bank. Those starts have not acquired a decoder/slot and have no serialized replay
contract. Rejection leaves the live queue and existing transports intact; after
ordinary bank admission, the existing decoder checkpoint can be captured. This
matches the explicit rejection policy for other in-flight audio preparation.

The browser-only `deferred_sample_capture` example takes an explicit local
installed-source fixture exported by `positional_emitters --export`. Package it
with wasm-bindgen and serve it beside `dev/fixtures/deferred-sample-capture.html`;
pass the fixture URL as `?fixture=...`. Run
`node dev/scripts/test-deferred-sample-capture.mjs CDP_URL PAGE_URL WASM OUTPUT_JSON`
against a disposable non-preview browser. It holds the normal bank-admission gate,
sends public script sound cues, and exercises public sample capture/replacement.
Actual graph PCM and transport counts check rejection continuity, once-only
admission, replacement, deferred cancellation and later playback. This controlled
audio-owner fixture does not establish full aggregate-world pending-save
reachability or physical-output parity. Native package tests cover the capture
boundary; bank deferral and its runtime PCM acceptance are browser-specific.

Browser bank preparation defers animation PlaySound commands in the same ordered
queue as script/flat starts. Deferred animation starts retain emitter identity,
requested effect, rate offset, surface and logical placement. A later PlaySound or
replacement-group Forget drops the remembered-handle intent without cancelling
older playback; EndAnimation cancels only the still-remembered start. Owned stops
and world teardown cancel queued starts as well as live channels. Admission does
not transfer a queued command to a recreated entity or a different logical owner.
Pending commands remain unsupported by aggregate audio checkpoints.

`node dev/scripts/test-animation-bank-browser.mjs CDP PAGE INSTALL OUTPUT
[LOOP_EFFECT] [SCENE] [EMITTER]` drives installed animation emitters through the
opt-in browser capture adapter, real bank reads, remembered stops and public
session replacement. It verifies actual graph PCM and executed browser provenance.

`cargo run -p cyrus-audio --example speech_teardown -- INSTALL LABEL OUTPUT`
loads one explicit installed RTX voice into a controlled script scene. It uses
`ScriptPlugin` world release/replacement, publishes a logical subtitle, and checks
that a subsequent skip edge cannot stop a replacement raw voice through an
inactive record. Run it in an independent paced runtime with host-mounted config;
`native-speech-teardown.py CONTAINER OUTPUT --binary EXECUTABLE` drives six bounded
stages and records actual stereo mixer PCM plus logical/sample ownership. Use a
fresh output directory for each run; prior stage receipts reject. The
optional example argument `--stale-record-control` restores the old inactive-slot
state through the public snapshot interface and must fail the skip-continuity PCM
stage. Output is local game data and must remain untracked. This isolated scene
check does not establish physical-speaker parity or full story-dialogue acceptance.

Browser music resolves both numeric WORLD.INI identities and installed map stems
before destination preparation. A different track retires at the load request;
the prepared world starts its configured track. Requests for the current track,
missing assignments and `-1` retain the live source, following preservation's
[world-load CD contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/music.md#track-selection).
Map stems use the first authored variant; numeric identities preserve variants.

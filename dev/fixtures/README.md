# Replay fixtures

Fixtures for repeatable comparisons between the original game, native cyrus
and the browser build. No capture, install file, emulator state or debugger
instrument belongs in Git: captures go under `magic/captures/`, and
executable-specific tooling stays under `magic/`. Background on comparisons is
in [docs/METHOD.md](../../docs/METHOD.md) and
[docs/SETUP.md](../../docs/SETUP.md).

| Fixture | Purpose |
| --- | --- |
| `belltowr.json`, `island.json`, `start.json` | Matched scene captures (below) |
| `segments/bell-monocle.replay.json` | Replay segment from BELLTOWR entry: Favis' bell topics, the bell-rope running jump and the monocle pickup (nightly) |
| `segments/ending-1-extpalac-entry.replay.json` | EXTPALAC entry; captures the session later ending segments resume (nightly) |
| `segments/sword-walk-extpalac.replay.json` | Walking while the sword is drawn (EXTPALAC): no script failure, and the player moves once the draw completes; stops the world on v1.7.0 (issue #345/#354) |
| `segments/sword-circle-extpalac.replay.json` | Armed circling (EXTPALAC, sword drawn, walk key plus Left/Right): no script failure and the player circles; stopped the world before the circle-continuation fix (#354) |
| `segments/ending-2-richton-fight.replay.json` | Richton fight resumed from that session (nightly); deterministic since the replay hold: two runs both reached Richton 220 through hits of 10 and 20 |
| `start-walk.jsonl` | Legacy control replay: START, one second walking forward |
| `*.html`, `listener-publication.rs` | Audio and animation acceptance pages and examples; see [cyrus-audio](../../crates/cyrus-audio/README.md) |

## Matched scene captures

A JSON fixture describes a world, a scene-space camera position and quaternion,
the viewport and a settling time. `just replay <fixture.json> <new-label>`
produces `original.png`, `native.png`, `browser.png` and a manifest under
`magic/captures/<new-label>/`. Existing labels are rejected.

These are scene and viewpoint comparisons. Animation and gameplay updates are
not synchronized, and settling times are host milliseconds, not original
ticks. **Unknown:** complete original/native/browser gameplay equivalence.

1. Prepare the original with the maintainer's guarded tooling bundle and
   `--static-capture`, which disables the task and animation systems in a
   writable runtime copy. Keep the install read-only. Check that F12 `oracle`
   reports task 0 and animation 0.
2. Set `CYRUS_ORIGINAL_CAPTURE` to your original-game adapter under `magic/`
   (contract below). A missing adapter fails the replay.
3. Run `just run-native --fixture dev/fixtures/<world>.json`, which loads the
   fixture's world and view and sizes the window. `CYRUS_CONTROL_ADDR` and
   `CYRUS_CAPTURE_NATIVE_CONTAINER` select another instance or container than
   `cyrus-dev`.
4. Build and serve the browser build (`just web-build`, then
   `just run-web --install /absolute/path/to/Redguard`). Set
   `CYRUS_CAPTURE_WEB_URL` to the game URL with an explicit `?install=` URL.
5. Set `CYRUS_CAPTURE_CDP` to a dedicated Chromium DevTools endpoint (setup
   below). Node 22 or newer and ImageMagick are required.
6. Run `just replay dev/fixtures/<world>.json <new-label>` and inspect all three
   images. Reject a triplet whose original image still shows loading artwork.

`just retro --fixture <fixture.json> <new-label>` runs only the original
adapter. Replay waits for each side's world to finish loading before settling,
and rejects scene changes, reloads, a mismatched view or inconsistent clocks
instead of producing a misleading triplet. Native frames come from
`capture_gameplay_frame`; the browser exposes `window.__cyrusCapture` with
`?capture=true`.

**Verified:** `belltowr.json`, `island.json` and `start.json` record measured
original camera poses: BELLTOWR frames the door and table, ISLAND terrain and
nearby barrels, START the ship deck and water. ISLAND loads twice, so the
adapter must reopen F12 after each load. Do not use them to claim animation or
script fidelity.

### Original adapter contract

The executable receives absolute paths to the fixture and a new output
directory. It drives the original through the fixture's console setup, waits
for the world to load, pauses at the measured view and writes fresh
`original.png` and `original.json`. The JSON contains `world`, `view` (seven
scene-space numbers) and `image_sha256`, plus the emulator identity,
measurement method and setup observations. It returns nonzero when loading or
measurement fails, releases held keys when resuming, and never writes to the
linked install. Copied screenshots and emulator save states are not live
captures. Measure the view with the `cyrus-playtest` skill's debugger
procedure; keep probes and raw measurements under `magic/`.

For selected-player comparisons, the adapter can also provide `player_pose`:
`{"name":"CYRUS","frame_reference":412,"position":[100,-200,300],"angles":[0,512,0]}`.
These example values are synthetic. The adapter must derive the actual name,
RAAN frame reference, Q8 effective position and engine angles from the same
presentation boundary as the original image. Native admission compares them
exactly against the image's render-owned player receipt; a preceding control
state or an animation group/progress value cannot substitute for that receipt.
The reference spans RAAN entries in their installed order, as
`Animations::resolve_frame` documents. A local frame index or runtime model
handle cannot be compared directly with the native `animation.frame` reference.
The adapter must resolve the original model identity and add preceding RAAN
frame counts before supplying `frame_reference`; an ambiguous `frame` field
is rejected.
A rejected image stays available for diagnosis but produces no accepted manifest.
This optional check binds the selected player's frame and placement, not runtime
model-handle resolution, other actors, or browser animation phase. Captures
without `player_pose` retain only the scene/viewpoint guarantee.

### Headless capture browser

Serve the browser build with `--bind 0.0.0.0` and use the port from
`vibepod-preview status` as `capture_port`:

```sh
docker run -d --rm --name cyrus-capture-browser \
  --add-host host.docker.internal:host-gateway -p 127.0.0.1:9222:9222 \
  chromedp/headless-shell@sha256:f70908c8020555a3bf89a070140913b443a03ead28c1a66563c3912a3b32b08d \
  --enable-unsafe-swiftshader --remote-allow-origins='*' \
  --unsafely-treat-insecure-origin-as-secure="http://host.docker.internal:$capture_port"
export CYRUS_CAPTURE_CDP=http://127.0.0.1:9222
export CYRUS_CAPTURE_WEB_URL="http://host.docker.internal:$capture_port/?install=http://host.docker.internal:$capture_port/original/"
```

The secure-origin override enables the asset service worker for this local
server only. Stop the container with `docker stop cyrus-capture-browser`.

## Build records

`dev/scripts/build-native.sh` (called by `run-game.sh`) writes
`magic/build-tools/native/cyrus.provenance.json`; `build-web.sh` writes
`target/web/build-provenance.json`. Each holds artifact hashes, the Git
revision and a fingerprint of the build inputs. Replay rejects missing records,
changed sources or artifacts, a different running native binary, and browser
JavaScript or WASM whose observed hashes differ from the record. Records
identify inputs and outputs; they do not claim a reproducible build.
`CYRUS_NATIVE_EXECUTABLE` and `CYRUS_NATIVE_CHECKPOINT_DIR` relocate the
retained native binary and its recovery directory.

```sh
python3 dev/scripts/build-provenance.py verify magic/build-tools/native/cyrus.provenance.json
python3 dev/scripts/build-provenance.py verify target/web/build-provenance.json
```

## Legacy fixtures

`.jsonl` fixtures send control commands and sleeps to a running native game,
then call `compare.sh`, writing current-view screenshots under
`magic/captures/legacy/`. They do not establish a matched original viewpoint.
`CYRUS_COMPARE_TOLERANCE` sets the baseline image tolerance.

## Acceptance drivers

- `python3 dev/scripts/dial-acceptance.py --port 7117 --case rune|empty` drives
  CATACOMB `CTDIAL01` through the control port. Use a fresh
  `--scene CATACOMB` process per case; `empty` is the negative inventory
  control. It uses diagnostic placement and inventory setup, not a natural
  route, and does not check timing, artwork or audio.
- `python3 dev/scripts/torch-acceptance.py --addr 127.0.0.1:7167` checks the
  burning-torch counter with `goldfinger` off and on in a fresh `START` world on
  dry ground, and restores the prior values. Lighting input, artwork, audio and
  water branches need separate checks.

Both print JSONL observations on stdout; redirect them into an ignored artifact
directory.

## Testing

```sh
node --test dev/scripts/matched-capture.test.mjs dev/scripts/capture-provenance.test.mjs
python3 dev/scripts/test-build-provenance.py
python3 dev/scripts/test-dial-acceptance.py
python3 dev/scripts/test-torch-acceptance.py
```

`node dev/scripts/test-browser-provenance.mjs` checks browser byte observation
against a dedicated Chromium endpoint (`CYRUS_PROVENANCE_CDP`,
`CYRUS_PROVENANCE_HOST`) without game files.

### Static sky comparisons

An adapter comparing a sky with movement disabled must stop native sky motion
before setting measured texture offsets. `synchronizeStaticSky(direction, offset,
send)` from `dev/scripts/matched-capture.mjs` submits `fxskyspeed 0`, requires its
successful console report, closes the console and then submits `set_sky_scroll`.
The `send` callback uses the normal control-port command contract. Setting offsets
alone lets the next presentation tick move them again.

Read back both camera poses and inspect the original image before choosing an
unobscured sky rectangle. A matching pose or a low pixel error does not establish
that the rectangle contains sky. This native adapter helper does not establish
browser acceptance or acceptance of the separate indexed software-sky path.

Entry scripts can retain control after a world transition. An
`{"await_control":{"world":1,"max_seconds":600,"max_updates":10000}}`
step waits for the scripted lockout to clear using one ordinary update at a
time. The limits measure game seconds and completed updates, so slow rendering
cannot shorten the allowed script duration. It sends no keys and never clears
lockout itself. Optional `"rope_attached":true` also requires the selected
player to be attached before climbing inputs continue. Optional
`"inventory":{"46":1}` waits for the script to grant the item before its
checkpoint; it does not grant items itself. A stalled ordinary boundary remains
an error. Use a separate
`modal_input` step for a scripted viewer that suspends ordinary updates.

Independent quest segments live in `segments/*.replay.json`. Default
`just replay-chain` discovery runs the chapter chain first, then each segment
in a separate process and config directory. Explicit fixture arguments still
share one session. A segment with `"entry_scene":"BELLTOWR"` uses the installed
world's ordinary scene entry, with fresh quest state; it never loads a private
save or places the player. Staged segments cannot be mixed into a chapter chain.

Ending sub-segments chain through captured sessions, so each launch stays short.
`{"capture_session": "name"}` saves the live session under `sessions/` next to the
segment's config directory; a fixture with `"resume_session": "name"` (and a staged
`entry_scene`, currently BELLTOWR or EXTPALAC) loads it into a fresh launch, then
waits for asynchronous model and geometry work to settle before any input. Chain the segments with one launch each, in file order
(`ending-1-…` before later ending segments); default discovery does so. Sessions are
local run state and never tracked. `setup` (console lines, a placement, original cheat values such as `drevil`) is a test-only
shortcut for prerequisites the segment does not play, such as the Richton item;
it is never used for quest flags. Cheats persist in the config directory, so a fixture that
sets one must reset it in a later `setup` step. `fight` draws the sword and uses
bounded `taps`, `busy-gate`, or `idle-tap` input policies, reading health between
short schedules. The policy and recording/admission/session-regeneration procedure
are documented in [cyrus's replay README](../../crates/cyrus/README.md#recorded-replay-chain). It requires whole ten-point
opponent losses, a surviving player and the target health (zero accepts the defeated opponent
leaving the actor list) inside a game-clock bound, and prints its progress every ten cycles.
The runner always uses the fixture's fixed step, so the fight does not depend on wall
clock. **Not covered:** the final kill and the ending movie.

A captured session is stamped with the digest of the executable that captured it
(`<name>.session.build`), and the runner refuses to resume it in a different build, naming both.
The session format itself is one version across builds (players' saves load in later builds); the
stamp only keeps replay A/B and bisection runs from mixing two builds' behaviour. Builds before
065dbf8ae also fail any restored session over a scene entry, so capture and resume with a build
that has it.

`ReplayHold` stops ordinary gameplay between scheduled boundaries. Replays use
simulated game ticks rather than the host clock. Under `--replay-step-micros` the
world advances only while a scheduled input boundary owns the frame loop and while no speech
line is still exporting or loading (speech readiness would otherwise depend on wall-clock load
time). The clock's BIOS tick is also derived from the simulated time instead of the host's
absolute elapsed time. **Verified** in the original's documented behaviour: combat pacing and
timed taunts read the cached raw BIOS-clock sample, interpreted as signed (preservation
`engine/SOUP.md`, decision-entry and taunt sections), and the FPS measurement counts windows of
18 BIOS ticks (`engine/player.md`, "FPS-derived frame scale"). The original's tick has an
arbitrary hardware origin, so its phase against the updates is not specified behaviour; the
host tick that normal play keeps is the faithful source, and the simulated tick exists only
so two replays of one capture make the same pacing decisions. **Unknown:** whether any other
host-dependent input remains; compare `state.random_state` per update between two fresh
processes (see crates/cyrus-control/README.md) to check.

`topic` steps select an exact displayed dialogue label using balanced arrow and
Control input, waiting with idle updates while speech runs. `face` steps turn
through ground or rope controls and observe the resulting heading. Both have
update bounds and leave script state untouched. Checkpoints may additionally
assert `health`; every checkpoint requires zero script failures.

Segment steering can record `forward_updates` and `settle_updates` to keep
short walking presses and their release interval explicit. Chapter steering
retains its existing adaptive press duration when these fields are absent.

`stop_updates` waits with all steering keys released before accepting a waypoint,
then measures the resulting position again. Overshoot resumes correction instead
of accepting the earlier moving position. BELLTOWR uses 50 updates: its measured
walk release travelled 13,768 Q8 and stopped by update 16, remaining stationary
through update 50. This interval is a fixture observation, not a gameplay delay.

`approach_radius` switches the last part of a waypoint to single-update walk
presses followed by `stop_updates` released updates. This keeps the final
approach stationary between corrections, including when the outer gait runs.

`stride_correction: true` opts a stopped approach into measured-stride steering.
When one completed walk would skip the entire target region, the runner aims
for the first leg of two walks instead of reversing along the same line. It
measures the released displacement and checks the next position through control
state. This needs `approach_radius` and `stop_updates`; all movement still uses
keyboard input, and the target tolerance stays unchanged.

## Original-first gameplay comparisons

A native-only chain pass is regression coverage. Gameplay acceptance additionally
requires the original first and cyrus second, with the same world, start state,
inputs and game-frame timing. Record player/actor positions, health, animation
groups, control state, flags and prompts on both sides. Keep screenshots for
visual checkpoints and post both measurements on the issue.

`dev/scripts/compare-segment.py` executes an **expanded input tape** from the
replay format (`input` and `checkpoint` steps), original first, and writes one
`comparison.json` with both snapshots and signed per-field checkpoint deltas:

```sh
CYRUS_MAGIC=/absolute/host/path/magic python3 -B dev/scripts/compare-segment.py \
  magic/comparison/favis-inputs.replay.json \
  --original-backend magic/comparison/original_backend.py \
  --native-backend magic/comparison/native_backend.py \
  --out magic/comparison/favis
```

The two adapters provide `open_session(segment, output_directory)`, a context
manager yielding `snapshot()`, `advance_to(relative_frame)` and
`input(KeyCode, "press" | "release")`. They prepare matching start states, set
frame zero after preparation, release keys and stop the owned process on exit.
Original adapters use `original-scenario.py`'s `Original` and the streaming reader
from `original-fight.py`; executable bindings remain under ignored `magic/`.
The reader should batch all requested actors and flags in each observation.
`original-scenario.py` also provides `FrameSession(game, project)` for real key
input and frame-counted waits over that stream; `project(raw)` normalizes the
local reader without substituting values for missing observations.
Native adapters drive the control port using a host-mounted `CYRUS_CONFIG_DIR`.
Use the original's frame counter and measured simulation timing, not host sleeps.

For strict input timing use `original-frame-session.py`'s stopped `FrameSession`,
not the streaming OS-key adapter. Supply an attached `Watchpoint` stopped at a
verified frame-counter boundary, a counter projection and observable projection.
It reads every intervening frame while stopped and rejects a counter gap or reset;
it never interpolates flags, prompts, animation or control ownership.
`Watchpoint.latch_key` writes an ordinary key's documented 16-bit make/break slot
and reads both the slot and frame counter back before execution can continue.
The ignored binding's `keys` map supplies each KeyCode's `guest_offset` and
`reader` dict/list path. Interrupt-special keys require a separate adapter.
This is diagnostic key-state injection, not proof of OS keyboard delivery.

`compare-segment.py`'s `NativeFrameSession(control, project)` provides the native
half after staging. ReplayHold must be active: inputs latch at the current
boundary and each observation-only schedule consumes exactly one update. It rejects script
failures, actor-list truncation and unexpected simulation progress. Both adapters
leave launch, exact-pose staging and process cleanup to the scenario backend.
Record consumed clock steps as well as frame indices: stopped original inspection
does not establish equal BIOS tick inputs or equal native simulation duration.
For measured clock playback, pass `frame_scales=[...]` to `NativeFrameSession`.
Each entry is the original's consumed Q8 movement scale for one update, including
zero. The adapter issues `replay_frame` under explicit `--replay-step-micros`
mode and rejects an exhausted tape. The clock consumes the scale directly, without
converting through milliseconds; nominal and fine tick carries still require a
matched initial clock continuation. This diagnostic mode does not change normal
Faithful timing. An equal duration tape alone does not establish matched starts.

`Original.capture_quicksave(destination)` requests F9 and copies the private slot
only after its changed content remains stable for two game frames. Its caller must
first enable the original's quick-save consumer and reload that save on both sides
before tape zero. Before opening Load, call `Original.wait_reload_boundary()` to
let the existing world fade and clock warmup finish through ordinary progress.
The local reader must expose `counters.world_fade` and
`counters.clock_continuation.warmup`; missing bindings reject admission. A save
reload does not make an active presentation fade part of the saved clock. Do not
reset warmup to hide an unmatched start. Enabling `final_version` before boot selects retail startup; it
is unsuitable for direct debug-world entry. Save capture is staging, not a matched
start or a completed import of actor animation and task continuations.

Snapshots contain `frame`, `world`, `player`, `actors`, `flags`, `prompts` and `timing`.
Player and actor records use Q8 positions, engine headings, health and animation
groups; include controller state and consumed timing in the records. Declare
unmeasured values as `null`; two unknown values never count as a match. Include
local screenshot paths for visual checkpoints and compare those images as well;
field equality alone does not establish visual acceptance.

The runner holds a per-output lock, completes and closes the original session
before opening cyrus, and refuses mismatched initial observations or skipped
frame boundaries or late input acknowledgements. `input` returns the relative
frame after key submission; missing acknowledgements are rejected. It compares only identical explicit inputs: `steer`, `topic`,
`repeat`, session restoration and arrival predicates must first be expanded into
the recorded key tape, rather than independently making decisions in each game.
Unsupported steps are rejected, never silently omitted. Reports distinguish
`match`, `different` and `blocked`; exit zero requires matching measured fields.
Use one heavy run on the pod at a time and coordinate the slot on #322.

### Restoring the retro image after a pod replacement

Run `dev/scripts/ensure-retro-runtime.sh`. It fingerprints the Dockerfile,
entrypoint and DOSBox configuration, checks the exact image, restores its retained
host archive when absent, or builds and retains it once when no archive exists.
Concurrent recovery calls share a build lock. The image name is printed on stdout.
Set `CYRUS_MAGIC` to the shared host research root to reuse its cache across
worktrees; archives live in `magic/build-tools/`, outside ephemeral Docker storage.
`run-retro.sh`, `original-scenario.py`, `original-fight.py` and the paired runner
perform this preflight before each launch. Changes to runtime sources select a
new cache key. This image contains tools only, never an installed game.

The scheduled wrapper downloads a main-push Linux artifact without a local build.
If the current main build has not uploaded it yet, `--fallback-main` searches the
latest 50 main-push runs for the newest eligible ancestor with an unexpired Linux
artifact. It never selects a newer or unrelated commit. The retained `artifact.log`
records the binary commit and workflow run separately from the checkout commit;
include both when reporting a chain result. If no eligible upload exists, the run
fails clearly instead of building locally. The downloader remains exact-commit
by default for manual proofs.

A strict paired backend also exposes `continuations`, keyed by actor identity,
with normalized script cursors and all retained task slots, including instruction,
invocation counter, blocking status and pause state. Both backends must bind these
from their live state. Matching position, animation and clock alone cannot admit
a shared-save start: a limited original import can retain native baseline tasks.
Missing or different continuations reject the comparison before native input.

### Bounded original route blocks

`dev/scripts/original-route-input.py` applies one fixture `input` block through
an already prepared `original-frame-session.FrameSession`. Offsets are relative
to the current stopped boundary; all offsets are checked before any input.
Every key needs an exact-frame acknowledgement, world changes reject the block,
and held keys are released on success or failure. The caller supplies an
explicit update bound and retains the start/end observables and input receipts.
Use the existing opening fixture's combat block after acknowledged
retail entry; do not assume F3 opens Load during the opening duel. This helper
plays one block, not conditional/repeated chapter steps or a complete route.
An original-only route checkpoint prepares evidence for a comparison; it does
not establish native parity. Its script tests run in `just check`.

For a retained original checkpoint, stage its private slot and open the retail
Load page. `load_page_ready` requires the constructed page and selection, not a
pending menu context. `restore_checkpoint` temporarily binds a locally evidenced
restore-to-pre-clock write sequence, confirms Load, and checks decoded world, selected
player, relevant actors and clock continuation from `rgpre`. Supply the evidenced
`stages` sequence of `(binding, write_count)` pairs for that Load path:
earlier loader-entry and retained-global writes may touch the same location.
Clock totals can already
match while actors are still absent, so they cannot select the boundary. The
final stage must follow actor restoration and precede ordinary clock/actor
updates; any observable mismatch there rejects immediately. The bounded write
sequence is executable evidence kept locally, not a gameplay timing constant.

Both accumulated clock totals must equal the saved values with **zero** resumed
steps, and positions, heading, health and animation group must match exactly.
The helper releases Return on success or failure and restores the ordinary
watchpoint while the emulator remains stopped. Only the caller's next explicit
advance executes the first resumed update. Keep executable bindings, receipts
and decoded expectations outside tracked files. `Watchpoint.select_boundary`
changes a stopped write watchpoint without running the emulator; a binding alone
does not establish restoration, so retain the exact observable checks.

The older `resume_checkpoint` ordinary-boundary helper expects one accumulated
clock step and unchanged actor state. It is suitable only when actors stay
unchanged across that step, not as a restored-state boundary for moving scripted
actors. Do not widen tolerances or treat the next loop-counter boundary as an
untouched save. The original reload check does not assert complete native import
fidelity.

A scripted `ShowItem` pauses ordinary frame advancement. Use
`dismiss_scripted_item` with the locally bound pending-item write and modal
clock, the requested item identity, and the documented opening animation's
update count. Bound any ordinary updates before the request separately with
`request_updates`; its observed frame becomes the modal frame. It waits for
that request, checks its item identity at the first modal clock boundary (the
pending-flag write can precede argument assignment), advances the opening, holds the close key across one modal update, releases it, and
requires the pending flag to clear at the next ordinary frame. Wrong items,
world changes, frame gaps and failed acknowledgements reject the phase; a
failure after pressing releases the key while stopped. This is a modal input
adapter, not a wall-time key hold or permission to skip dialogue.

A fixed combat tape's acknowledgements do not establish damage or route
progress. For a resumed retail combat phase, use `play_checkpointed_phase` with
an explicit state predicate (an opponent's health decreased, or an installed
script progression flag changed). It inspects every consecutive boundary,
rejects unexpected world changes before checking completion, stops at the first
checkpoint and releases held input. A checkpoint already satisfied at entry
returns without input, so reloading a later checkpoint does not replay an earlier
phase. Exhausting the update budget fails with the phase name instead of blindly
repeating the combat block.

`aimed_combat_keys` provides a retail driver policy: turn toward the opponent,
release turning when inside the installed attack arc, approach if beyond the
installed range, then apply the admitted attack tape's keys. Supply the first
motion's profile from `rgpre`'s `COMBAT.INI` decoding and the actor's move bindings;
there are no fallback combat ranges or damage expectations. Its heading estimate
steers input; the original's observed health/flags decide acceptance. Keep the
attack cadence from the fixture, but do not hold its turn key throughout a retail
attack merely because that native tape previously passed. Retain phase receipts,
per-frame poses/animations, and the exact saved-state reload receipt together.

For an installed script's automatic world change, use
`FrameSession.wait_for_world(target_world, max_updates, checkpoints=...)` after
its same-world control checkpoint. Optional ordered `(name, predicate)` entries
must be observed before the target world appears; a world number alone cannot
stand in for missing dialogue. Supply predicates from the installed sequence
and reader observables, keeping labels and captured data outside tracked files.
The wait sends no input. Natural speech completion needs no skip key; if a skip
is required, the documented modifier edge applies only under script lockout and
must have its own acknowledged press/release and completion checkpoint.

Source-world counters stay strictly consecutive. Only the explicitly requested
world may change the counter continuation; that first target boundary establishes
a new relative frame zero. Unexpected worlds and same-world counter gaps still
fail. An already loaded target skips the earlier phase, permitting resume from a
later save. The arrival receipt acknowledges the transition, **not** a ready
world: the first target boundary can still have no selected player. Observe
player allocation, fade/warmup completion and control or modal state before
saving or sending gameplay input. Retain and actually reload the next save
through retail Load before calling it reusable.

Checkpoint staging preserves the main SAV, journal and per-world TSG sidecars;
after a scripted world change those sidecars retain previous-world state. The
stager validates regular source files before replacing the private config/save
directory, rejects linked checkpoint files, and copies no unrelated captures.
This uses preservation's documented save-slot layout rather than treating a
current-world SAV as the entire multi-world checkpoint.

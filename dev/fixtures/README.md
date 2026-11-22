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
| `pending/ending-2-richton-fight.replay.json` | Richton fight resumed from that session; **pending**: on the shared CI binary three runs ended in no hit, one hit and player death, so it is outside the nightly chain until the fight is reproducible |
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
sets one must reset it in a later `setup` step. `fight` draws the sword and taps attack once per
eight updates, reading both healths after every update. It requires whole ten-point
opponent losses, a surviving player and the target health (zero accepts the defeated opponent
leaving the actor list) inside a game-clock bound, and prints its progress every ten cycles.
The runner always uses the fixture's fixed step, so the fight does not depend on wall
clock. **Not covered:** the final kill and the ending movie.

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

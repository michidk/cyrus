# Matched scene captures

JSON fixtures describe a world, scene-space camera position and quaternion,
viewport, and settling time. `just replay <fixture.json> <new-label>` produces
`original.png`, `native.png`, `browser.png`, and a manifest under
`magic/captures/<new-label>/`. Existing labels are rejected. No capture, install
file, emulator state, or debugger instrument belongs in Git.

Inspect the original image for a fully loaded scene. A measured camera pose and
nonuniform pixels can also occur behind loading artwork; they do not establish
rendering readiness. Reject that triplet and preserve it under a rejected label.

These are scene/viewpoint comparisons. Animation phases and gameplay updates are
not synchronized. Settling times are host milliseconds, not original simulation
ticks. **Unknown:** complete original/native/browser gameplay equivalence.

1. Start the original with `just retro` and enter gameplay. Use a separate named
   instance when another session owns the default container.
2. Set `CYRUS_ORIGINAL_CAPTURE` to your executable original-game adapter under
   `magic/`. The adapter contract below keeps executable-specific instrumentation
   outside tracked files. Missing instrumentation fails the replay.
3. Run `just run-native --fixture dev/fixtures/<world>.json`. This supplies the
   fixture's world/view and resizes the window. `CYRUS_CONTROL_ADDR` selects the
   native control endpoint for an independently managed instance.
   Set `CYRUS_CAPTURE_NATIVE_CONTAINER` too when using a container other than
   `cyrus-dev`. The runner requires a local Docker endpoint and checks its port
   binding, container identity and running `/proc/1/exe` hash before and after
   capturing. A matching executable on disk is insufficient.
4. Build the browser with `just web-build`, then serve it with
   `just run-web --install /absolute/path/to/Redguard`. Set
   `CYRUS_CAPTURE_WEB_URL` to its game URL, including an explicit `?install=` URL
   reachable from your capture browser.
5. Set `CYRUS_CAPTURE_CDP` to a Chromium DevTools HTTP endpoint. Node 22 or newer
   supplies the WebSocket client; ImageMagick checks image dimensions. Use a
   dedicated browser for this automated replay. Interactive inspection uses the
   shared T3 browser. The runner creates and closes its own browser tab.
6. Run `just replay dev/fixtures/<world>.json <new-label>`. Inspect all three
   images. The manifest records image hashes, fixture hash, source/build records,
   native state, browser version and observed camera poses.

`just retro --fixture <fixture.json> <new-label>` runs only the original adapter.
The native application must already be running with the fixture's view for a full
replay. A different world or camera fails instead of producing a misleading pair.
Browser `?capture=true` exposes read-only `window.__cyrusCapture` only after entering
a world. The replay validates this observation before capturing.

## Build records

`run-game.sh` calls `build-native.sh`, which writes
`target/debug/cyrus.provenance.json`; `build-web.sh` writes
`target/web/build-provenance.json`. Each records SHA-256 hashes of the produced
artifacts, the Git revision, and a fingerprint of repository build inputs:
Cargo manifests/lockfile, the toolchain file, `.cargo/`, `crates/`, and `dev/`.
Untracked input files are included. Python bytecode caches are excluded; input
symlinks are rejected. Install data and captures are outside this scope.
The build fails if this snapshot changes during compilation.

Check an existing record without starting a game:

```sh
python3 dev/scripts/build-provenance.py verify target/debug/cyrus.provenance.json
python3 dev/scripts/build-provenance.py verify target/web/build-provenance.json
python3 dev/scripts/test-build-provenance.py
node --test dev/scripts/capture-provenance.test.mjs
```

To rebuild without launching either game, run `bash dev/scripts/build-native.sh`
and `bash dev/scripts/build-web.sh`. Both use this checkout's `target/` directory.

These records identify repository inputs and resulting bytes; they do not claim
a hermetic or bit-for-bit reproducible compiler environment. Replay rejects
missing records, changed source, changed artifacts and a different running binary.
Native and browser builds must have the same source snapshot as the checkout.

Build records include the generated wasm-bindgen JavaScript snippets loaded by
the application.

Each browser replay uses a new isolated browser context. Chromium's debugger
reports the executed JavaScript and compiled WASM bytes from both the page and
asset worker. The runner compares their hashes with the build record and checks
the actual HTML/CSS responses. Refetching a URL is not proof of previously loaded
bytes. Missing observations, unexpected application scripts, and mismatched
hashes reject the triplet. Context disposal closes its tabs and workers.

The synthetic integration test exercises this observation against a dedicated
Chromium CDP endpoint without loading game files. Set `CYRUS_PROVENANCE_CDP` to
that endpoint and `CYRUS_PROVENANCE_HOST` to this machine's address reachable from
the browser, then run `node dev/scripts/test-browser-provenance.mjs`. The test
starts and closes a temporary HTTP server. It verifies both page/worker modules
and rejects deliberately changed JavaScript and worker WASM. Stop the dedicated
browser afterward. Unit tests also run in `just gates`.

## Starter views

**Verified:** `belltowr.json`, `island.json`, and `start.json` record measured
original camera poses. Their original-game setup explicitly sets `task = 0` and
`animation = 0` before loading. Native/browser may advance supported runtime
updates; the fixtures do not synchronize animation or gameplay. BELLTOWR frames
the door and table, ISLAND frames terrain and nearby barrels, and START frames
the ship deck and water. Do not use them to claim animation or script fidelity.

## Headless browser setup

For a Docker capture browser, serve the application with `--bind 0.0.0.0` and use
the actual port printed by `vibepod-preview status`. The loopback default remains
appropriate for shared T3 browser checks. For example, after setting
`capture_port` to that port:

```sh
docker run -d --rm --name cyrus-capture-browser \
  --add-host host.docker.internal:host-gateway -p 127.0.0.1:9222:9222 \
  chromedp/headless-shell@sha256:f70908c8020555a3bf89a070140913b443a03ead28c1a66563c3912a3b32b08d \
  --enable-unsafe-swiftshader --remote-allow-origins='*' \
  --unsafely-treat-insecure-origin-as-secure="http://host.docker.internal:$capture_port"
export CYRUS_CAPTURE_CDP=http://127.0.0.1:9222
export CYRUS_CAPTURE_WEB_URL="http://host.docker.internal:$capture_port/?install=http://host.docker.internal:$capture_port/original/"
```

The secure-origin override is limited to this local test server and enables the
asset service worker. Keep the image's default debugging ports. Choose a free
host port/name if another session owns these. Stop your container with
`docker stop cyrus-capture-browser` after capturing. Browser startup waits are
bounded; failure logs remain beside any partial captures.

## Original adapter contract

The executable receives absolute paths to the fixture and a newly created output
directory. It must drive the original through the fixture's console setup, wait
for world loading, pause at the measured viewpoint, and create fresh
`original.png` and `original.json`. The JSON contains `world`, `view` with seven
scene-space numbers, and `image_sha256`. Include the executable/emulator identity,
measurement method and setup observations as additional fields. Return nonzero
when loading or measurement fails. A copied reference screenshot is not a live
capture. The runner checks the measured view, image hash and viewport.

Use the `cyrus-playtest` skill's debugger procedure to measure the original view.
Keep executable-specific probes and raw measurements under `magic/`, archive the
research text privately, and record only camera poses/setup in these fixtures.
Maintainers can retrieve the verified adapter and setup scripts from the private
[research snapshot](https://github.com/michidk/redguard-decomp/tree/6289acd4acb6ac06a394dcfe7d18e697fbdd3d9d/research/supplements/2026-10-13-stage0-tooling).
Copy those instruments under `magic/` and follow the snapshot setup instructions.
The private [restoration procedure](https://github.com/michidk/redguard-decomp/blob/main/research/supplements/2026-10-13-stage0-final-verification/magic/artifacts/2026-10-13-stage0-followups/restore-verified-runtime.md)
provides a restore command that checks archive and executable hashes, copies the
install into a fresh writable runtime, and restores the instruments without
launching them. Keep its updated adapter beside the restore script. It requires
the recorded debug-enabled emulator binary. **Unknown:** reconstruction of that
emulator from source and original CD/audio parity.

The adapter must release held keys when resuming and must not write to the linked
install. Use the original's writable runtime copy. Emulator save states are not
accepted as evidence of a fresh world render.

## Legacy fixtures

`.jsonl` fixtures still send control commands and sleeps to a running native game,
then call `compare.sh`. Their two current-view screenshots go under
`magic/captures/legacy/`; they do not establish a matched original viewpoint.
Baseline image comparisons still use `CYRUS_COMPARE_TOLERANCE` and local baseline
images. Test pose rejection with `node --test dev/scripts/matched-capture.test.mjs`.

Replay waits for the current render instance to finish spawning on native and browser before applying the fixture settling time. Entering `InGame` alone can precede asynchronous geometry loading. Failed or unfinished loading does not produce an accepted triplet.

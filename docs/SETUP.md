# SETUP — running cyrus and the original

For player controls, saving, resuming and current limitations, see
[Playing cyrus](PLAYING.md). This guide covers setup and developer tools.

## You need

- The GOG release of *The Elder Scrolls Adventures: Redguard*. Buy it; the
  installer is an Inno Setup executable containing a `Redguard/` directory
  with `fxart/`, `maps/`, `RGFX.EXE` and the CD image `game.gog`/`game.ins`.
  On Linux extract it with `innoextract` (also available in Docker:
  `docker run --rm -v "$PWD:/w" -w /w alpine sh -c 'apk add innoextract && innoextract -d extracted setup_*.exe'`).
- Stable Rust (`rust-toolchain.toml`), `just`, and for a window Bevy's usual
  Linux libraries (alsa, udev, wayland, xkbcommon dev headers).
- Docker, for the original game and for the virtual-display runner.

### Native Windows verification

Use the native Rust/MSVC toolchain and keep stable Rust current enough for the
pinned Bevy release. In PowerShell, set the install and bound build parallelism:

```powershell
$env:REDGUARD_DIR = 'D:\Games\GOG Galaxy\Redguard\Redguard'
$env:CARGO_BUILD_JOBS = '2'
$env:RUST_MIN_STACK = '8388608'
cargo build -p cyrus
```

For install-dependent tests, this variable names the inner game directory
containing `RGFX.EXE`, `maps` and `fxart`, rather than its outer GOG folder.

The default parallel workspace test build can exhaust Windows' paging-file
capacity; two build jobs avoid starting many compiler processes together.
The shell recipes require Git Bash and a working `python3.exe` on `PATH`, rather
than the Windows Store execution alias. Invoke the gate with Git Bash explicitly:

```powershell
just --shell 'C:\Program Files\Git\bin\bash.exe' --shell-arg=-c gates
```

The POSIX cargo-wrapper test uses `pty`/`termios` and cannot run under native
Windows Python. Container runners also require a running Docker Linux engine.
Quit an owned native game before rebuilding its executable: Windows prevents
the linker from replacing a running `cyrus.exe`.

## Link the install

```sh
ln -s /path/to/GOG/extracted/app/Redguard redguard   # or: export REDGUARD_DIR=...
```

The link is gitignored. cyrus only reads from it.

## Run

```sh
just run                 # splash/title, then Introduction for a fresh native launch
just run --skip-startup   # skip splash, prepare menu assets behind the title
just run --new-game       # New Game from the book, with its introduction movie
just run --new-game --skip-movies   # ... entering the world straight away
just run --movie 0        # play Movies-page cinematic 0, then return to the book
just run --load-slot 3    # restore the newest save in slot 3 (`--list-slots` prints them)
just run --scene START
just run --scene 27       # WORLD.INI ID selects the night island variant
cargo run -p cyrus -- --export-all-worlds # rebuild all worlds and exit
cargo run -p cyrus -- --export-startup    # export startup, menu, starting world, cinematics and HUD for the browser
just run-headless        # no window: preload + scene export + control server
just run-native          # in dev/docker/dev with a virtual display, no preview: control port 7117 for MCP and compare
```

`dev/scripts/start-duel.py --port 7117` drives a running native cyrus (launched with `--skip-startup`
and no `--scene`) through the START pirate duel with installed keys only and exits 0 once both
pirates are defeated; the story then loads the next world by itself.

Compare movement and collision against the original at an equal update rate. Both builds step by the
frame scale the clock measures (the original shows 16 to 23 frames a second in its overlay; the dev
container renders about 6, which the clock clamps at `min_frame_rate`), and contact sequences depend on it:
on the HIDEINT boat walk the original itself ended in two different places in two runs, and cyrus
reproduced the second at frame scale 192. Run cyrus on a scratch copy of the install (under
`/home/vibepod/workspaces`, so Docker can mount it) whose `SYSTEM.INI` sets `min_frame_rate=16`: the clock then
steps at 192 even when the host is slower, and game time runs slower than wall time.

`dev/scripts/world-sweep.py --port 7117 --seconds 60 START ISLAND DRINT` loads each named world
through the same port, lets its scripts run and prints every actor whose script failed (exit 1 if
any); `--flag CATACOMB:scene2=1` first sets a global flag through the console to start the story
scene that polls it; add `--expect-transition` when that scene ends in a world change (the usual
outcome), and the sweep follows it into the destination world and inspects that world instead of
reporting the replacement as a failure. Run native cyrus with a real-time audio sink: ALSA's `null` device drains faster than real time, which
shortens every voiced line (**Verified:** the START story from the last kill to the world change took
30 game seconds with it and 43 with a paced sink, the original's 41 to 45). In the dev container,
prefix the command with `paced-audio.sh`, which starts a PulseAudio null sink behind ALSA's default device.
The sink uses signed 16-bit stereo at 48000 Hz, matching the monitor calibration signal.
An unspecified sink rate can select 44100 Hz and resample a 48000 Hz calibration twice,
so an unchanged player volume does not establish unity gain.

For native book PCM acquisition, recover the runtime with
`dev/scripts/ensure-book-audio-runtime.sh IMAGE /absolute/path/to/local-image.tar`.
It restores the supplied archive or builds `Dockerfile.sound-acceptance`, checks software
Vulkan and the PulseAudio ALSA plugin together, and seals a newly built image only after
that check passes. Mount the current `paced-audio.sh` when reusing an older image.
Use the same live sink for calibration and the acknowledged public transition; retain
its sink/player settings and reference hashes. A paced recording alone does not admit
waveform continuity or calibrated production gain. The transition recorder retains its
monotonic PCM/input epoch in `result.json` so separately sampled game clocks can be
bound to that recording without reconstructing wall time.

`paired-book-audio.py` records a known unity signal and a public transition in one
monitor WAV while an independent clock observer runs. Use a private paced runtime
and a host-mounted evidence/config directory:

```sh
python3 paired-book-audio.py --producer dosbox-x \
  --clock-command 'python3 /evidence/clock-observer.py' \
  --ready-command 'python3 /evidence/check-ready.py' \
  capture /evidence/paired -- python3 /evidence/transition.py
```

The readiness command observes the settled source page. The clock command emits
JSON lines with monotonic `before`/`after` read bounds and `game_seconds`.
The transition command uses the recorder's epoch and emits the ordinary `sent`,
`acknowledged`, `from` and `to` event. It owns its bounded audio tail. The clock
observer is terminated when that driver finishes, so a longer observer timeout
cannot add unrelated repeated cues to the transition window.

Calibration temporarily mutes exactly one named game producer while the known
signal plays. The original unmuted player settings are restored and compared
before public input, including after calibration failure. Other producers are
untouched. This prevents a quiet producer's constant sample offset from corrupting
strict unity; it does not change the transition's production volume. Recording
ownership is protected by a per-output lock.

Run `check-paired-book-audio.py RECORDING REPORT --reference EFFECT.wav` for each
installed cue. Its phase windows come from recorded events, with the existing
recorder pacing allowance retained as PCM/wall epoch uncertainty. It requires
strict stereo unity, unchanged player settings, cue continuity, and observed
clock samples bracketing that uncertainty. Shared epoch error is retained when
bounding cue cadence; host time is never interpolated into game time.

For different recording sample rates, play the retained source recording through
the target sink with `paced-monitor-check.py --reference SOURCE.wav`. Then run
`book-audio-transfer.py CAPTURE SOURCE.wav REPORT --reference EFFECT.wav` for each
cue. It requires admitted explicit-unity playback of that source hash and cue
continuity at both boundaries, and reports the measured transfer ratio per cue.
It does not assume a generic resampler response or authorize changing production
gain to compensate for a frequency-dependent recording path.


`dev/scripts/world-pursuit.py --port 7117 DRINT EXTPALAC` exercises combat actors' lost-sight routing:
it places the selected player (`place_player`) next to the nearest living combat actor, then holds it
far away, and prints failed actors, a world-level script error and how many actors reached
lost-sight timing and routed pursuit (exit 1 on a failure). The `teleport` command moves only the
render transform, so it cannot be used for this. Run one world per process.

`dev/scripts/native-audit.py --scripts game-config/cache` lists the natives that the exported
installed scripts call and that no Rust source mentions (exit 1 if any): a forgotten handler shows
up there, though a mention does not prove the handler works.

`--matrix` prints structural source pointers. `--behavior-matrix` prints the reviewed behavior
scope, owning issue, public caller, inspected test assertions, evidence references and remaining
limits for each called native. The reviewed snapshot lives in
`dev/scripts/native-behavior.json`, with its source revision and review method. Unreviewed entries
are `unaudited`, not `missing`. `partial` means some behavior exists but the stated scope still
has implementation or acceptance gaps; `missing` requires an explicit review finding. `done`
requires recorded original/native/browser acceptance for that scope. References are reviewed
evidence, not automatically verified by the tool. A test name or source mention never promotes
a status. Use `--behavior PATH` for a separately reviewed catalogue. Check the reporting contract
without Cargo with `python3 dev/scripts/test-native-audit.py`.

In the browser build, `?capture=1` publishes `window.__cyrusCapture`; setting
`window.__cyrusInspectScripts = true` adds the first 64 script actors (the `scripts` command's
data). `dev/scripts/start-duel-web.js` evaluated in the page plays the same duel with DOM key
events (a software-rendered tab runs at about 2 frames per second, so its key holds are long).

`dev/scripts/original-duel.py` plays the same duel in the original under DOSBox-X with the same bot
policy (xdotool keys; positions, yaw and health read from the emulator's memory by a reader kept
under `magic/tools/`, because it holds executable addresses). The
container is started with `--cap-add SYS_PTRACE`, and the run copy's `maps` link is replaced by a
real directory: the game saves `MAPS\\<world>.TSG` when it leaves a world and hangs if the
emulator cannot create that file. `dev/scripts/compare-duel.py ORIGINAL.log NATIVE.log` compares
the two logs: pirate health falls in whole 10-point hits, both pirates are defeated, the story
reaches the next world, the player arrives at the same place and heading, and the delay after the
last kill agrees within a factor of two. **Verified** (runs PASS): both place the player at
(38112, -278, 36352) facing 256 in world 1 (a later run's arrival sample, taken while the player was
already turning, differed by 127 units and 45 heading units). Pacing is compared in each engine's
own game seconds (the original: frames over its measured frame rate; cyrus: `state.clock`), and
that is not frame-rate independent: the guards' and pirates' attack cadence is counted per update
in both engines. With cyrus at 3 to 7 frames per second here and the original at 8 to 13, a
catacomb guard (`original-idle.py --world 2`) lands a 10-point hit every 0.65 to 0.94 game seconds
in the original and every 1.0 to 1.7 in cyrus, but every 12 engine frames in the original against
10 in cyrus, so the two agree per update. The story after the duel took 41 to 45 game seconds in
the original and 28 to 36 in cyrus; its cause is not found. Animation groups seen at the samples
overlap (death groups 122 and 124, attack groups 102 to 104 in both); frame-synchronised replay,
camera and audio are not compared.

`dev/scripts/original-idle.py --world N` starts the original in world N with no input and logs the
player's health and the nearby combat actors; the same world run natively (`load_scene`) shows
whether the idle player dies in both. In the catacomb (world 2) both die to the same two guards.

For a bounded original rope contact probe, use a prepared writable run directory
with its shared assets linked as described above:

```sh
docker build -f dev/docker/retro/Dockerfile.contact \
    -t cyrus-retro-contact:dev dev/docker/retro
bash dev/scripts/original-rope-contact.sh RUN READER.py PLACE.py OUTPUT
```

The launcher holds the host live-run lock for one run, mounts the host config
and local diagnostics, restores `WORLD.INI` and `SYSTEM.INI` afterwards and stops its own
container on success or failure. It rejects the canonical install as a writable
run. It uses `cyrus-retro-contact:dev`, which runs the same guest configuration on
Xvfb without the interactive preview services. Build it with the command above;
`CYRUS_RETRO_IMAGE` can select an existing compatible image. The launcher passes
`-set "dosbox hostkey=ctrlalt"` to set DOSBox-X’s host modifier to Ctrl+Alt, leaving
F12 for the guest console.
The contact image includes the full Python standard library. Its build checks
that `ctypes` can resolve the process-tracing entry point used by atomic boundary
readers; an image with only `python3-minimal` cannot run those readers.
Original audio capture also configures this modifier explicitly, including when
reusing an older runtime image. Use Ctrl+Alt+W for the emulator recorder; the
recorder chord must not open the guest console. A local config override must place
`hostkey=ctrlalt` in `[dosbox]`, not `[sdl]`.

The reader and placement adapter are local diagnostics kept outside tracked
files. The reader follows the `original-state.py` JSON contract; the placement
adapter runs as `python3 /tmp/place.py`, with that reader at `/tmp/state.py`.
Neither adapter may modify installed assets. The launcher reads installed text-suppression settings from
`SYSTEM.INI` and applies them in the writable run to avoid inherited developer
diagnostics while preserving direct-world admission and other run settings;
`CYRUS_RETRO_SYSTEM_INI` can select an explicit baseline file.

Executable-specific readers can use `dev/scripts/original-memory-region.py` to
locate their guest region from a local signature and its offset. Supply readable
anonymous process mapping ranges and the required region size. The helper accounts
for a region beginning inside a coalesced mapping and rejects absent, ambiguous
or truncated-region candidates. Keep signatures and executable bindings in the
local reader. Run its controls with `python3 dev/scripts/test-original-memory-region.py`.

The reader must report top-level `console_mode` as `closed`, `overlay` or
`interactive`, derived from active console state and its modal/nonmodal mode.
The public driver requires `interactive` as well as a paused world clock;
a paused overlay or missing mode fails before any gameplay text. It focuses DOSBox,
checks that console entry paused world updates before sending commands, grants
and draws the sword through console/input, then invokes placement. Escape closes
each command entry. Underscores in object and attribute names use a held Shift
with the minus key through the polling interval; they are not literal xdotool
key names. Shift is released even when character entry fails. The JSONL record
includes selected-player state and world clock through walking, attack and continuation. A sheathed, falling or
nonordinary living state fails before movement. Held inputs are released on
failure, and stalled world progress has a wall-clock cutoff. Capture mixer audio
separately to assess clank: a successful exit alone does not prove contact,
attachment or sound equivalence. Match placement, heading and observed game-time
input duration in the native comparison. Run the driver and launcher controls
with `python3 dev/scripts/test-original-rope-contact.py`.

`dev/scripts/new-game-compare.py` loads START, waits for the idle player to die, chooses NEW in the
death book (INTRO.SMK plays first and is skipped with Enter; the install's parent must hold the CD
image) and compares the script globals, HUD and inventory with the first start. Measured: inventory,
compass, power-ups and flags are identical; only timing-driven values differ (frame counters, health
mid-fight, the retained death position, the script choice highlight).

Headless mode creates no camera or audio backend. Control state reports a null
camera position; player input, preload, world export and protocol commands remain
available. Use a windowed run for camera and screenshot checks.

Both native executables can be opened without arguments, including by double-clicking
on Windows. If no usable install is found, the same window shows the Redguard
folder picker. The viewer does not require an asset selection; the game waits for
the folder before preparing installed audio and images.

Settings, preferences, the remembered game-files folder, saves, exported caches and logs
live outside the install, in the directories `crates/cyrus-config` selects once per process:
`CYRUS_CONFIG_DIR` when set, else a `game-config/` directory that already exists in the
working directory, else the platform directories (on Linux `~/.config/cyrus`, `~/.cache/cyrus`
and `~/.local/state/cyrus/logs`). This document calls them the config, cache and log
directory. In a checkout they are `game-config/`, `game-config/cache/` and
`game-config/logs/`: `cargo run` and `cargo test` pin `CYRUS_CONFIG_DIR` to the checkout's
`game-config/` (`.cargo/config.toml`), and `just run-native` pins it inside the container.
A binary started by hand from a directory without `game-config/` uses the platform
directories. Screenshots are written wherever the `screenshot` command's path says.

`CYRUS_WINDOW_SIZE=WIDTHxHEIGHT` replaces the default window size. In the software-rendered
dev container the renderer, not the game logic, limits the frame rate: an optimized build
(`CARGO_PROFILE_DEV_OPT_LEVEL=2`) at 640x480 runs above 16 frames per second, where the
default size runs at about 5 to 11. Measure the frame rate (control `state`, field `frame`)
before comparing timing-dependent behaviour such as combat pacing with the original.

A slow native run in the container is slow, not hung. The unoptimized build under software
rendering draws about 1 to 2 frames per second, and the game clock advances only about 0.2 to 0.4
game seconds per wall second (older builds ran about 0.015, which looks like a frozen clock). Poll
the control `state` clock to see progress, normalise every measurement by that game clock, and
abort a run only when the game clock itself has not advanced for several minutes. A clock that
stops while the state's `frame` counter keeps rising is different: the script world has stopped
after an error, and `state.scripts.error` names it.

The window waits for the display's refresh by default, so a measured frame rate is capped at it
(and drops to half of it when a frame just misses). `CYRUS_PRESENT_MODE=novsync` presents at
once; use it, on a machine with no other builds running, when measuring what the game can
render. `vsync` is the default.

Unoptimized native loader threads need a larger stack. Launch such binaries
with `RUST_MIN_STACK=8388608`; container launches also need
`-e RUST_MIN_STACK=8388608`. The test runner sets this automatically; native
development launchers do not yet supply it by default.

### Player placement

Direct scene inspection uses available marker zero for initial player placement.
Native, headless, and browser loading consume the same marker metadata. Re-export
worlds after updating to populate exact marker coordinates in `.animations`
and installed movement/terrain inputs in `.player`. Re-export for player schema 2,
which adds authored static collision geometry and placement lists.
Entry waits for both sidecars and settles against dry terrain and static models.
Unavailable markers, unsupported terrain, and missing player definitions report a
player-entry error. Bounded sheathed forward walking uses the ordinary player
update. Complete installed traversal, dynamic ground support and the original
follow camera remain unverified; use the fly camera mod (`) for inspection.
The game camera uses the original Glide horizontal field of view, including on
viewport resize. Its inspection position is independent of the original follow
camera; matching the projection alone does not establish a matched view.
For repeatable comparisons, `--scene NAME --view=x,y,z,qx,qy,qz,qw` sets the
initial inspection pose in exported scene coordinates with a unit quaternion.
The browser accepts the same `scene` and `view` URL parameters. The pose applies
once, does not move the player, and does not persist into later scene loads.
Omit `--scene` or the `scene` URL parameter to enter through the book menu;
the supplied pose waits for the first world, including New Game.

## Asset viewer

`just viewer --install /path/to/Redguard` launches the separate native asset
viewer. It accepts `--install`, `REDGUARD_DIR`, or the same default-folder discovery as the game.
Build with `just viewer-web-build`, without an install. Run
`just run-viewer-web --install /path/to/Redguard` to serve original files at runtime.
The browser parses them in a worker and generates selected previews in memory.

The game and viewer share `target/web`, the loader, Cache Storage, and the
selected read-only install service. Both prepare assets in browser memory.
Digest-addressed original-file ranges reuse Cache Storage when the explicit range agrees with the HTTP Range header. Other
Range requests bypass this cache. Run `bun dev/scripts/test-original-range-cache.mjs`
to check reuse, source/range isolation and response validation.
Set `CARGO_TARGET_DIR` to build the wasm outside the checkout (for example on a
volume that is not cleaned); the web root stays in `target/web`.
See [cyrus-viewer](../crates/cyrus-viewer/README.md) for controls and limits.

## Releases

The `Release` workflow packages the native game for Linux x64,
Windows x64, macOS Intel, and macOS Apple Silicon, plus a browser bundle.
It also includes `cyrus-viewer` and its browser worker when the viewer crates
are present in the checked-out revision. Both applications are built from this workspace.
Browser artifact builds compile all available applications and the viewer worker
in one Cargo invocation so they share dependency features and compiled libraries.

After the release is published, `Release` deploys GitHub Pages: the landing page
from `dev/pages/index.html` at the root, the game under `game/`, and the viewer
under `viewer/`. The landing page links directly to the latest Windows x64,
Linux x64, macOS Apple Silicon and macOS Intel game packages.
Both pages share the engine files and service worker at the root. The repository's Pages source must be set to "GitHub Actions" once.
The site contains application code only. Choose a local GOG game folder in the
browser, or supply a service with `?install=<encoded-install-service-URL>`.

Pushes and pull requests run tests, lint, and browser compatibility checks.
They do not build release archives. Release builds run only for stable `vX.Y.Z`
tags, either when pushed or dispatched from the default branch with a `tag` input
such as `v0.2.0`. A tag push dispatches the build on the default branch; every
build still checks out the specified tag. This saves compiled dependencies in
the default-branch cache scope so later releases can restore them. The tag-push
run only dispatches; follow the separate dispatched `Release` run for build and
publication results. The first run for a platform or changed toolchain can
still need a full build.

Run `Bump Version & Tag` on `main` with `auto`, `patch`, `minor`, or `major`.
It updates the shared workspace version and lockfile, commits, and atomically
pushes the commit and tag using the configured releaser App. The tag push
triggers `Release`. To retry an existing tag, dispatch `Release` on the default
branch with that tag as its input.

`Release` checks that every workspace package matches the tag and runs lint and
test gates in parallel while all platforms build. Both verification jobs reuse
the matching CI caches. Nothing is published unless every job succeeds.
Verification and native Linux builds remove preinstalled SDKs only when less than
12 GiB is free; browser builds still free space for their additional swap. The macOS
Intel build uses the Intel runner pool, independently of Apple Silicon capacity.
Browser release builds use four Cargo
jobs, extra swap, and the `wasm-release` profile (thin link-time optimization,
16 codegen units) to stay within hosted-runner memory; build logs include
resource usage. After every build succeeds, it generates notes from conventional
commits with git-cliff and `cliff.toml`, attaches all packages and `SHA256SUMS`,
and publishes the release. No release is created when a build fails; successful
packages remain available as Actions artifacts. Rerun failed jobs in Actions to
finish. Reruns leave an already published release unchanged.
Actions artifacts expire after seven days; GitHub release assets remain available.

Linux and macOS packages are `.tar.gz` archives holding only the executable;
Windows packages are the bare `.exe`. The browser bundle is an archive. Packages contain
no game files, exported previews, or local state. Native executables still need
the player's GOG install. Browser bundles accept a local folder or an
install-service URL. Local files remain on the player's computer; URL sources
are remembered and validated on each launch. A WASM worker prepares game and
viewer assets in memory. Downloading an artifact does not establish runtime or fidelity
support on that platform.

## Browser build

```sh
rustup target add wasm32-unknown-unknown && cargo install wasm-bindgen-cli   # version from Cargo.lock
just run-web --install /path/to/Redguard # supplies files automatically to game and viewer
```

Before compiling, the browser build checks the bindgen CLI against the exact
`wasm-bindgen` version in `Cargo.lock`. A matching CLI on PATH wins. If PATH
contains an older CLI after a restart, the build can reuse an already retained
`magic/build-tools/wasm-bindgen-VERSION/wasm-bindgen` executable after checking
its reported version. If neither matches, it stops before Cargo and requests
`just toolchain`. It does not change dependencies or replace the active CLI.
Run `python3 dev/scripts/test-resolve-wasm-bindgen.py` for selection and
wrong-schema failure controls.

Local browser builds use the `wasm-dev` profile, which inherits `wasm-release`.
Both use 16 codegen units and thin LTO. Releases trade a larger download for
shorter compilation compared with the former single-unit profile.
Set `CYRUS_WASM_PROFILE=wasm-release` to build with the release profile name.

For browser CPU profiling, set `CARGO_PROFILE_WASM_DEV_STRIP=none` and
`CYRUS_WEB_KEEP_DEBUG=1` before `dev/scripts/build-web.sh`. This retains Rust
function names through wasm-bindgen. An optional `CYRUS_WEB_CHECKPOINT_DIR`
on a persistent host mount saves each completed raw WASM package before
packaging. Rerunning the same command restores matching completed packages
without Cargo, including after target-directory loss or a packaging failure.
Source, compiler, profile and Rust flags must match; corrupt checkpoints fail
closed. During application compilation, completed Cargo units, dep-info,
fingerprints and build-script outputs are retained in compressed, hashed units.
After target-directory loss, matching units restore before Cargo resumes;
Cargo still checks dependency freshness and finishes interrupted units. Existing
shared artifacts are never overwritten. Restoration streams each compressed unit
once and validates each recorded member before publishing any file. Temporary staging
stays inside the writable target directory, including when its parent is managed
and unwritable. Build orchestration changes do not invalidate unchanged compiler
inputs. No whole profile is copied or removed.
The checkpoint directory contains build outputs only, never installed game data,
and does not change the inherited Cargo target directory.
Run `python3 dev/scripts/test-web-compile-checkpoint.py` for interrupted-build,
corruption, changed-input and shared-artifact preservation controls.

Without a `scene` query, the page runs startup through the book menu. Native and
browser hosts compose this presentation through `cyrus-startup`; scene graphics
and listener poses remain in `cyrus-render`, and playback remains in `cyrus-audio`.
Native `--scene`, `--skip-startup`, `--new-game`, `--movie`, `--skip-movies`, `--volume`, `--fog`, `--sky`, `--music`, `--controls`, `--camera` and `--mode` map directly
to the same URL parameter names. Viewer `--asset`, `--file`, and `--palette` map
to URL parameters or existing fragment links. `--install PATH` maps to
`?install=URL` for the browser, where URL serves `manifest.json` and original bytes.
The WASM worker prepares game assets from that manifest and original bytes.
Original reads use bounded requests, a transport retry and contextual errors.
See the crate READMEs for the complete mappings.

Both native applications discover `./redguard`, `./Redguard`,
`~/GOG Games/Redguard`, `~/GOG Games/Redguard/app/Redguard`, and on Windows
`%SystemDrive%/GOG Games/Redguard`. Explicit `--install` and `REDGUARD_DIR` take
precedence over the remembered native location in `install.json` in the config directory;
conventional discovery follows when there is no saved location. Windowed game
and viewer launches open a shared install-selection screen when resolution fails.
Enter a path or choose Browse, then Continue to validate and remember the resolved
root. The picker, loading progress and game or viewer share one window and process.
Initial game images are prepared in the background so the window remains responsive.
Cancel closes the application without changing preferences. A missing or
invalid remembered folder reopens the picker. Headless, world-listing and export
commands report contextual command-line errors instead of opening a window.
Linux Browse uses a desktop file portal or Zenity; manual input needs neither.

Native game and viewer share path resolution: `--install` and `REDGUARD_DIR` also accept a GOG
library folder, the outer Redguard package folder, or any directory inside the
install. Resolution checks enclosing directories and known `Redguard`,
`Redguard/Redguard`, and `app/Redguard` layouts for `fxart`, `maps`, and
`RGFX.EXE`, without recursively scanning the library. A sibling `DOSBOX`
selection resolves through the enclosing package. Missing paths and unresolved
selections remain on the picker with validation details; noninteractive commands
report an error with `--install PATH` instructions.

The browser always opens the install picker. Choose the main GOG folder containing
`Redguard`, `DOSBOX`, `game.gog` and `game.ins`. All browsers use a directory
file input. Files are read locally in the worker, never uploaded or copied into
browser storage. The initial content check reads bounded chunks and reports
progress. Select the folder again after reload or navigation. Cancel leaves the
picker open; validation failures allow another selection. Companion files are
discovered throughout the selected folder, including nested GOG support folders.
Files outside the selected folder are inaccessible.

The expandable server option remembers validated URLs in localStorage
`cyrus-install`, shared with the viewer. URL `install`, the server default and
the saved location prefill that field in order, without starting a connection.
`?install=local` leaves it empty. Click Connect to server to validate and load
the source. Local selection stores only the preference `local`, not file access.

For unattended tests, supply `--install /absolute/path/to/Redguard` to the server
or `?install=<encoded-service-URL>`, then automate Connect to server. Do not ask
the user to fill in the picker for an agent's test.

`viewer-server.py` serves both applications. Its `--install` flag provides the
read-only original files and shared browser default. The WASM worker prepares
startup and requested worlds in memory through the same byte-source exporters
used natively. Cinematics read bounded compressed ranges and decode first-track
PCM during playback. The server binds to loopback; remote or static original-file
hosts need a manifest, CORS and HTTP byte-range support. No native executable or
prepared cache is required by the browser.

For manual export, prepare startup with `cargo run -p cyrus -- --export-startup`. The exporter
reads the installed `3DfxSpl2.dll`, `system/STARTUP2.GXA`, and the configured
`gui_low_font` from the install; all generated images stay in
`startup/` in the cache directory. Native windowed launch generates this cache when
needed. The splash DLL is selected from `Redguard`, with the sibling GOG `DOSBOX`
directory as a fallback. **Verified:** its metallic logo and orange swoosh are
the Windows Glide splash GOG players see; cyrus plays only that one ([DIFFERENCES.md](DIFFERENCES.md)).
The [preservation contribution](https://github.com/michidk/redguard-preservation/blob/main/docs/formats/GLIDE-SPLASH-DLL.md)
documents the DLL format. Enter, Space, or Escape advances startup.
Default startup selects its entry from installed final-version and
ignore-autosave flags and a validated host autosave, native storage or browser IndexedDB. A final-version launch
without an autosave plays Introduction before the starting world. A valid
autosave opens the book. Non-final or ignore-autosave configurations enter the
starting world directly. This follows [preservation startup entry](https://github.com/michidk/redguard-preservation/pull/314).
`--skip-startup` explicitly opens the book while required menu preparation remains behind the title. `--new-game`,
`--movie INDEX` and `--load-slot SLOT` (native) skip the splash and drive the book once it is open, as the
entry's key press would; `--skip-movies` makes New Game, from the book too, enter its world without the
introduction movie. `--scene` loads a world directly and excludes all three. MCP `skip_startup` goes to the menu; `load_scene {name}` enters
a world from startup or the menu. Control `state` reports the current app state.

For original startup captures, set `CYRUS_CAPTURE_STARTUP=1` when running
`dev/scripts/original-audio-capture.sh <events.json> <out-dir> <seconds>`.
Keep the existing `CYRUS_RETRO_RUN`, `CYRUS_RETRO_GOG` and
`CYRUS_STATE_READER` inputs and set `CYRUS_SHOT_INTERVAL` to capture images.
This mode waits at the DOS prompt, arms the audio recorder, takes a baseline
image, then launches the installed executable. The timeline records that
launch boundary and stops at the first positive world-loop frame or the
requested duration. The default mode still waits for an already running world.
Use an unmodified final-version run and inspect its autosave state when comparing
startup paths; a development-mode run can select a different entry path.
Run `python3 dev/scripts/test-original-startup-capture.py` for the recording
order and game-clock cutoff regression without game data.

The book menu reads `MENU.INI` through rgpre, using original geometry, page
textures, indexed FNT glyphs, and GXA widgets. Up/Down navigates eligible
entries; Enter or the configured B key activates them. Options opens Display,
Sound, and Controls. Left/Right adjusts toggles and bounded volume sliders;
Enter toggles checkboxes or starts key capture. Escape cancels capture or
returns to the parent page. Controls uses the installed key names and capture
prompt, rejects reserved keys, and clears the first conflicting binding.
The existing player-input actions use the edited bindings, including direct
`--scene` launches. Extra gameplay actions remain pending their handlers.

Changes persist to `menu.json` in the config directory on native builds and localStorage
under `cyrus-menu` in browsers. Installed defaults apply when no local state
exists. Malformed state disables edits without overwriting it; failed saves
leave the previous runtime values intact. The install stays read-only.
Saved Sound preferences apply to effects and CD music on native and browser
builds, and to native speech. The book menu plays its installed CD track. Cinematics consume subtitle and interlace preferences.

Movies browses the initially unlocked entries and installed previews. Native
playback reads the configured movie file or GOG `game.gog` beside the install
through a bounded sequential reader; the native virtual-display runner mounts the CD image read-only. It decodes video and the first embedded audio
stream, uses the installed subtitle definitions and the REDSEL font with its shadow ([preservation#110](https://github.com/michidk/redguard-preservation/pull/110)), and applies the
saved subtitle/interlace preferences. A key press advances to the next installed
skip point, or finishes after the last one. Native playback decodes toward every skip point in the background, so a skip does not wait for the frames before it. Playback returns to the same menu
selection. Native playback does not write movie data to `cache/`. Browser movie caches exported before the REDSEL subtitle font need re-export.
Browser playback reads the selected cinematic's compressed original ranges on
demand. The shared codec decodes indexed frames and first-track PCM together;
Web Audio plays bounded PCM blocks using its sample clock. Playback starts with
the first frame and audio data, without complete media downloads. The same skip,
subtitle, interlace and menu-return rules apply. Forward skips decode preceding
delta frames. Closing playback cancels further reads and releases queued audio.
Original-file hosts must support byte ranges. The worker also prepares CD WAV
tracks and installed world assignments for music. A key press or pointer release
activates browser audio. Runtime exceptions and graphics context loss produce a
visible error above the game.

MCP `state.menu.movie` reports loading, filename, frame, and total frames. The
installed `force_movies` setting exposes all cinematic entries. Playing a
movie unlocks its entry, saves keep the unlock flags and New Game clears them
(preservation `docs/engine/movies.md`). A gameplay script's `QueueMovie` 5 through 10
pauses the world behind the in-game book context, plays that cinematic and resumes
the world (**Verified** natively through the control `queue_movie` command: A_RUINS1.SMK
played to its last frame, then gameplay resumed). A queued movie plays between worlds
the same way, before the next world loads (**Verified** natively: `queue_movie 1` then
`script_load_world 13 0 0` in CAVERNS played SCENE3.SMK first, and MGUILD loaded and became
ready after it ended). Native Quit closes the book and exits. Browser Quit
attempts to close the tab; when the browser refuses, the menu remains usable.
New Game enters the `WORLD.INI` starting world and marker. Load opens the installed page with paginated saved slots and restores the
selected revision through the host session boundary. Save opens the authored
description editor for new and existing slots. Save and Continue remain grayed
without a resumable world. World entry shows the installed
destination artwork and red percentage bar until required assets and dependencies
are ready. **Verified:** original world loading has a gold lower-left percentage
and no right-hand status message. Native and browser presentation use that layout.
The startup title updates its bottom-right red label during menu/HUD preparation,
menu loading and browser sound loading. Title labels describe cyrus loading stages.
Escape returns to the menu after a loading error. Full-game progression and
new-game-to-ending acceptance remain incomplete. Menu export writes `cache/menu/book.menu` and an rgpre-generated
status font, plus the installed effects bank under `cache/sounds/` for browser menu
playback without loading a world. Runtime redraws authored book frames and neighboring-page text during
startup camera entry, submenu turns, and the closing Quit sequence. Navigation waits for
motion to settle; MCP drivers must wait for `state.menu.ready` after each page
change. Timing uses the startup presentation cadence rather than original update
performance. Book/page
motion and navigation feedback play installed effects through the shared sound
path on native and browser builds. **Verified:** the original default book menu has no pointer
hover/click selection. Gamepad movement and buttons follow the installed joystick bindings (see `docs/DIFFERENCES.md`); remaining mouse-bound actions are incomplete. Lighting,
camera-plane clipping remain unsupported. Entering the book from gameplay, or by a fatal death, captures a 256x192 centre-cropped
view that the book shows and slot saves store; a save made from live gameplay (quick save) stores no thumbnail.

With `?scene=...`, the page requests that world's GLB and sidecars from the
selected install service. The service prepares missing worlds through the native
exporter. Map stems and WORLD.INI IDs retain native selection semantics.
The browser bootstrap downloads only the WebAssembly engine after validating the
install URL. Startup/menu and world downloads occur inside the running engine
behind installed artwork. Cache Storage retains the engine and selected-host
assets, revalidating mutable game exports. Original downloads retain digest-based
caching. Browser builds contain application code only and do not link local game
files into their web root.

### Animation data

Re-export worlds to generate `.animations` and `.environment` sidecars beside
each GLB. Native and browser scene loading retain animation groups, model
references, frame geometry, original animation section payloads, and environment
inputs. Re-export for animation schema 2 and source-mapped GLB geometry. Explicit
group playback accepts supplied simulation ticks and updates private model
meshes. Re-export again to retain script payloads and version-2 typed SOUP
program descriptors plus the independent version-4 `.scripts` sidecar with actor RASC origins, installed actor-pool capacity and initialized attribute defaults. Explicit scripted playback accepts a caller-selected default,
executes authored PushAnimation tasks, and applies their mesh frames. End stores
its target for a later eligible stage through the shared `cyrus-soup` core.
Control `state.scripts` reports cache readiness, program and unsupported counts,
and bounded unsupported examples. Loaded scripts do not imply live execution in
the inspection host. Production world activation and host scheduling run through
the native game path; the inspection host does not infer those stages, and
interrupts, synchronization channels and other script operations remain unsupported there. Missing animation models
are listed in the `.animations` asset's `unsupported` map.

### Model lighting

World and player-model exports must include `rgpre_lighting` primitive metadata,
and environment exports must include `model_lighting`. Native world entry regenerates
these files; regenerate older browser exports.
**Verified:** models use the installed ambient/directional RGB, authored face
brightness and solid-color tint through the shared vertex shader. Invalid mesh
metadata is reported with world/mesh context and the unsupported mesh is hidden.
Local lights and complete static-prelighting behavior remain unsupported.

### Sky, fog, and water

Re-export worlds to regenerate `.environment` sidecars. F11 provides independent
Faithful/Modern buttons for Fog and Sky, which default to Faithful. Water changes only through the
`modern-water` native mod.

**Verified:** Faithful sky uses the installed BSI/COL texture, camera-relative
plane mapping, per-world scrolling speed, and BIOS timer documented by
preservation. The renderer also applies the fixed 24/2048-turn tilt and the
installed sun's camera-facing opacity texture. Full original-game comparison
of those additions is pending. The initial random scroll direction is not
synchronized automatically; MCP `set_sky_scroll` accepts a captured direction
and U/V offsets for comparisons. Modern sky uses
the same scrolling texture on a camera-centered background: translation cannot
cross its surface. Both retain the world's colors and horizon haze.

**Verified:** Faithful fog uses the original tables and installed clipping
range (7/3800 engine units in the shipped configuration). Modern fog defaults to
four times that far distance and fades smoothly over the distant portion of
the view. F11's distance control selects Modern Fog; Original resets the distance
and returns Fog to Faithful. Sky is unaffected.

**Verified:** Faithful water follows preservation's vertex selection, waveform,
bias, spatial phase, and BIOS-rate clock. The `modern-water` mod keeps the same selected
vertices and installed texture, with crossing waves, ripples, shore foam and wet sand. Water
reflections belong to Modern Lighting in the base game and need no mod. These are optional artistic effects, not recovered engine behavior. Turning
the mod off restores the original wave at the current clock tick. Terrain placement, base heights, and lighting
retain the existing export limitations.

## The original game

```sh
just retro               # DOSBox-X + Voodoo emulation in dev/docker/retro, VibePod preview "cyrus-retro"
```

The retro runner needs a writable copy of the GOG `app/` directory (the game
writes saves); `dev/scripts/run-retro.sh` makes one under `magic/retro-run/` and
copies DOSBox-X's `glide2x_emu.ovl` next to `RGFX.EXE` so the 3dfx build
renders through the emulated Voodoo. Sound is off in the container.
For independent original audio captures, `original-audio-capture.sh` repairs missing
`soup386` and `system` files in the writable run copy before launch, using
`prepare-original-definitions.py INSTALLED_REDGUARD RUN_REDGUARD`. Existing
per-run files are preserved. Linked definition destinations are rejected so
recovery cannot write into the install.

`python3 dev/scripts/animation-transition.py ADAPTER KEYS.INI OUTPUT --group GROUP
--effect EFFECT` drives a selected-player quick-sword transition through an
executable observation/capture adapter. Group and effect come from the selected
actor's installed animation definition; the key binding comes from the supplied
`KEYS.INI`. The adapter protocol is documented in the script. It must arm event
and PCM observation before input. The driver waits for ordinary progress,
releases input on observed group entry to prevent held-key retriggering, and
stops at observed group completion or bounded game progress. A wall deadline
limits a stalled observer. Changed player/world identity, missing emission/stop
ordering or incomplete PCM reject the receipt. A sample that finishes before
EndAnimation may instead terminate with an observed mixer-completion query;
clearing its remembered handle alone is insufficient. Starts, remembered
ownership and terminal events match both slot and start generation, and events
retain their original immutable snapshots. The driver drains the completed
transition through the next ordinary frame before finalizing PCM. Every observation is saved
atomically; only `status=captured` admits the complete contract. A group change
without sound-service observations remains rejected. This requires an adapter
that observes the real sound service; group/frame interpolation is insufficient.
Run its failure/continuation controls with
`python3 dev/scripts/test-animation-transition.py`.

`python3 dev/scripts/original-sound-observer.py BINDINGS OUTPUT` records actual
sound-service writes with Linux x86-64 hardware watchpoints. The original test
container needs ptrace permission. Executable-specific bindings remain in ignored
research, verify the loaded executable and relocated instructions, and supply
`pid()`, `bind(pid)` and `capture(hit_indices)` as documented in the runner.
Each capture runs while the emulation thread is stopped. Service events include
the selected animation, frame/game clock, reusable sample slot and mixer start
generation. Missing event batches, sequence gaps, invalid generation identities
and overflow reject the stream. `status.json` records the final result separately
from flushed `events.jsonl`; an armed or interrupted stream is not acceptance.
Use a fresh output directory, request shutdown with its `stop` file, and retain
both files with executable and binding hashes. The runner clears its hardware
watches before detaching, including on failure. Bindings must observe real service
boundaries; they cannot turn group changes or PCM energy into starts or stops.
Run `python3 dev/scripts/test-original-sound-observer.py` for failure controls.
Set `CYRUS_OBSERVER_HARDWARE=1` inside the original test container to also execute
the real stopped-write and detach control.
The disc is mounted from `game.gog` through a `game.cue` copy of `game.ins`: stock DOSBox-X
reads a cue sheet only by its `.cue` extension, and without the disc every movie is skipped.
The default copy keeps `final_version=0`, which makes the original skip the Introduction and start in the first
world; `final_version=1` plays it, with or without the debug-text flags (**Verified**, Linux, 2026-10-18).
`RETRO_PRISTINE=1 just retro` uses the install's own `SYSTEM.INI` and an empty `SAVEGAME`
(under `magic/retro-pristine/`), so the original starts as a player sees it: title, Introduction,
then the book menu, where Movies plays the cinematics. From the first world, Escape
did not open the menu; the book menu appeared once the opening duel played out. Preservation
`engine/menu.md` says Escape opens the book with `final_version=1` (the install's own value), which
that observation did not show (**Unknown**: it may have been the scripted duel). cyrus follows
preservation: Escape in a world opens the book after its release and gives Save, Continue and Quit
the paused world (**Verified** in the native build: the book opened from the first world and its
Save page showed the 256x192 gameplay thumbnail; `--scene` launches have no book, so use
`--new-game --skip-movies` or the menu). F2 and F3 open the Save and Load pages (same `final_version` gate; their entry camera, from the settled main page, is **Inferred**). F4, F5 and F6 open Display, Sound and Controls on the page's settled pose from preservation `engine/menu.md` (**Verified** as the documented end state; their own transition is not documented and none is played). **Verified** in the native build: each key opens its page on that pose and Escape returns through Options to the main page without leaving a depth shift behind. Quit from a world prepares the `Redguard AutoSave` slot when a gameplay snapshot exists, or removes it while the death override is active (preservation `engine/menu.md`). **Verified** in the native build: F3 opens the Load page, a save made from the open book stores its thumbnail and the Load page previews it, loading it returns to the world, and Quit wrote slot one with a thumbnail before exiting. **Verified** in the native build: a fatal death opens the Last Game dialog when a save exists, choosing Yes restores the saved world with death override and hit inhibition cleared, and controls resume; without a save it opens the death book. The original and browser both reach the inhibited death-book outcome, while retained-save browser readback remains unavailable. **Verified** in the native build: after a pirate kill in START the death book opens, and Quit from it removes the `Redguard AutoSave` slot and exits; Quit from the world book with an unwritable autosave target shows the slot path and OS error, keeps the game running, and the retry after the fault is cleared writes the slot and exits. **Verified** in the native build: a save made in START after a console `load 1,0` and `load 0,0` round trip (ISLAND visited) survives a process restart, and the restored session holds START as current with 36 actors and ISLAND as visited with 976. A save refuses with a contextual error while a dialogue speech export is still pending. **Verified** in the browser build: a save from the paused book persists in IndexedDB, and after a page reload the Load page lists it with its thumbnail and loading restores the world. **Verified** in the browser build: loading a slot whose stored document is malformed shows the decoding error on the book, stays on the Load page and leaves the running world intact.

## Logs

The game and the viewer write to the console (stderr) and to a daily log file,
each with its own filter in `tracing` syntax. The file is
`<application>.<UTC date>.log` in the log directory (never in the
install), unbuffered and plain text, and the newest seven are kept. It records
the version, platform, command line and install path at startup, state changes,
the fidelity modes, world preparation with timings, control commands at
`debug`, errors, and panics with a backtrace. Attach it to a bug report; it
contains local paths but no game data.

| Option | Environment | Default | Meaning |
| --- | --- | --- | --- |
| `--log <FILTER>` | `RUST_LOG` | `info` | console filter, for example `warn` or `info,cyrus_script=debug` |
| `--log-file <FILTER>` | `CYRUS_LOG_FILE` | `info,cyrus=debug` | file filter; `off` disables the file |
| `--log-dir <DIR>` | `CYRUS_LOG_DIR` | the log directory | directory of the daily files |

Control polling and script steps are never logged, and no per-frame system logs
except once per failure. Expected, high-volume dependency messages (empty-mesh
warnings from the renderer, the importer's per-section progress) are filtered out;
set `RUST_LOG`, for example `RUST_LOG=rgpre=info`, to see them. The browser build
logs to the browser console only.

## Driving the game from an agent

The game listens on `127.0.0.1:7117` (`--control`, `CYRUS_CONTROL_ADDR`, or
`settings.toml`). `crates/cyrus-mcp` wraps it as MCP tools; Claude Code picks
it up from `.mcp.json`, Codex from `.codex/config.toml` (trusted project). Both
start `dev/scripts/mcp.sh`, which runs the built binary and builds it only when
missing, so a busy build queue cannot keep the tools down.

```sh
printf '{"id":1,"cmd":{"type":"state"}}\n' | nc 127.0.0.1 7117
```

## Shared build queue

`just setup` installs [cargo-hauler](https://github.com/ScriptedAlchemy/cargo-hauler)
(Node 22.19 or newer). Its hooks route the cargo commands Claude Code and Codex
run through one daemon, and the build scripts source `dev/scripts/cargo.sh` for
the same. Builds in one workspace queue instead of blocking on Cargo's lock,
identical requests share a run, and long builds continue after the agent's
command returns.

```sh
hauler status            # running, queued and recent tickets
hauler await cc-12       # wait for a ticket; hauler result cc-12 for its output
```

`cargo run` and the game binaries stay direct. Claude Code enables the plugin
for this project only. Codex has no project scope, so its hooks apply in every
project, and Codex runs them only after you trust them once when it asks.
Without cargo-hauler everything falls back to plain cargo.

## VibePod notes

There is no root in a VibePod. Bevy's system headers were unpacked into
`~/.local/sysdeps` and `~/.cargo/config.toml` points `PKG_CONFIG_PATH` there.
Previews: `cyrus-web` (new game in the browser), `cyrus-retro` (original).
A restart clears `~/.local`, which holds the global npm packages. Add a
command schedule set to `@startup` in the VibePod control panel that runs
`dev/scripts/setup-hauler.sh` in this checkout, so it is restored on every start.

### Player input and inspection camera

Press F11 and use **Flycam** to switch between inspection
and the player pose. Flycam starts disabled unless `--view` or `?view=` supplies
an inspection pose. In the book menu it moves the book scene's camera instead. WASD moves it, right mouse looks,
and Space/Shift moves up/down; its speed is adjustable in the same menu.
F11 world selections show the installed loading screen while the destination assets
Free flight is also available as the bundled `flycam` mod (see `mods/README.md`), toggled with Insert.
load, then recreate the player at marker zero and return to the gameplay camera.
Changing fidelity does not detach the view. Detaching preserves the player
position. Initial placement uses the installed player marker. The ordinary player
update supports bounded sheathed forward, backward and lateral movement; complete installed traversal
and the original follow camera remain unfinished.

Native player input reads `KEYS.INI` and its user overrides. The TCP/MCP `state`
response reports held `player_input` actions and the separate `flycam` position.
Direct browser scene launches load exported keyboard defaults and saved local
Controls overrides. Re-export worlds to include the keyboard defaults. The bounded
walking adapter requires walk mode and admitted ordinary sheathed state; running
and composite actions report unsupported. Headless inspection does not supply
the camera view required for automatic ordinary traversal.

### Explicit SOUP inspection

Native control and MCP `scripts` return paged logical actor state after
`state.scripts` reports `Loaded`. `step_script` takes the returned actor
`generation`, `index`, and stage `main`, `interrupt`, or `tasks`. Actor IDs
expire when a world is replaced, including a reload of the same cache handle.

**Verified:** the portable core supports expressions, control flow, actor return
state and shared task slots through explicit hosts. Installed programs and
initial instance locals are retained independently of rendered meshes. The
inspection host supports self `GetMyAttr`, `SetGlobalFlag`, `ResetGlobalFlag`
and `TestGlobalFlag`; unsupported services produce a
sticky contextual failure. These explicit inspection commands are separate from
ordinary gameplay scheduling and the production Save/Load pages; they do not
establish full-game progression or fidelity acceptance.

### Explicit shared script animation

Native control/MCP `scripts` returns logical actor IDs and world generations.
Use `bind_script_animation` with that generation, index, and an established
`default_group` to prepare actor animation. An optional exact scene `node` name
binds its visible pose and animation sound source. Without a node, preparation
supports logical inspection and returns sound intents in `captured_sounds` without playback.

`step_script` accepts separate `animation`, `native_hook`, `ordinary_main`, `entry`, `tasks`, `main`, and
`interrupt` stages. Supply `nominal_ticks` whenever any actor has animation bound, including
zero. The `tasks` and `ordinary_main` stages additionally need `frame_scale` with eight fractional
bits. These are explicit original timing inputs, not milliseconds or frame counts.
The shared host can combine animation tasks, waits, attributes, and flags.
These commands do not infer actor eligibility or start automatic gameplay.

For explicit SOUP spatial inspection, use `step_script` with `prepare_spatial`
once per world generation, then `families` to compose attached poses. Use
`snapshot` at an established actor-update boundary, `axis_locks` after script and
movement stages, and `transform` to refresh an unattached actor matrix. Executable stages then require both `frame_scale` and
`nominal_ticks`. Spatial state appears in `scripts`; bound scene nodes consume it
without advancing scripts. PlayerDistance uses runtime `neversaydie` state unless `distance_override` is supplied
and uses `fallback_position` only when no selected player pose exists. These
inspection inputs do not establish production scheduling or gameplay readiness.

### Cheats and console

Native TCP/MCP `cheat {name, value?}` toggles a named state or assigns a raw
unsigned integer. Use `oracle` to inspect values and support status and
`savecheats` to write `REDGUARD.CHT` in the config directory. Startup reads that local file
first, then the installed CHT file, then empty state. Malformed files remain
errors without being overwritten. The install stays read-only.

F12 opens the console in a world; F12 or Escape closes it. Gameplay also accepts
a cheat name followed by Enter. TCP/MCP `console {open?, line?}` uses the same
command dispatcher and reports retained output through `state.console`.
`commands` lists the original non-cheat commands and aliases, marking unavailable handlers; `oracle` lists cheats.
Assignments use `name = value`, with decimal, negative decimal, hexadecimal,
percent-prefixed or packed four-byte values. **Verified:** the original percent
syntax returns zero; quoted input packs the first four following bytes after
lowercasing. Decimal and hexadecimal conversion consume the numeric prefix and
wrap at 32 bits. Assignments can also read a named SOUP flag's current value.
Unknown names report a diagnostic and use one; unavailable or stale world
metadata blocks symbolic assignments. Short quoted input remains unsupported.

Console preferences load from native `REDGUARD.CPF` in the config directory or browser
localStorage `cyrus-console`, with the read-only installed CPF as fallback when
local state is absent. `console save` persists preferences; `console write`
writes native `CONSOLE.LOG` in the config directory or downloads it in the browser.
`console show` reports preferences and load errors. Malformed CPF blocks preference
edits and saving. **Verified:** the original loader clamps hex line counts below
one and does not enable shadows from a saved nonzero shadow value.

`console font`, `shadow`, `paper` and `col0` through `col3` change appearance;
`col3` requires paper off and the current world palette. `console hex,number`
and `console limit,number` retain diagnostic preferences; raw memory dumps and
on-screen diagnostic logging remain unsupported. The font selector uses the
installed GUI font's original linked copies. `show world` reports supported
environment settings. `fxskyscale`, `fxskylevel`, `fxwaveamp`, `fxwavespeed` and
`fxwavefreq` accept signed numbers or live flag names and retain edits for the
session, including world revisits. `get` and `set` report no matching console
variables, as the original Glide variable table is empty. `getflag pattern` reads live SOUP values/types/latches;
`setflag pattern,value[,status]` assigns values and optionally marks their latch.
Status zero remains unsupported because its original metadata XOR changes the
flag type; rejection leaves every flag unchanged. `load world[,marker][,angle]` requests a world
load through the same path as the script `LoadWorld` native: the world must be a WORLD.INI entry from 0
through 99, the marker supplies the start position and the angle (masked to 0 through 2047) the heading,
and omitted values are zero. The request waits while the console is open: a later `load` replaces it,
closing with F12 applies it and Escape discards it (**Verified** in the original's key and world loops,
preservation `engine/console.md`; a command run through the control port with the console closed applies at
once). It accepts numeric literals only, not symbols; a missing marker reports a
placement error and keeps the player where it was. `item add,id` gives the selected player one item through
the session inventory, the same acquisition path as the `AddItem` native (**Inferred** from the console reference;
the original handler was not compared). The other `item` operations (`player`, `show`, `sub`, `drop`) report
that they are unsupported. `getmapmarker number` moves the selected player to the loaded map's marker of that
number and keeps its heading (**Verified** from preservation `engine/console.md`; not compared in the original);
a marker beyond the map's table or without its presence byte reports an error and moves nothing. The built-in
table used when no map table is loaded reports unavailable. `setmarker number` saves the selected player's
position and orientation and the published camera pose under the current world and number in a marker file in
cyrus's local state, and `getmarker number` moves the player to the first saved marker of that world and number,
position, orientation and camera pose (**Verified** from preservation `engine/console.md`). The camera values are
written at the command boundary; the next ordinary camera update retains its normal ownership. The file
never lives in the install: the native game keeps it as `REDGUARD.MRK` in its local state directory, the browser
keeps it in local storage under `cyrus-markers`, and a host without marker storage reports `setmarker` and
`getmarker` unavailable. `delay [number]` makes every main-loop pass wait that many milliseconds in the native
game (**Verified** from preservation `engine/console.md`); none, zero or a non-number clears it, a negative number
is refused, and the browser reports it unavailable because its wait would block the page.
Other catalog entries report unavailable owners.

Browser `savecheats` persists to localStorage `cyrus-cheats`. F11 Cheat files
imports and exports CHT using the shared codec. A valid explicit import can
recover malformed local state; failed imports and saves retain existing state.

**Verified:** `neversaydie` supplies the documented distance override during
automatic world updates and explicit script steps, unless `distance_override`
is supplied by the caller. `goldfinger` holds the selected player's burning torch at a full
counter: with any nonzero state, on the update that runs the torch burn (every eighth world-loop
frame, outside the dousing state) the counter becomes exactly 100 and neither the water branches
nor the countdown run; a zero counter stays zero. `drno` skips the ordinary main stage, including its
response timers, for every actor except the selected player and the camera actor (**Inferred**: the original's
early return is **Verified**; that its skipped part is this stage is inferred from the task-system test beside it
and the matching timer bookkeeping). It acts only where the caller supplies the cheat state: the automatic
update and the control port do, direct stage calls without it do not. `magiccarpet` (the value exactly 1, not any
nonzero value) makes the selected player's ground-support probe clear its falling byte and end. `task` and `animation` are the
live replacements for the `SYSTEM.INI` `task_system` and `animation_system` switches the startup restore
already seeds: each is on only for the value exactly 1, and the ordinary update carries the state into the world,
so the existing gates respond to a console toggle (with `task` off the ordinary main stage and the hook gates
run no scripts; with `animation` off the animation requests, advancement and the timing checks that read the
switch stop). `moonraker` (any nonzero value) takes the selected player out of the late collision stage's forward obstacle probes and actor overlap pass; other actors still collide. (The original also keeps the overlap-enable bit of the player's control record in step with the slot; cyrus gates the stage itself.) While `animation` is exactly zero the selected player's body is not drawn; other values leave it
drawn. The `animation` value 2 runs the animation system (requests, advancement, timing) but skips the movement
natives' wait for their requested group, which only the value 1 keeps, as the original's checks split. `yeahbaby`
integrates its ground bypass and debug navigation handler. The bypass is on for a nonzero state or in developer mode (`[debug] final_version=0`,
where missing key state leaves it inactive): while Page Up or Page Down is held, or the Home toggle (flipped once
per Home press while it is on, and kept while it is off) is set, the selected player's probe clears the living
state, the sliding, queued-motion and falling bytes and the motion classification and ends before any support
query; unknown key state refuses the update. In the same state the selected player's handler moves the actor by
16 map units for each of Page Up (up) and Page Down (down) held once per update, so both held cancel; Insert held suspends the camera's
movement limiting (bit 0 of the selected camera record's flags, set on the first held update and restored on release
or when the handler turns off, on the record selected then) and moves the actor forward by the frame scale times
128 Q8 each update. With the camera session absent, Insert refuses the update. The catalog reports
`yeahbaby` as a connected `ground_bypass` effect; the control port's Page Up and Page Down presses reach the handler and move the player 16 units per update (**Verified** in a native run). Installed START tests drive
these keys and cheat states through ordinary Bevy app updates; they do not establish host parity. The keys are
sampled from the keyboard once per frame, under the same input locks as the quick keys (console, logbook and
loading read them as released), and read by that frame's ordinary update; a Home press in a frame where the
ordinary update does not run is not retained. Other gameplay
effects report unsupported.
**Unknown:** the original gameplay effect of `oddjob`; its raw state still
supports toggling, assignment and persistence. These commands do not enable
unsupported gameplay systems.

## Native startup capture

Attach `python3 dev/scripts/native-startup-capture.py <control-port>
<host-timeline.json> --expect movie` to a native production launch. Choose
`--expect menu` for a book-menu launch or `--expect world` for world entry.
Run the game with a host-mounted `CYRUS_CONFIG_DIR` and hold the shared
`/tmp/cyrus-live.lock` for its lifetime. The observer does not launch the game,
change its startup options, or acquire a second lock.

Preparation has its own `--prepare-seconds` deadline, default 180 seconds.
The presentation deadline, `--seconds`, defaults to 45 seconds and starts
when the title becomes visible. These are capture limits, not fidelity timing
constants. A preparation screenshot alone cannot complete the capture.
Success requires the title and the requested endpoint: a ready book, a movie
with a presented frame, or positive game-clock progress in `InGame`. World
progress always stops observation, including when a different endpoint was
expected. A ready book fails immediately when a movie was expected.

The JSON timeline retains complete control samples, transition screenshot
requests, the stop reason and success status, including on timeout. Screenshot
paths are resolved by the game's process; `--screenshot-dir` defaults to
`/out` for a container with the capture directory mounted there. A screenshot
request acknowledgement is not proof that its asynchronous PNG write finished.
Keep the process alive until the requested files exist before collecting them.
The observer exits nonzero on an incomplete or unexpected startup sequence.
Keep timelines and images under ignored `magic/` and compare the same install,
splash variant, autosave state and launch options in the original and native.
Wall times alone do not establish game-clock transition parity.

## World audit and matched captures

`cargo run -p cyrus -- --audit-worlds` scans every installed WORLD.INI entry
without opening a window. Its text and stable JSON reports live under
`magic/artifacts/world-audit/`. They reuse parser coverage, normal scene export,
and script preparation. Reports distinguish skipped placements, referenced asset
failures, script decoding/preparation, and observed runtime failures. Uninitialized
animations are counted explicitly; an unexercised operation is not verified.
See [cyrus](../crates/cyrus/README.md) for the report's scope and exit behavior.

[Replay fixtures](../dev/fixtures/README.md) define matching original/native/browser
worlds, viewpoints and capture timing. `just run-native --fixture <fixture.json>`
launches the native view; `just replay <fixture.json> <new-label>` validates the
views and writes the triplet under `magic/captures/`. Original instrumentation and
all copyrighted captures remain local. Legacy JSONL fixtures still work but do
not establish a matching original camera.

Native `state.scene_ready` and browser `window.__cyrusCapture.ready` report that
this world's render instance has finished spawning. Replay waits for that signal
before the fixture settling time. `InGame` can precede asynchronous geometry
loading; it is insufficient for a scene capture. Script and gameplay readiness
remain separate.

Maintainers restoring the original adapter should use the complete
local guarded tooling bundle and the [fixture workflow](../dev/fixtures/README.md).
Pass `--static-capture` to the restore helper. It checks archive and executable
hashes, copies the install and Glide overlay into an isolated writable runtime,
and disables the documented task/animation SYSTEM.INI switches before startup.
Enter New Game, inspect the loaded ship deck and F12 `oracle` task 0/animation 0,
then close the console before fixtures. ISLAND's fixture loads twice; its adapter
reopens the console between loads. Check fresh measurements and Docker identity
after restarting; an interrupted adapter's saved dump is not a new observation.
**Unknown:** reconstruction of the recorded debugger-enabled emulator from source
and original CD/audio parity. Static captures do not verify either.

After copying the guarded bundle under `magic/`, restore without launching:

```sh
python3 magic/original-tools/restore-original.py \
  --snapshot /absolute/path/to/research/supplements/2026-10-13-stage0-tooling \
  --install /absolute/path/to/Redguard \
  --emulator /absolute/path/to/recorded/dosbox-x \
  --runtime /home/vibepod/workspaces/cyrus-original-runtime \
  --container cyrus-original-runtime --static-capture
```

Keep all five instruments beside `restore-original.py`. The earlier snapshot
supplies runtime templates; the guarded bundle supplies the corrected instruments.
Choose a fresh runtime path and an unowned container name. If the guarded
runtime image is absent, build its prerequisites:

```sh
docker build -t cyrus-retro:dev dev/docker/retro
docker build -t cyrus-stage0-retro:20261013 - < /home/vibepod/workspaces/cyrus-original-runtime/Dockerfile
```

Run the generated `start-retro.sh` through the managed preview workflow above,
then supply the adapter:

```sh
export CYRUS_ORIGINAL_CONTAINER=cyrus-original-runtime
export CYRUS_ORIGINAL_CAPTURE=/home/vibepod/workspaces/cyrus-original-runtime/magic/instruments/original-capture.py
just retro --fixture dev/fixtures/island.json island-original-check
```

After inspecting that original-only result, follow the fixture README to build and
run native/browser and produce a new full triplet. Existing labels are rejected.

### Installed browser session CD check

For the diagnostic session audio check, put the `wasm-bindgen` CLI version
listed in `Cargo.lock` on your path before building. Build the game with
`dev/scripts/build-web.sh cyrus-web` and serve that output with an explicit,
read-only install using `just run-web --install /absolute/path/to/Redguard`
(or `dev/scripts/viewer-server.py --page index.html --port PORT --install …`
when running a dedicated local acceptance server).

Use Chromium with a DevTools endpoint and a secure page origin. With Chromium
in a container, forward the server port to Chromium's loopback so AudioWorklet,
IndexedDB, and install access work there. Run from the checkout root with Node
26 and Python 3 with the `blake3` module installed:

```sh
python3 dev/scripts/build-provenance.py verify target/web/build-provenance.json
SESSION_CD_PYTHON=/path/to/python node dev/scripts/test-browser-session-cd.mjs \
  http://127.0.0.1:9222 http://localhost:8080/ http://localhost:8080/original/
```

The driver owns and disposes an isolated browser context. It defaults to
scene 20, which must have active installed CD music and a HUD. It waits for the
real track to approach its seam before saving; the wait is bounded by the
track duration, a deadline, and a game-frame progress check. It checks a late
publication rejection with a separately framed copy of the named save and
then loads the intact save. It compares both channels of the rendered graph
with the original source across the rejection and the restored loop seam,
and checks that the old source was stopped and disconnected. It emits
measurements and browser artifact hashes, without exporting audio or saves.
It records PCM preparation separately from publication and requires channel
copies to finish before retirement. Prepared replacement and old-source stop
must share one scheduled render-clock boundary. This does not establish
physical or original-game gaplessness. This checks the WebAudio
graph, not speakers or original-game audio timing.

### Installed browser speech/effect session check

Build and serve a sealed browser with an explicit install as above. Select a
voiced installed RTX label long enough to remain active through preparation and
an installed unbounded effect. Run the driver with its DevTools endpoint and
secure install origin:

```sh
SESSION_CD_PYTHON=/path/to/python node dev/scripts/test-browser-session-samples.mjs \
  http://127.0.0.1:9222 http://localhost:8080/ http://localhost:8080/original/ LABEL 25
```

The opt-in audio adapter uses the ordinary control voice and generation-owned
script sound paths. The driver saves during both sounds, rejects a missing voice
source hint and a late HUD publication mismatch, then loads the intact document.
It checks source-decoder checkpoints, fresh transport tokens, sample slots and
script generation bindings. The examine voice-stop path must remove only the
restored voice. A parallel AudioWorklet compares actual stereo graph output with
the submitted production mixed buffers before and after publication and beyond
the queued-buffer drain watermark. Already mixed buffers retain their documented
drain policy. This checks the browser graph, not physical speakers or original
game audio parity. Runs own an isolated browser context and use game-clock
progress cutoffs; a caller should also hold a per-run lock and wall-clock limit.

### Resumable native rendered session capture

With a rendered native process exposing its public control port, capture a
START/ISLAND diagnostic session round trip without rebuilding the executable:

```sh
python3 dev/scripts/native-session-capture.py 7126 /absolute/host/evidence \
  --binary /absolute/path/to/cyrus --runtime-output /run/evidence
python3 dev/scripts/test-native-session-capture.py
```

The host output and runtime output must designate the same mounted directory.
Keep `CYRUS_CONFIG_DIR` host-mounted and the install read-only. Use a separate
output directory for each executable/fixture. A nonblocking per-directory lock
excludes simultaneous drivers. Preparation (including movie export) has a
separate `--prepare-seconds` deadline; `--seconds` bounds operations and gameplay
progress waits (180 seconds by default). `--game-seconds` specifies the required game-clock advance.

Rerun the identical command after a runtime restart. Before a durable checkpoint,
the driver visits both worlds again. Afterward it validates executable and saved
document hashes and republishes the saved capture without repeating visits.
Rejected publication, missing screenshots and absent game-clock progress fail the
run. Checkpoint files survive failures; a prior result is removed before retry.
The timeline, session documents and screenshot stay local and must not be tracked.

A successful `result.json` verifies publication, subsequent gameplay progress,
retained world source identities and references from rendered bindings to saved
world generations and actor indices.
It does **not** establish attachment, animation, dormant-world or pixel parity:
compare the before/after documents and rendered capture separately. The driver
intentionally leaves that comparison unverified rather than treating a successful
load as full #46 acceptance.

Installed browser movie-to-world acceptance is available through
`node dev/scripts/test-browser-movie-handoff.mjs CDP_URL PAGE_URL INSTALL_URL OUTPUT`.
Use a secure browser origin, such as HTTPS or browser-local loopback, and a server
started with the explicit read-only install. The isolated context follows the
installed movie skip controls and checks the final exit. It stops animation
callbacks at ordinary update four, before screenshot readback, and checks the
Introduction entry heading and following camera offset.
It does not rerun fresh/autosave policy cases or claim natural full-movie timing parity.

The original rope driver retains readiness diagnostics separately from gameplay
samples. `reader_failed` and `reader_invalid` preserve reader errors; `actor_absent`,
`selection_absent` and `selection_ambiguous` distinguish actor lifecycle from a
reader failure. Optional reader `readiness` metadata records binding diagnostics.
A readiness budget expiring does not establish that loading stalled.

The original launcher stores atomic progress in `OUTPUT/config/rope-readiness.json`
and appends observations to `contact.jsonl`. After a timeout before input, rerun
with the same paths and `--resume`. Reader and placement identities must match.
Waiting can reconnect to a new runtime because no input has begun; once console
or gameplay preparation starts, an interrupted attempt cannot be replayed
through resume. Completed attempts also refuse replay. A per-journal lock excludes
concurrent drivers. Direct driver invocations opt in with `--progress PATH`.
Observations retain host timestamps, elapsed readiness time and engine counters;
the journal records the latest observation and last progress poll.

The original rope driver accepts `--record` to capture DOSBox-X mixer audio
only after a valid placed baseline. It stops the recorder on completion or
failure; the launcher mounts the run output's `audio/` as the capture directory.
A recording alone does not establish which sound occurred.

`dev/scripts/native-trace.py` samples the selected player through an observation-only
`schedule_input` event at an ordinary update boundary. Pose, motion, health,
animation, game clock and logical generation come from that single snapshot.
`frame` is the ordinary update number. Placement observes the current boundary;
continuation requests the next ordinary update instead of polling the same
boundary again on fast control replies. A generation change, duplicate/reversed
update or nonadvancing clock rejects continuation, and `missed_updates` reports
sampling gaps. Selected-actor failures are errors rather than healthy samples.
Held trace inputs are released on failure. These observations do not establish
exact input timing: `--key` still uses asynchronous public input requests. Use an
acknowledged schedule for a matched input comparison.

`dev/scripts/native-held-jump.py` compares a complete atomic original held-Space
receipt with a native public schedule. Start both runs in the same installed world
with the same supported, stationary player, heading, attributes and clock cadence.
The original receipt uses the ordinary-boundary reader's `observations`, `steps`
and explicit release `input_edges`; retain its reader, executable and scene
provenance with the capture. No original memory layout is shipped with this driver.

```sh
python3 dev/scripts/native-held-jump.py --addr 127.0.0.1:7117 \
  --original ORIGINAL/held.json --updates 32 --out NATIVE/accepted.json
# Recheck finalized receipts without launching either game:
python3 dev/scripts/native-held-jump.py --native NATIVE/accepted.raw.json \
  --original ORIGINAL/held.json --updates 32 --out NATIVE/rechecked.json
python3 dev/scripts/test-native-held-jump.py
```

The native offset-zero baseline precedes the acknowledged press at offset one.
The driver aligns subsequent consumed Q8 clock scales, explicit release, exact Q8
positions, animation groups, health and fall/queued/slide/living/rope state. Both
ends must be idle at the same position. Each attempt is limited to 32 consumed
updates plus the admission baseline so a slow rendered run can finish within the
public scheduler's wall deadline. A deadline or incomplete sequence is rejected,
never promoted from matching prefix evidence. The driver releases Space on
failure, retains the complete native response before comparison, removes stale
accepted summaries before a new attempt and hashes both receipts in a successful
summary. The launcher must retain actual executable/source identity separately;
a receipt hash does not establish binary provenance. Use a per-run live lock and
host-mounted config/output directories. After a reset, reuse a verified retained
executable and `ensure-runtime-image.sh` archive, then restart only the incomplete
attempt. Do not treat an erased compiler ticket as a completed build.

`dev/scripts/native-edge-trace.py --out <ignored-path> --release-update <N>
--updates <M>` captures one scheduled edge attempt from an externally established
alive, ordinary, nonfalling pose with no queued motion or sliding. Observation zero precedes the default ArrowUp press at
update one; `--key` and `--press-update` select the trigger. Every boundary through
M is requested in the same schedule, including the acknowledged release at N.
Acceptance rejects gaps, generation changes, early falling, missing falling entry,
and a missing health decrease followed by a nonfalling continuation boundary.
The raw response is retained as `<out>.raw.json` even when validation fails;
accepted JSONL is written only after validation, replacing any previous result.
`native-trace.py` waits for the first admitted ordinary update as well as loaded
scripts, avoiding placement in the initial Loaded-before-update window.

Use `--place X,Y,Z --heading H --settle-updates N` to restore a verified
floor position through public logical placement before arming input. The driver
first requests an observation-only sequence, retaining it as
`<out>.support.raw.json`. Its final three consecutive boundaries must have a
valid stable position, no rope attachment, ordinary nonfalling controls, unchanged
positive health and identical position. Failure stops before the input schedule.
Choose the floor and approach from installed geometry and original evidence;
placement alone does not establish support.

This verifies acquisition completeness, not original parity or edge-service
attribution. Compare the retained clocks and actor states against original evidence
before accepting gameplay behavior. Schedules retain the public service's bounded
execution deadline and 256-event limit.

Replay an accepted original rope schedule through a running native control port:

```sh
python3 dev/scripts/native-rope-contact.py --addr 127.0.0.1:7117 \
    --original ORIGINAL/contact.jsonl --output NATIVE/contact.jsonl
```

The driver rejects incomplete or contaminated original schedules before sending
input. An accepted schedule must contain `start` and `end` boundary rows for each
walk, attack and continuation phase, with `input_ack: true` and cumulative
`clock: [before_seconds, after_seconds]` ranges. Older update-count traces need
new bounded timing samples; FPS or a single update's tick increment cannot
reconstruct their elapsed game time. The original reader supplies cumulative
`game_seconds`, `clock_end`, and observed `input_state` for `Up` and the attack key.
The original driver brackets key delivery and waits for the observed key state.

The native driver pulses the sword toggle for one update, so asynchronous reads
do not hold it through animation completion and request sheathing again. It
prepares the gravity cheat through public controls,
places the player at the observed original baseline and records attachment state.
It submits each walk, attack and continuation interval through `schedule_input`.
Transitions apply before ordinary input sampling at offsets from the next admitted
update; continuation uses observation-only boundaries. An observation-only ordinary interval
chooses an attempted update count, but acceptance uses only the actual clocks
returned with the boundary acknowledgements. Each acknowledgement also identifies
the world generation, ordinary update and selected actor state before that update.
Its timestamp excludes the forthcoming update, even when frame scales vary.
Skipped boundaries or a changed generation reject the attempt. Setup state reads
still bracket actor reads so their uncertainty remains visible. `--clock-tolerance SECONDS` declares the
maximum timing error, including both runs' boundary uncertainty; the default is
exact. A mismatch fails before any displacement/contact comparison is emitted.
A `comparison: "matched"` row establishes timing within that budget, not gameplay
or sound equivalence. All held input is released on failure, and host deadlines
bound stalled game clocks.

For native sound evidence, run the driver in the same PulseAudio environment as
cyrus under `dev/docker/dev/paced-audio.sh`, adding `--record-mixer OUTPUT.wav`.
This records the paced sink's actual `cyrus.monitor` mixer output through `parec`,
finalizes it on success or failure, and rejects empty, failed or unexpected-format
recordings. The runtime needs `pulseaudio-utils` and `libasound2-plugins`, supplied
by the dev image. ALSA `type null` is unsuitable. A recording alone does not
identify a clank; compare it against the retained original WAV over accepted
clock intervals. Driver controls run with
`python3 dev/scripts/test-native-rope-contact.py` and
`python3 dev/scripts/test-original-rope-contact.py`.

Native rope readiness writes an atomic `OUTPUT.progress.json` before any input.
After a readiness timeout or process restart, rerun the same command with
`--resume`. It verifies the accepted original schedule hash and preserves loading
observations. A completed run validates its output hash and returns without
sending input again. An interrupted input attempt is never automatically replayed;
keep its partial JSONL and use a new output path for a fresh comparison. The output
lock rejects concurrent drivers for the same attempt. `--ready-timeout` bounds
startup separately from each world-progress interval.

`node dev/scripts/test-browser-world-entry.mjs CDP_URL PAGE_URL INSTALL_URL OUTPUT`
uses the public New Game/movie-skip launch path to compare the first-world
presentation at ordinary update four. It checks destination, camera, fade and
player preparation, retains the image, and closes its isolated page within a
90-second endpoint bound. Compare its image with the original at the same
ordinary update; host wall duration does not establish fade timing parity.

For animation/attachment continuation, reuse a captured two-world seed and select
an installed animated actor with an equipped attachment in each world. Actor
indices come from `scripts` and the seed's retained actor census:

```sh
python3 dev/scripts/native-session-capture.py PORT HOST_OUTPUT \
  --binary EXECUTABLE --runtime-output RUNTIME_OUTPUT \
  --continuation-seed HOST_SEED --runtime-seed RUNTIME_SEED \
  --attachment-actor ACTIVE_INDEX --revisit SCENE --revisit-actor DORMANT_INDEX
python3 dev/scripts/test-native-session-assertions.py
```

This mode restores the seed instead of repeating world visits. It captures a new
durable fixture with a precise pre-capture observation, then arms the public
`rendered_actor` generation wait before publication and before revisiting the
dormant world. Each reply records the first settled presentation update's frame
and game time after rendered readiness and retained revisit restoration. Assertions
require the displayed frame to match that update's
logical model, a validated rendered model source, fresh actor ownership, and
matching attachment sources, visibility, local transforms and world matrices.
Saved frames may advance; a changed frame alone is not a failed continuation.
The dormant logical document must remain unchanged before revisit, apart from
generation rebinding. First-update observations and the revisit screenshot remain
local. This fixture does not establish original-game pixel or timing parity.

For installed light restoration, `native-light-session.py` uses the public native
session boundary and material-bound light-image observation. It visits both worlds
to build a fixture on the current executable. Supply an enabled locked steady light
actor index, a uniquely named scene mesh and a view containing active map lights:

```sh
python3 -m pip install blake3==1.0.11
python3 dev/scripts/native-light-session.py PORT HOST_OUTPUT \
  --binary EXECUTABLE --runtime-output RUNTIME_OUTPUT \
  --scene ACTIVE_WORLD --dormant OTHER_WORLD --light-actor INDEX \
  --node UNIQUE_MESH --position X Y Z
python3 dev/scripts/test-native-light-session.py
```

An optional `--runtime-seed RUNTIME_SEED` reuses a compatible captured session.
The driver verifies its requested inspection view and suspends gameplay through the
public console during asynchronous session preparation, then closes it and requires
game-clock progress. It saves its public fixture and requires publication with matching
bound light rows, saved scripted-light state and fresh scene ownership, then submits
an integrity-valid negative document with inconsistent installed light eligibility. Rejection must
preserve the live generation, mesh/material/image bindings and light rows. Leaving
the world must remove its named binding; revisiting must restore matching light
rows and saved logical state under a new owner. The final screenshot remains local.
CPU material rows establish binding continuity, not GPU readback or original
visual/timing parity.

For original timing acquisition, `original-rope-contact.py --schedule-command PATH`
uses a local executable specific to the original build. It receives one JSON
request on stdin with `phase`, `frames` and `key` (`null` for observation only).
It returns `phase`, `key`, the installed `normal_frame_rate`, two `boundaries`,
and exactly `frames` consumed `steps`. Each boundary contains `frame`, integer
`clock_scale` in 1/256 nominal tick, `input_state` and the selected `player`.
Each step contains its contiguous `frame`, consumed integer `frame_scale` and
`input_state`. The end clock must equal the start plus all consumed scales;
held input must be acknowledged through every step and released at the end.
The driver retains the backend evidence before validating it and emits exact
boundary clocks only after the complete schedule and baseline pass. Backend
identity is included in the readiness journal. Build-specific instrumentation
stays local, outside the tracked tree. Record its method in `instrumentation`;
debugger stops can change measured FPS, so exact boundary timing alone does not
establish ordinary playback cadence or gameplay parity. Without this option,
original host-input acquisition retains its conservative clock ranges.

`original-rope-contact.py --schedule-frame-scale N` requires every consumed
instrumented update to use that measured Q8 scale; the backend must wait for an
admissible start and the driver rejects a changed scale. `--attack-frames` and
`--continuation-frames` select bounded reacquisition lengths alongside `--frames`.
These constrain observations and never set the original simulation clock.
Native planning rejects an exact interval that is not a whole number of the
measured ordinary interval before sending gameplay input. A reported common
interval is a reacquisition candidate, not accepted timing evidence.

For an ephemeral original image, set `CYRUS_RETRO_IMAGE_ARCHIVE` to a local Docker
archive containing `CYRUS_RETRO_IMAGE`. The launcher restores it before taking
the live-run lock, serializes concurrent restores and verifies the requested
image exists. An existing image requires no archive load. The helper
`dev/scripts/ensure-runtime-image.sh IMAGE ARCHIVE` also supports native run rigs.
Archives remain local; recovery never removes peer images or containers.

For separate transient-geometry restoration, run the native diagnostic driver with
an explicit installed game, the host config directory mounted into the runtime,
and the same per-run locking used above:

```sh
python3 dev/scripts/native-transient-session.py PORT HOST_OUTPUT \
  --binary EXECUTABLE --runtime-output RUNTIME_OUTPUT \
  --scene ISLAND --dormant START --game-seconds 0.4
python3 dev/scripts/test-native-transient-session.py
```

The driver selects a retained effect record when available. Otherwise it selects an
expiring image from the installed effect catalog. Its position is derived from
the camera pose in the actual public capture, so deferred entry framing cannot
leave the fixture outside the view. Its local diagnostic fixture activates that
installed descriptor at speed zero and
holds its image frame to isolate publication from lifetime and animation. It uses
the public inspection camera and session capture/load, without modifying the
install. This fixture does not establish the producer's original timing.
The `effect_geometry` control observes actual billboard mesh/material/texture
bindings after PostUpdate. Optional `after_generation` arms one bounded request
before replacement and samples the first ready presentation update under the new
generation. It waits for dormant-world service restoration and an admitted
model-ready world clock sample for that generation, but does not retry
missing geometry or repair bindings. Responses include acquisition frame, actual
vertices, transforms, image-byte hashes, logical and retained slots, and all live
billboard entity identities. These are CPU observations, not GPU readback.

The driver requires matching geometry under fresh ownership, removal of old
entities/assets from the selected slots, preservation after an integrity-valid
invalid effect-source load, and dormant-world revisit continuation. It then
resumes the installed lifetime rule and requires the expired effect to stop
submitting. Captures, negative documents, first-update observations and the final
PNG stay local. Preparation and game-clock progress have separate bounded
cutoffs; slow decoding alone is not treated as a stalled game clock. Original
pixel and timing parity remain unverified.

`native-transient-runtime.py` recovers the minimal software-render runtime after
Docker images disappear, without invoking Cargo. Supply the retained executable
and its independently recorded digest:

```sh
python3 dev/scripts/native-transient-runtime.py \
  --runtime /absolute/host/runtime --binary /absolute/host/runtime/cyrus \
  --sha256 EXECUTABLE_SHA256 --install /absolute/path/to/Redguard \
  --cache /absolute/host/cache
python3 dev/scripts/test-native-transient-runtime.py
```

Bind-mount sources must live under `/home/vibepod/workspaces`. The launcher checks
the executable digest, reuses its minimal image when present and reloads its host
archive when the
Docker image disappears. When both are absent it builds only that image. Image
tags and archive names include the runtime recipe digest; completed image saves
replace the archive atomically. A host lock prevents overlapping runs of this runtime.
Each attempt retains image provenance, image-build output, runtime output and
driver output in `evidence/transients-current/attempts/`. Each attempt also
retains the prior observation/session/image files before the driver replaces them.
The host config directory
survives container retirement; its cache link resolves to the explicit cache mount.
Only the container created by that invocation is stopped.

The transient driver atomically checkpoints preparation, successful publication,
rejection, revisit and expiration, including a public session and evidence hashes.
After interruption, the same invocation validates executable, driver, options and
retained evidence, publicly loads the last completed stage and resumes outstanding
assertions. Incomplete stages rerun. Changed inputs require a fresh output directory;
a stale result file cannot satisfy a new run. Package gates remain reusable when
their source and gate inputs have not changed.

`--seed-session PATH` can reuse a retained installed diagnostic fixture for
bootstrap recovery. The application still validates and publishes it through
public session load. Completed checkpoints take precedence over a bootstrap seed.

For automated transient GPU-output acceptance, add `--gpu-pixels` and an explicit
`--seed-session` pointing to the retained stationary installed fixture. Install
`Pillow==11.3.0` and `blake3` into the Python environment first. Output goes to
`evidence/transients-pixels`, preserving the earlier binding checkpoints.
`native-transient-pixels.py` reuses the public screenshot command: Bevy copies
the rendered texture into a GPU readback buffer, maps it, and writes the PNG.
The driver waits for a complete new PNG within the existing screenshot deadline.
It holds the installed effect image at frame zero, derives a visible signal from
paired empty/active public sessions in a central crop excluding HUD, and requires
that signal after restored admission and rejected loading. Empty replacement
and expiration must match the empty control at those pixels. The oracle rejects
missing output, old visible output after retirement, dimension changes and weak
positive controls. Run `python3 dev/scripts/test-native-transient-pixels.py` for
synthetic mutation and real PNG-decoding controls. This checks actual GPU output
after admission, not the exact first GPU frame or original pixel parity.
The pixel driver checkpoints each completed output assertion with executable,
seed, driver and helper identities plus readback/session hashes. A restart
validates those inputs, skips accepted stages and establishes the live-state
precondition for the next incomplete stage through public loading. Modified
inputs or readbacks fail explicitly. Completed pixel evidence remains separate
from the logical/binding driver's checkpoints.

For positive rope attachment, use `--attachment` on both rope drivers with a
placement derived from an installed rope and a backend supporting the original
`space` key. This is a separate unarmed scenario: Jump approaches the rope,
released input must retain attachment, and a second Jump must detach. `--frames`,
`--continuation-frames` and `--attack-frames` bound those three phases respectively.
Original acquisition requires acknowledged ordinary boundaries. Both drivers
reject absent or wrong attachment outcomes; airborne state is admitted only in
this explicit unarmed scenario. The default armed/nonfalling contact guard stays
in force. A positive diagnostic fixture does not establish natural cadence,
weapon contact or clank parity.
For a gravity-enabled release, add `--physical-release` to original acquisition.
It keeps fly mode for placement and attachment, then turns it off after hanging
and records a `physical-release` observation before the detach schedule. Native
replay admits that observation only between the acknowledged hang and detach
boundaries and turns fly mode off at the same phase. This prepares a physical
fall; the observation alone does not prove landing, natural cadence, or clank
parity. Keep the ordinary-boundary clock and outcome checks for those claims.
For a bounded Jump pulse followed by released continuation, add
`--attack-frames <pulse-updates> --landing-frames <released-updates>`.
`--landing-frames` requires physical release. The backend receives `release_after`
and must report a single acknowledged `input_edges` release at that update.
Every subsequent consumed step must report Jump released. The endpoint must be
detached with falling and queued motion cleared, and the atomic sequence must
contain a landing animation. Held-input reattachment is rejected. Native replay
plans the pulse and whole interval separately, acknowledges the release and
samples each continuation boundary; both intervals retain the exact clock oracle.

Physical acquisition also requests `atomic_snapshots` from the local scheduling
backend. It must stop gameplay execution at `ordinary_update_start` and return
one `snapshots` entry per boundary, including both endpoints. Each entry carries
its zero-based `sequence`, global `frame`, cumulative `clock_scale`, selected
`player`, and `execution: "stopped"`. A second read while stopped supplies
`confirmation: {frame, player}`. The driver rejects repeated/skipped frames,
changed confirmation state, different boundary kinds, missing entries and clocks
that disagree with consumed scales. Input endpoint state must equal the
corresponding snapshot. Accepted endpoint rows report `snapshot_boundary` and
`snapshot_sequence`; raw evidence retains the entire sequence. Native physical
replay revalidates that evidence and requires its published endpoints to agree
before input. Backend stops can affect cadence, so atomic acquisition alone does
not establish natural timing.


## Paced original book audio

`dev/scripts/original-book-audio.sh RUN GOG OUTPUT IMAGE_ARCHIVE READY.py TRANSITION.py`
restores `cyrus-retro:dev` from the explicit local archive when necessary, then
records the real PulseAudio sink monitor. The image must include PulseAudio,
its utilities and the SDL PulseAudio backend dependencies. Build the retro
Dockerfile and retain a local image archive before an ephemeral runtime reset.
The default retro runner keeps dummy audio; this launcher explicitly selects
PulseAudio. No hardware renderer or host audio device is required.

Use host-visible paths under `/home/vibepod/workspaces` for the writable run,
GOG files and output. The launcher copies its small scripts into the output,
mounts config and evidence there, holds per-output and per-run locks and removes only
its own container. Existing capture evidence is never overwritten. An optional
`CYRUS_DOSBOX_CONFIG` selects a local emulator configuration. The run must use
real writable configuration files and read-only links to the shared assets.

For isolated rustle measurements, set `CYRUS_BOOK_REALTIME=1` and
`CYRUS_BOOK_ISOLATE_MUSIC=1`. The former enables DOSBox-X adaptive maximum cycles
for this run, allowing its guest mixer to keep pace with the real sink instead
of demanding a fixed emulated CPU budget under host contention. The latter
copies the private run's `SYSTEM.INI` into evidence, changes its enabled
`redbook` setting to `off`, and mounts that file over the guest configuration.
The source configuration stays unchanged even if the runtime disappears.
Linked configuration files, canonical installs and unsupported settings are
rejected. This isolates overlapping music from the effect waveform; a mixer
command before game launch is insufficient when the game subsequently sets
its CD audio volume. These capture options do not change cyrus audio policy,
the interactive original's defaults or the continuity rejection limits.

The readiness observer must wait for an identified book page, then print one
JSON object such as `{"ready":true,"page":"options"}`. A DOSBox window or title
screen is not book readiness. The transition driver sends ordinary input and
observes the destination page. It prints JSON lines containing `from`, `to`,
`sent` and `acknowledged`. Times are monotonic seconds relative to the supplied
`CYRUS_CAPTURE_EPOCH`. Keep installed image references and observer scripts in
local evidence, outside the tracked tree. A changed screenshot alone does not
identify the requested destination.

`CYRUS_BOOK_TIMEOUT` bounds each readiness and transition process separately
(default 120 seconds). The recorder starts only after readiness, admits PCM
before input and retains the driver log, WAV and result on failure. Its result
checks stereo 16-bit 48 kHz PCM, wall-clock pacing, nonzero audio and valid
acknowledgements starting at the observed page. The 250 ms default pacing
allowance covers recorder admission and shutdown uncertainty. These checks
establish a paced audible recording; they do not establish uninterrupted game
mixing, rustle identity or parity with cyrus. Compare the waveform and installed
effects separately before changing production scheduling or gain.

Run the failure controls without an install or Docker:

```sh
python3 dev/scripts/test-paced-mixer-capture.py
python3 dev/scripts/test-original-book-audio.py
python3 dev/scripts/test-ensure-runtime-image.py
```

## Global sample stop

For global sample-stop acceptance, `dev/docker/dev/Dockerfile.sound-acceptance`
adds the project's paced Pulse sink to the minimal rendered runtime. Mount an
explicit read-only install, host config/cache and output, then run
`python3 dev/scripts/native-sound-reset.py PORT HOST_OUTPUT --container NAME`.
It uses public play/stop requests, keeps ordinary gameplay paused briefly through
the console to isolate the selected installed looping effect, and records real
Pulse monitor PCM before/after cancellation and subsequent playback. It checks
bounded game-clock progress before and after the isolated windows. PCM remains
local. `browser-sound-reset.js`, evaluated after `session-sample-graph.js` in a
capture-enabled installed browser, exposes `__cyrusSoundReset.sample(name,
present)`. Drive the real book/menu input between active, stopped and continued
observations. The probe waits past buffers already submitted at the boundary,
then checks 8192 frames of actual WebAudio output. Active windows wait for a new
nonzero submitted buffer, so a slow host is not mistaken for failed continuation.
The complete installed driver runs with
`node dev/scripts/test-browser-sound-reset-live.mjs CDP_URL PAGE_URL INSTALL_URL VOICE_LABEL [LOOP_EFFECT] [SCENE]`.
It uses real Escape input, records book readiness and game-clock continuation,
and verifies the executed browser artifacts against the sealed build record.
Use a secure origin or a loopback origin for AudioWorklet support. Its negative controls run with
`node dev/scripts/test-browser-sound-reset.mjs`. These checks do not establish
original output latency or physical-device parity.

The sample graph observer records setup failures in `__sessionSamples.error`
and leaves production buffer starts intact. An observer error rejects the capture;
it does not permit substituting submitted buffers for actual output. Run
`node --test dev/scripts/test-session-sample-graph.mjs` for missing-worklet,
synchronous/asynchronous setup and production-start failure controls.
`node dev/scripts/test-browser-sample-graph.mjs CDP_URL SECURE_OR_LOOPBACK_PAGE`
checks a generated tone against actual render quanta, then removes the observer's
worklet API and requires the real source's playback-end acknowledgement. It owns
and closes a disposable browser context; it does not need the install.

The graph tap arms on an explicit future sample boundary. An a-rate automated
sample-frame parameter with short, quantum-aligned epochs identifies each rendered
quantum; the worklet-global clock
is retained separately as `reportedFrame`. The automated clock rejects captures
at or beyond 2^24 sample frames, where float32 cannot represent every integer.
Missing or duplicate quanta, missing messages, malformed clock ramps and observer
failures reject the recording permanently. Setup before the armed boundary is
outside the recording. No missing samples are synthesized or discarded from the
armed interval. `test-browser-sample-epochs.mjs CDP_URL LOOPBACK_PAGE` compares
132 seconds of captured quanta with independently indexed offline output, including
an audible tone at 130 seconds and every epoch boundary.

`captureSampleGraph` in `dev/scripts/capture-sample-graph.mjs` freezes a requested
window before CDP readback and writes its exact frame/sequence/clock metadata
before admission. Rejected windows retain diagnostics and export no PCM. Admitted
windows export interleaved stereo float32 PCM without quantization. Both boundaries
are integer sample frames on render-quantum boundaries in the tap's audio context.
Use `captureSampleGraphAtEndpoint(cdp, output, observationExpression)` when the
endpoint is detected live. It clones the caller's source/clock observation and
freezes the delivered graph window in one browser task. Use the returned
`observation` for reconstruction; a later source snapshot can include playback
that occurred during screenshot or PCM readback. The observation expression must
be read-only and must not poll a stateful playback clock. This does not establish
the native admission time of a source started with a past requested timestamp.

The source observer connects an already-ready tap before native source activation.
A source can render while `start()` is still returning; attaching afterward can
miss its onset. `test-browser-source-admission.mjs CDP_URL LOOPBACK_PAGE RESULT_JSON`
exercises that race with a delayed native return and an exact generated waveform.

`sourceAdmissions` in graph receipts records the requested time and native-call
entry/return clocks in seconds. Its `frame` is a command lower bound for immediate
sources, not a measured first-consumption frame. `match()` rejects a command-only
model when an immediate source can overlap its window. An ended callback records
a conservative end-of-quantum retirement bound; it does not rewrite PCM. Actual
PCM export still requires every sample-frame/sequence/clock boundary to pass.

`test-browser-source-render-boundary.mjs CDP_URL LOOPBACK_PAGE RESULT_JSON` records
native source processing traces and generation-clock markers in an isolated
24-source control. It compares independently traced first processing with exact
PCM onset and preserves the trace beside its receipt. Concurrent new sources,
missing native processing, missing or wrong-thread generation clocks, and altered
waveforms reject attribution. The control observes both immediate commands and
future scheduling. It derives no production delay, gain or timing constant.



### Original sound-emission capture boundaries

`original-audio-capture.sh` retains an optional probe's stdout, stderr and
`hook-result.json` under the run's capture directory. A requested hook delay is
only a request. It is recorded as `hook_requested`, never as an observed
placement or sound emission. A nonzero probe exit fails the capture; an
unfinished probe is terminated when the recording ends.

Set `CYRUS_REQUIRE_HOOK_ACK=1` when admitting an emission comparison. The local
probe must print exactly one JSON object with `acknowledged: true` and a
`boundary` containing `event: "sound_emitted"`, monotonic `before` and `after`
timestamps, observed `game_seconds`, integer `frame`, `scene`, `owner` and
integer requested `effect`. The probe owns observing the real emission, rather
than polling later or echoing its requested delay. The recorder rejects missing,
failed, malformed, unfinished and post-recording acknowledgements, retaining
their diagnostics. These structural checks do not independently prove that a
probe observed the right engine event.

`CYRUS_CAPTURE_MIN_FRAME` selects the initial world-frame readiness threshold
(default 400). `CYRUS_CAPTURE_GAME_SECONDS` optionally stops a run after observed
game-clock progress; the positional duration remains a wall deadline and fails
when the requested progress is not reached. Slow movie decoding does not count
as world-clock progress. `CYRUS_STATE_TIMEOUT` bounds each reader call in wall
seconds (default 20); use a shorter bound for short diagnostic runs. Missing or
timed-out clock observations cannot satisfy the requested progress. Each launcher
invocation holds its lock for one run.

The timeline identifies DOSBox's WAV as `emulated_mixer`. Probe wall bounds and
game-clock observations remain separate from its PCM sample clock. File flush
sizes, requested delays and wall-time scaling do not establish exact emission
sample offsets or physical-device playback. Use an independently calibrated
output recording and matched emission boundaries for that comparison.

Run `python3 dev/scripts/test-capture-hook.py`,
`python3 dev/scripts/test-original-capture-hook.py` and
`python3 dev/scripts/test-original-startup-capture.py` for probe failure,
acknowledgement, game-clock cutoff and startup controls.

### Rustle continuity before timing or gain comparison

A real-time sink can contain emulator underruns while still producing exactly
one second of PCM per wall second. Before interpreting a book recording, check
its sample timeline against the exported installed effects:

```sh
python3 -m venv /absolute/host/analysis-env
/absolute/host/analysis-env/bin/pip install numpy==2.4.6 scipy==1.17.1
/absolute/host/analysis-env/bin/python dev/scripts/book-audio-continuity.py \
  /absolute/host/capture/audio.wav /absolute/host/continuity.json \
  --reference /absolute/host/config/cache/sounds/066.wav \
  --reference /absolute/host/config/cache/sounds/067.wav
/absolute/host/analysis-env/bin/python dev/scripts/test-book-audio-continuity.py
```

Reuse an existing analysis environment when available. The tool requires signed 16-bit PCM and resamples both
inputs to 8 kHz and locates independent 100 ms reference windows. By default,
anchors require correlation of at least 0.8; at least three must span 300 ms.
Their relative delay must vary by no more than 10 ms. These are configurable
analysis admission limits, not recovered engine constants. The report retains
individual anchor times, correlations, gains and input hashes. A discontinuity
or insufficient reference evidence exits unsuccessfully and withholds shared
onset/gain estimates. Output is never overwritten. Overlapping cues, noise or
resampling can prevent enough anchors; a failure alone does not diagnose a
production audio bug. Passing establishes timing consistency at the admitted
windows, not every sample between them.

For browser acceptance, run `dev/scripts/test-browser-book-audio.mjs` against a
private Chromium CDP endpoint and a sealed installed build:

```sh
CYRUS_BROWSER_BUILD_RECORD=/absolute/host/web/build-provenance.json \
CYRUS_ANALYSIS_PYTHON=/absolute/host/analysis-env/bin/python \
node dev/scripts/test-browser-book-audio.mjs \
  http://127.0.0.1:9228 http://localhost:36084/ \
  http://localhost:36084/original/ /absolute/host/066.wav \
  /absolute/host/new-capture
```

The install service must expose the player's install explicitly. The driver uses
public keyboard input to open Options and return to Main, waiting for acknowledged
selection and page readiness. It records stereo PCM from consecutive AudioWorklet
quanta, the input and ready epochs, executed application/worker hashes, and the
unchanged continuity analysis. Missing graph quanta, stale build artifacts,
readiness failure and reference discontinuity reject acceptance. The output
folder must be new so a retry cannot overwrite evidence. Run the continuity
failure controls with `dev/scripts/test-book-audio-continuity.py` in the same
analysis environment. This harness does not start or restart managed previews.
Append another installed WAV after the output path to require both cues, for
example effect 67 alongside effect 66. This also writes `normalized.json`.
The driver reads all references before connecting, retains exact copies and source
hashes in the evidence directory, and analyzes those copies. Runtime cache
invalidation cannot remove a reference after recording. Missing sources fail
before browser work; existing evidence is never overwritten. Run
`node dev/scripts/test-book-audio-references.mjs` for retention failure controls.

`book-audio-normalize.py` accepts the same monitor/output/reference paths plus
`--input-seconds`, the acknowledged input epoch measured from the recording's
PCM start. It refines the common 8 kHz analysis grid by sixteenfold polyphase
interpolation, giving 7.8125 microsecond alignment resolution. Integer-grid
alignment can underestimate fitted gain or reject an overlapping cue when the
true onset lies between samples. The original coarse result is retained beside
the refined anchors. Correlation 0.8, three independent windows spanning 300 ms,
and the 10 ms drift limit are unchanged. Overlap annotations show where an
admitted cue continues while another plays; they do not subtract another cue or
admit obscured windows. These are window-continuity observations, not proof of
every tail sample. Fitted gain describes the shared analysis band, not a
calibrated output device or a recovered engine volume constant. Compare measured
input-relative onsets separately from game-clock scheduling and rendering pace.

Use the same transition direction and selection, explicit input epoch and
bounded audio window on both platforms. Check every expected rustle reference
before comparing onset/gain. The browser driver retains same-epoch PCM/game-clock
samples in `clock-observations.json` and `capture.json`. With both references,
normalization reports enclosing observed game-clock bounds for each admitted
onset. It rejects missing brackets, duplicate PCM timestamps and clock resets;
it never interpolates game progress from wall time. Polling bounds do not
establish an exact cue-dispatch update. The clock observer reads the AudioContext
clock without scanning retained PCM buffers. Acknowledged page input and paced WAV duration
remain separate requirements. The capture tool retains readiness stderr in
`readiness.log` when the observer fails, so display startup failures can be
distinguished from a missing book page.

To make continuity part of the original launcher's exit status, append the host
reference WAV paths after its transition driver argument and set
`CYRUS_AUDIO_ANALYSIS_PYTHON` to the analysis environment's Python executable.
It writes `OUTPUT/continuity.json` and propagates continuity rejection while
preserving the paced recording and cleaning up its owned container. Omit
references only for capture diagnostics, not for timing/gain acceptance.


### Isolate monitor drain from emulator delivery

The book recorder requests 20 ms PulseAudio latency. Default recorder buffering
can leave a large unconsumed tail when SIGINT stops a short recording. Keep the
existing pacing and installed-waveform rejection limits; a longer or apparently
non-silent WAV does not establish continuous emulator output.

Inside a runtime already running `paced-audio.sh`, use the independent control:

```sh
python3 /host-mounted/scripts/paced-monitor-check.py /host-mounted/control
python3 /host-mounted/scripts/paced-monitor-check.py /host-mounted/rustle \
  --reference /host-mounted/installed-cache/sounds/066.wav
```

It creates a deterministic three-second stereo signal when no reference is
supplied, warms the null sink with a low-latency silent playback client, records
its monitor and plays the reference through PulseAudio. It retains admission,
playback, wall and sample durations, the exact reference hash and an explicit
PulseAudio playback volume. `--playback-volume 65536` is unity. The player PID
must appear in the retained sink-input snapshot while playback is active;
a pre-playback snapshot describes only the warmup stream and cannot establish
calibration gain. Missing or exited playback rejects the control. This measures
the PulseAudio capture segment only, not the emulator mixer or native application
volume, and must not calibrate a browser graph tap taken at a different boundary. Run `book-audio-continuity.py` on the host
against each generated monitor/reference pair to check the sample timeline.
`--recorder-latency-ms 0 --cold-sink` selects server defaults without warmup
as the idle-clock/delayed-drain control;
actual server latency constraints can make this control pass when another client
already keeps the sink active. Use a separate idle control runtime for isolation.
The warmup allows the idle null sink's existing two-second timer to expire.
All processes are bounded and evidence is never overwritten.

The finite calibration player prebuffers the complete reference. It retains
verbose negotiated buffer metrics and rejects a prebuffer smaller than the
reference, preventing shared-host client scheduling from inserting silence into
an otherwise paced monitor. The recorder still requests 20 ms latency. Pacing
compares wall/sample progress from the same admitted PCM boundary, retaining
launch admission delay separately. The 250 ms pacing and 10 ms waveform limits
are unchanged. `--streaming --producer-stall-ms 200` is a diagnostic failure
control for the old 20 ms producer path; the same interruption on the default
preloaded path must leave the waveform continuous. `--producer-stall-ms` only
interrupts the owned signal player, never the game or other processes.

For a same-rate signed16 unity calibration, run:

```sh
python3 dev/scripts/check-monitor-calibration.py CAPTURE REFERENCE.wav REPORT.json
python3 dev/scripts/test-monitor-calibration.py
```

The unity checker requires the captured reference hash, admitted pacing,
unchanged waveform continuity and unit fitted gain within FFT numerical
roundoff. It rejects altered gain even when continuity passes. Different sample
rates require a separate resampler calibration and are rejected. This strict
signal-path control is not a production volume tolerance.

This isolates server/recorder behavior from DOSBox. A clean direct reference and
a discontinuous emulator recording locate the remaining fault upstream of the
monitor. DOSBox mixer prebuffer, block size, emulated CPU load and host scheduling
must be evaluated separately. The continuity analyzer sums local window energy
directly and excludes sub-unit PCM energy, so floating-point residue in a silent
tail cannot displace an audible waveform anchor. Excessive prebuffer can prevent sound entirely
when the mixer drops queued samples before reaching its startup watermark.
Emulator tuning stays in explicit local capture configurations until an installed
acknowledged recording passes both waveform and clock checks. Do not change game
cue timing or gain to compensate for an emulator delivery fault.

```sh
python3 dev/scripts/test-paced-monitor-check.py
```

### Non-stalling gameplay media capture

The minimal `dev/docker/retro/Dockerfile.contact` runtime includes raster and
PulseAudio capture tools. Its entrypoint defaults to dummy audio and preserves
an explicit `SDL_AUDIODRIVER=pulseaudio`. Run the entrypoint through
`dev/docker/dev/paced-audio.sh` to create the real sink monitor before launch.
Mount a writable host `CYRUS_CONFIG_DIR` and an evidence directory, keeping the
install read-only. The entrypoint creates the configured directory and stores
its Xvfb framebuffer there, with `/config` as the fallback. Use an exclusive lock for the writable run.

Inside that runtime, `dev/scripts/gameplay-media-capture.py` records real monitor
PCM and screenshot windows without stepping or stopping gameplay:

```sh
python3 gameplay-media-capture.py /evidence/new-capture \
  --observe 'python3 /local/observe.py' \
  --trigger 'python3 /local/trigger.py' \
  --screenshot scrot --game-seconds 3 --timeout 90
```

The local observation adapter returns JSON with `ready: true`, `scene`, integer
`frame`, finite `game_seconds`, integer `elapsed_scale` in Q8 nominal ticks,
installed integer `normal_frame_rate`, `execution: "running"`, and selected
`player` state. `game_seconds` must equal the integer clock divided by 256 and
the rate. A passive reader also returns `elapsed_scale_end` from after its actor
read; an atomic boundary snapshot may omit that field. Read spans are retained,
not collapsed to an invented instant. Missing scales, inconsistent seconds and
reversed read spans reject acquisition before they can become success evidence.
The adapter must observe without stopping execution. The trigger adapter returns
`acknowledged: true` and a `boundary` captured when placement is applied. That
boundary carries the same actor/scene/frame/clock fields, integer `elapsed_scale`
in Q8 nominal ticks, installed `normal_frame_rate`, `event: "placement_applied"`
and `phase: "before_ordinary_update"`. Its `execution` may be `"stopped"` for a
single setup stop. Native `place_player` supplies this acknowledgement when its
clock and prepared world are available. Boolean-only acknowledgements and later
polling timestamps cannot establish the trigger origin. The clock at this boundary
counts completed movement updates. An adapter whose accumulator already includes
the forthcoming update must declare `clock_accounting: "includes_pending_update"`,
`pending_frame_scale`, `pending_accumulator_scale`, and `startup_remaining`.
The recorder subtracts that pending step, retains the raw acknowledgement and
normalized origin, and rejects startup or unequal accumulated/consumed scales.
Adapters already reporting completed updates use `"completed_updates"` (the default);
applying the subtraction twice changes the interval and is invalid. These rules
normalize clock phase, without changing the exact comparison tolerance.
Per-update debugger stops cannot establish natural cadence. The screenshot command receives its output path as
its last argument. Keep executable-specific adapters outside the tracked tree.

The recorder admits PCM and a baseline raster before the trigger, then captures
until the requested game-clock progress from that event boundary. The later
trigger observation remains diagnostic and cannot shift the origin. Each raster
retains both surrounding actor observations and wall/game-clock bounds, integer
`clock_scale`, installed `normal_frame_rate`, and signed `event_relative_scale`
bounds measured from the acknowledged placement. Pre-trigger baseline windows
retain negative offsets. A window is explicitly non-atomic. Gaps stay visible and are never admitted as exact
update coverage. Scene changes, clock reversal, stopped execution, unavailable
actors, silent audio and monitor pacing failure reject capture. Partial media,
adapter diagnostics and failure results survive rejection; retries require a new
evidence directory. These observations support natural-cadence investigation;
they do not replace the exact update oracle or establish pose/pixel/audio parity.

For original mixer-output attribution, `Dockerfile.capture` also builds
`sdl-mixer-capture.c`. Set `CYRUS_MIXER_CAPTURE_DIR` to a new, host-mounted evidence
directory before starting the original. The capture entrypoint loads the optional
tap with the presentation interposer. It supports one legacy SDL callback device
with stereo signed 16-bit little-endian output, at the actual device rate.
A bounded ring copies completed callback buffers; a worker writes `generated.raw`
and `generated.jsonl`. The callback never writes files or waits for the worker.
Overflow, unsupported formats, write failure and unavailable generation cursors
reject capture. The output directory cannot be reused.

The frozen presentation result then includes `pcm_generation`: completed `frames`,
callback `sequence`, generation `monotonic_ns`, and `rate`. Local screenshot
adapters pass that object into their boundary without substituting a recorder
file length. Use `--owned-frames --generated-pcm /evidence/mixer` to select this
source. The driver exports `audio.wav` between its first and last frozen callback
prefixes, without resampling, and retains event-relative game-clock windows.
It validates consecutive producer blocks, cursor identity, complete PCM,
non-silence and the existing 0.25-second pacing bound against generation time.
Recorder startup, filesystem latency and unobserved tail delivery do not define
that interval. The monitor mode keeps its original pacing rejection.

A completed callback prefix identifies generated output, rather than the physical
playback instant or a sample inside a callback. It does not establish exact
within-block game-event alignment or original/native parity. The paired
comparison still requires identical event-relative game-clock intervals.
`test-generated-pcm.py` checks discontinuities and generation/delivery separation;
`test-sdl-mixer-capture-runtime.py` exercises the real SDL callback with delayed
writing, overflow, unsupported format and missing-tap controls.

To admit two captures' clock windows before interpreting their pixels or PCM:

```sh
python3 dev/scripts/gameplay-media-capture.py --compare-captures /evidence/original /evidence/native
```

Every raster records a `pcm_frames` range of complete stereo sample frames
delivered to the recorder during its observation/screenshot bracket. The recorder
captures raw signed 16-bit stereo at 48 kHz in `audio.raw`, then wraps those bytes
in `audio.wav` without resampling. An incomplete final sample frame rejects
finalization. `pcm-windows.jsonl` retains adjacent observation brackets through
the recorder tail, including integer/event-relative clocks and file frame ranges.
Those ranges cover the complete successful recording and may overlap across an
observation's read span. `observations.jsonl` retains each observation, recorder
cursor bracket and monotonic epoch immediately, so a failed run keeps its progress.
`pcm_unobserved_tail_frames` reports delivered PCM after the last retained
observation if capture fails.

These sample ranges describe recorder delivery, which may lag mixer generation.
They do not assert a zero-latency correspondence with a game event. The output
keeps `exact_pcm_event_mapping: false`; event mapping must be established before
interpreting playback timing.

Native `schedule_input` observations
publish these fields before the input's update is consumed. The comparison
requires equal scene and event-relative game-clock intervals for every raster
and PCM window, using exact rational arithmetic. Missing PCM windows, gaps in
sample coverage and incomplete recording coverage reject admission. Missing scales and unequal intervals reject
admission. Different absolute world ages are allowed. Admission establishes only
matching clock windows. It does not establish equivalent world state, atomic
pose/pixel correspondence or exact PCM sample-to-event mapping inside a wall-clock
bracket. Those still require their own evidence.

Run the capture and explicit-audio failure controls without an install:

```sh
python3 dev/scripts/test-gameplay-media-capture.py
python3 dev/scripts/test-contact-entrypoint.py
```

For native capture, pass `--owned-frames` to `gameplay-media-capture.py` and use
a screenshot adapter that returns the full `capture_gameplay_frame` control
result. The driver uses the render extraction boundary returned with each PNG
instead of polling state around GPU readback. It records reply latency separately.
The adapter may pass the active stereo signed-16 recorder file as `pcm_path`.
Its cursor counts delivered frames at extraction, not samples generated by the
mixer at that game clock. This mode does not establish exact mixer alignment or
make polling-based original capture windows admissible. The paired interval gate
continues to reject unequal clock bounds without a tolerance.

The optional `dev/scripts/sdl-frame-capture.c` interposer captures an original
SDL2 software surface at `SDL_UpdateWindowSurface` or its rectangle variant.
`dev/docker/retro/Dockerfile.capture` builds an independent runtime with that
interposer. Set `LD_PRELOAD=/usr/local/lib/sdl-frame-capture.so` on DOSBox only,
and set `CYRUS_FRAME_CAPTURE_DIR` to a host-mounted request directory. This does
not start a preview. OpenGL output is unsupported by this capture path.

A local probe supplies named process-memory bindings to
`dev/scripts/sdl-frame-capture.py REQUEST_DIRECTORY NEW_OUTPUT BINDINGS_JSON`.
Each binding has an integer `address` and a `size` from 1 to 4096 bytes, with at
most sixteen bindings. A dependent binding may instead supply a `pointer` object
with `field` (a preceding binding), byte `offset`, little-endian `width` (4 or 8)
and signed `addend`, alongside its target `size`. The presentation owner resolves
that pointer from the frozen preceding bytes, then reads the target. This allows
selected-object attributes and animation records to share the pixel boundary
without following an earlier polled pointer. Null pointers, address overflow,
forward references and out-of-bounds pointer slots reject capture. The complete
chain is checked again after the surface copy. Layouts and animation labels come
from the caller's verified bindings and installed catalog; none are built into
the interposer. Bindings and raw captures remain local research files.
The caller must verify its process/selected-object binding before admission.
The producer reads the bindings, copies the surface, and verifies the bindings
again on the presentation thread. Changed or unreadable bindings fail capture.
The worker writes the frozen BMP and result asynchronously. PNG conversion reads
that BMP, never the live window. Results identify the presentation sequence and
thread separately from the game's ordinary update counter. The request directory
permits one caller and the producer retains at most one copied surface/worker.

Before scheduling a counterpart, select actual retained presentation receipts:

```sh
python3 dev/scripts/gameplay-media-capture.py --plan-presentations ORIGINAL_CAPTURE COUNTERPART_CAPTURE
```

The planner normalizes each acknowledged placement origin and compares exact
rational event-relative times. It returns shared point receipts with source
row/pixel references and absolute counterpart clocks. Extra observations do not
change the selected boundaries. Missing counterparts are listed explicitly;
unrepresentable clock values are never rounded. Duplicate clocks and polling
brackets cannot identify a presentation and are rejected. A plan with fewer
than two shared points cannot define a paired window. Planning neither compares
pixels nor admits PCM parity, and does not replace `--compare-captures` or its
pacing/continuity gates. A proposed absolute clock without an observed counterpart
is an acquisition requirement, not evidence that the counterpart presents there.

For an exact planned boundary, pass `--at-clock FIELD VALUE`, or supply
`at_clock={"field": FIELD, "value": VALUE}` to the Python `capture` function.
The field must bind an unsigned little-endian 32-bit monotonic clock. The
presentation owner leaves the request pending while that clock is below the
requested value. It captures only equality; a skipped value, reset followed by
expiry, or expired deadline produces no accepted raster. The deadline uses the
same host monotonic clock as the producer. This schedules acquisition without
stopping or changing simulation updates. The probe must derive the absolute
value from the acknowledged placement origin and requested event-relative scale;
it must not round a missing boundary or reuse a late completion.

The media driver's `--owned-frames` adapter may return this boundary with
`event=render_frame`, `phase=sdl_surface_present` and its `render_frame` sequence.
Repeated presentation sequences fail admission. An optional recorder cursor
counts delivered stereo signed-16 frames at presentation; it still does not map
samples to mixer generation. Exact original/native windows must match before
interpreting pixels or PCM as a gameplay comparison.

For bounded playback acquisition, pass `--progress FIELD TICKS --timeout SECONDS`.
This retains each owned presentation and writes `complete.json` only after the
bound playback clock advances by the requested ticks. Frozen clocks, reversed
clocks/sequences and exhausted budgets fail without a completion receipt. Raw
captures and incremental `progress.json` remain available after failure. Bind the
actual movie decoder ordinal for cinematics: the gameplay clock can remain
stationary during playback ([playback clock](https://github.com/michidk/redguard-preservation/pull/315)).
Never convert wall duration into a movie frame number.

With both SDL probes loaded, `--require-pcm` requires a committed generated-PCM
cursor at every presentation and rejects missing generation, backwards cursors
or a changed sample rate. The retained cursor is a callback-generation prefix,
not DAC position. Each result also retains the original SDL surface format,
channel masks, pitch and dimensions before conversion to RGB24. These describe
the capture boundary; upstream renderer quantization must be established
separately. Converting a surface to RGB24 does not prove that its source had
24-bit color precision.

Run the binding controls with `python3 dev/scripts/test-sdl-frame-capture.py`.
The runtime control compiles `test-sdl-frame-capture.c` against SDL2 and runs
`test-sdl-frame-capture-runtime.py FIXTURE_EXECUTABLE INTERPOSER_SO FAILED_BACKEND_SO`. It requires
real SDL2 and Pillow. It verifies capture through the public surface API, then
changes the clock and pixels before asynchronous delivery, and rejects unreadable
bindings, skipped scheduled clocks and expired requests. The scheduled positive
control ignores an earlier red frame and retains the green frame at the exact
requested clock. Without the interposer, the same caller produces no accepted capture.

Compile the failure backend from the same C fixture with
`-DFAIL_PRESENT_BACKEND -shared -fPIC`. Its rejected presentation call verifies
that a copied surface cannot acknowledge a failed backend presentation.


### Authored browser speech lifecycle

`dev/scripts/run-authored-speech-browser.py RUNTIME INSTALL FIXTURE OUTPUT`
runs the installed browser game in an isolated headless context without starting a
managed preview or invoking Cargo. Build and seal `cyrus-web` first through the
shared queue. `RUNTIME` must be a host directory under `/home/vibepod/workspaces`;
it retains the per-run lock, configuration mount and server log. After an image
reset the launcher restores `chrome-image.tar` there, or pulls the pinned image
when no archive exists. It never removes an unowned container.

Keep the fixture and all results local. Its JSON fields are `scene`,
`destination`, `placement` (`x`, `y`, `z`, `heading`, in public placement units)
and `minimum_lines`. Select two distinct installed worlds and an authored
conversation whose initial exchange reaches a topic menu; derive the placement
and expected line count from that install and existing original observations.
The driver uses ordinary Use and walk/run skip input, public scene admission,
script inspection, public session capture and actual WebAudio graph quanta.
New-world ambience remains valid: retirement checks compare old playback entities
and the interrupted source identity, rather than requiring global silence. It records the initial
exchange, interruption, world teardown and fresh speech ownership, with subtitle
screenshots at 640 by 480. Output directories cannot reuse prior run receipts.

The launcher relays the installed service to browser loopback. A bridged HTTP
URL can lack secure-context AudioWorklet support even when Chromium accepts an
insecure-origin launch flag; the driver rejects that setup before installation.
The graph tap and PCM checks are shared with the session/sample acceptance tools.
Already mixed buffers may drain after retirement. A software WebAudio graph is
not physical-speaker parity. The dialogue ownership assertions follow
preservation's [dialogue and teardown rules](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/sound.md).

Run `node --test dev/scripts/test-browser-authored-speech.mjs` for stale-owner,
retained-sample, stale-presentation, generation and insecure-origin failure controls.

Pass `--driver music` to the same launcher for installed world-entry CD acceptance.
Its local fixture contains `scene` (numeric installed identity), `alias` (the same
world's map stem) and `destination` (a map stem assigned a different CD track).
The driver holds real destination asset reads to separate request-time retirement
from world publication. It checks uninterrupted same-track playback, retirement
and actual graph silence while destination admission is held, then a fresh source
whose stereo graph matches its installed CD buffer after publication. A dormant
revisit must retire the destination source, admit fresh music and
retain that source on another same-track visit. Each graph window contains 8,192
consecutive frames, retained as interleaved stereo float32 little-endian `.f32le`
files beside the source/clock metadata. Retain failed-before receipts separately;
prior output directories cannot be reused. This tests software WebAudio output,
not physical-speaker parity.


Set `scenario` to `restore` in the local music fixture to exercise diagnostic CD
ownership instead of the world-entry transition comparison. The driver enters and
leaves installed F10 credits, saves stopped CD ownership through the public session
API, and verifies that loading it preserves silence. It then holds destination
reads, supersedes that request with the original world, releases the stale reads,
and checks fresh music plus continued ownership. A final active save/load must
retire the old output and admit the restored track. Each stage retains its PCM
and metadata separately. These diagnostic saves restore exact runtime ownership;
original-style book saves keep their existing world-selection policy.

### Installed cinematic generation and presentation capture

`node dev/scripts/capture-cinematic-boundaries.mjs CDP_URL EXPLICIT_INSTALL_MOVIE_URL
NEW_OUTPUT_DIRECTORY MIN_END_FRAME INSTALLED_AUDIO_RATE [SKIP_AT_FRAME]` records the production browser
path in a disposable browser context. Set `CYRUS_BROWSER_BUILD_RECORD` to the
sealed build receipt; executed main/worker artifacts are checked against it.
Use the movie export's audio rate and an
explicit install URL. The caller owns runtime recovery, the host-mounted config,
a per-run lock and container teardown. Capture ends after the requested movie
progress with nonzero audio in the final observation window; a silent tail cannot
prove waveform equivalence. This tool does not start a preview. An optional skip frame sends one public Escape
press before the later endpoint, exercising the installed forward-skip path.
Screencast delivery order is asynchronous PNG encoding completion. Chromium's
`monotonicTimestamp` is the captured frame's expected-display `reference_time` in
TimeTicks seconds, not a physical scanout acknowledgement. The presentation
receipt keeps every raw timestamp and arrival sequence, lists all arrival
reversals, and assigns a separate expected-display sequence. Missing clocks,
duplicate clock/identity pairs and altered image pixels still reject admission.
Do not use CDP session IDs as frame sequence numbers.

The observer retains the actual installed worker `.movie` metadata, including
MENU.INI subtitle intervals/text and REDSEL glyphs/palette, together with live
menu preferences. The independent image oracle applies vertical expansion,
scanlines and subtitle composition using those values. Its composition hash is
part of the receipt. Missing composition metadata cannot fall back to a bare
video oracle. Raw capture and composed reference pixels are never masked.

The capture freezes PCM and drains tracing before running the image oracle,
then releases the application page. Trace export uses bounded encoding rather
than one aggregate JSON string. Presentation acceptance requires `--align`
automatically. For offline captures, pass `--align` to
`cinematic-presented-frames.py` to require native trace boundaries for the
production movie-clock calls bracketing each
identified image. The receipt records both call intervals and picture-age
bounds. These measurements do not identify physical display or DAC latency and
must not be converted to a compensating playback delay.

Set `CYRUS_CINEMATIC_ACCEPTED_CANCELLATION` to a previously admitted capture
directory to reuse its exact cancellation phase and boundaries during a separate
presentation run. The driver revalidates its retirement and PCM, records their
hashes, and sends a public Escape without the diagnostic clock hold. This new
seek supplies presentation evidence only; it does not replace active-bank
cancellation coverage.

For skip acceptance set `CYRUS_CINEMATIC_PRESENTED_WINDOW` to an independent
`movie_window` export containing early images and the destination endpoint.
`FRAME_COUNT` and `FIRST_FRAME` can retain a small late window after decoding the
prefix. Combine reference windows without changing their movie frame numbers.
The endpoint requires an exact identified compositor image at or beyond the
requested frame; published status alone cannot finish the recording. Passing the
last independently exported frame without identification rejects the run. The
reference uses the captured subtitle/scanline presentation state; raw
decoded pixels cannot establish equivalence to an image with subtitle overlays.

The diagnostic launch arms both sample and source observers and waits for a
validated rendering quantum before invoking production playback. The real play
call establishes the playback origin; observer failure still calls it once but
invalidates capture. `trace-completion.json` must report no lost events, and
`trace-coverage.json` requires creation, explicit stop, acknowledged retirement,
seek dispatch and final graph boundaries. Missing evidence is rejected.
Retired diagnostic branches drain through an acknowledged rendering endpoint
and disconnect, preventing inactive taps from exhausting long-seek traces.
A late retirement acknowledgement cannot extend an already frozen endpoint.

The skip trigger waits for an actual source bank with connected rendering input
and remaining scheduled samples. A diagnostic-only audio-context suspension holds
those owners across public input transport. Trusted-input resume requests for
that context are deferred until the real seek has stopped the bank; other
contexts and production handlers are unchanged. This controlled hold is
cancellation coverage and must not be used as normal cue-cadence evidence.
Source taps use an independently automated clock branch and precede the mixed
tap, so suspension cannot strand a source observation behind an already captured
mixed quantum. Native endpoint acknowledgements are emitted once.

After public seek dispatch, destination animation is held while native stops
retire and each source port acknowledges its exact drain endpoint. The driver
persists `cancellation-phase.json`, `cancellation-trace.json`, native boundaries
and `cancellation-phase-result.json` before releasing destination decoding. A
failed phase stops acceptance immediately. These short phase receipts survive a
later runtime reset; an interrupted continuation remains unaccepted. Final
continuation still requires continuous PCM and independently presented frames.

A skip run additionally requires sources owned at actual seek dispatch, native
stop requests within that dispatch and acknowledged retirement before new PCM.
Obsolete source PCM at continuation rejects acceptance. An empty source bank
rejects queued-cancellation acceptance even if continuation PCM matches exactly;
`seeks` in `generation.json` retains the actual bank and context-clock bounds.

The observer records each source's actual rendering input and native first
processing, with shared sample-frame clocks. Initial unobserved sources and
retired sources lacking native ownership require an acknowledged retirement
before a mixed-PCM comparison. Each source port drains through the fixed endpoint
before its PCM is copied; the requested tail is never shortened. Missing quanta remain
errors. A source's first diagnostic input can follow its first native processing;
those boundaries are kept separate. `generation.json`, `boundaries.json` and
`mix-comparison.json` retain that evidence. A transparent wrapper retains actual
returned `movieTime` values and trace brackets without adding playback-clock polls.
Source receipts retain packet timeline positions and explicit playback durations.
Admission rejects an audio cursor beyond decoded video even when the returned
movie clock was clamped to hide that overshoot.

Compositor PNGs and their swap clocks are separate from published movie status.
Run `python dev/scripts/cinematic-presented-frames.py CAPTURE_DIRECTORY
INSTALLED_WINDOW_DIRECTORY RESULT_JSON` with Pillow available to identify them
against an installed `movie_window` export. Identification requires exact RGBA
bytes after the movie's vertical bilinear expansion. Identical pictures retain
all possible frame identities. Loading pictures or advancing status alone cannot
admit a cinematic comparison. These tools establish capture boundaries; complete
original/native/browser timing parity still requires matching runtime recordings.

### Original cinematic raster oracle

`dev/scripts/cinematic-original-raster.py DECODED_IMAGE STATE_JSON PRESENTATION`
compares the entire original 640×480 movie raster with an independently decoded
installed image. It requires Pillow and NumPy. The state file supplies the
observed texture padding colour and ordered `draws`. Each draw supplies its
`source` rectangle `[x,y,width,height]`, power-of-two `backing` dimensions,
exclusive clipped `destination` rectangle, integer-pixel setup `anchor`, and
signed 32.32 texture-coordinate pairs `start`, `dx` and `dy`. Obtain this state
from the renderer boundary independently of the image being tested. The tool
never fits gradients, moves anchors or masks mismatching pixels.

The software Voodoo profile wraps through padded texture backing, retains four
fractional filter bits, floors both bilinear stages and stores dithered RGB565
before expanding to the captured RGB surface. Supply the actual raster clipping
bounds. A capture-runtime clipping defect belongs in that explicit state, not
in cyrus's production renderer.

For subtitles, pass `--subtitles INSTALLED_MOVIE_METADATA` and supply `frame`,
`subtitles_enabled`, `subtitle_right` and the complete eight-bit
`subtitle_palette` in the state file. Capture the selected palette during glyph
drawing. A palette read after movie drawing has finished can already have been
restored to the movie palette. The oracle shares installed glyph layout with
the browser presentation observer, but applies integer source/destination
blending, framebuffer dither subtraction and RGB565 storage independently.
`--output IMAGE` retains the reconstructed image. Any changed pixel, including
the last pixel beyond the former decoded-prefix check, fails admission.

Run its synthetic failure controls with
`python3 dev/scripts/test-cinematic-original-raster.py` and the existing browser
composition controls with `python3 dev/scripts/test-cinematic-presented-frames.py`.

### Interrupted runtime probes

Bounded comparison drivers can use `dev/scripts/probe-checkpoint.py` to retain
readiness, diagnostic clock progress and acknowledged ordinary boundaries in a
host-mounted evidence directory. Hold a per-run lock while writing. Include the
scene, input schedule, source identities and exact target scale in the fixture
identity passed to `Run.start`; record each readiness observation before input.
Call `register_job` with a path on that mount to retain the probe's process identity.

After interruption, run `python3 dev/scripts/probe-checkpoint.py <receipt.json>
--pidfile <container-path-to-job-receipt>`. Exit 3 identifies a missing/stopped
container or lost probe process; Docker availability errors remain errors.
Attach to a live attempt rather than launching another. Restart an incomplete
lost attempt from its fixture with `restart=True`; its old receipt is preserved
and its new clock starts at zero. This does not restore a pose into a new world.
Reuse accepted acquisition receipts without rerunning their completed stage.

Boundary receipts reject update gaps, changed generations and inconsistent
consumed scales. Diagnostic polling cannot satisfy the target. Acquisition
acceptance requires the exact declared scale; gameplay comparison still needs
matched original/native inputs, placement, timing and outcome evidence.

For exact counterpart acquisition, `dev/scripts/probe-clock.py` proposes a common
integer-scale endpoint from contiguous measured original/native boundary arrays.
Measure a steady tail before arming input; pass the installed normal frame rate
and a bounded minimum duration in Q8 nominal ticks. The plan uses the least common
multiple of the observed steps, rather than assuming that a fixed update count
always consumes the same game time. A steady measurement proposes a candidate;
`validate` must still check every executed interval and the final endpoint.
A cadence change, update gap or skipped endpoint rejects the comparison. The tool
never changes frame scale or interpolates an unavailable boundary.

When measurements change during execution, `pair_observed` selects shared elapsed
scales only from the two complete acknowledged receipt sequences. Both sequences
must start at their declared zero-scale input origins. A required endpoint absent
from either sequence stays rejected; an earlier shared point establishes only
that shorter interval. Matching elapsed time alone does not prove matching poses,
platform phase or complete action continuation.

For changing cadence, use `probe-clock.py`'s `Admission` at the input-owning
boundary or the native `acquire_input` control command. Supply actual counterpart
endpoints rather than converting a steady measurement into a future update count.
`Admission.observe` consumes contiguous zero-origin receipts and chooses the first
actually shared endpoint at or beyond the requested duration. The native command
records every boundary while holding the key, then releases before input sampling
at an exact member of the supplied endpoint set. Endpoint exhaustion is a rejected
acquisition. Do not substitute the last earlier match for a skipped later endpoint.

A host checkpoint fixture may declare `target_scales` as a bounded, strictly
increasing set of positive counterpart endpoints instead of one `target_scale`.
Acceptance still requires an exact observed member and records
`accepted_target_scale`. Supply `next_scale=None` only for a terminal boundary
whose following step was not captured; another boundary cannot follow it.
This avoids inventing a future step in a completed acknowledgement.

### Retaining completed acceptance stages

`dev/scripts/acceptance-stage.py` retains command logs and source-bound completion
receipts in a host directory. Pass every source/configuration input with `--input`,
relevant environment names with `--environment`, optional retained artifacts with
`--output`, and the command last after `--command`. A stage is reusable only after
successful exit with unchanged inputs and intact log/artifact hashes. Interrupted
or failed attempts restart their setup; a retained PID or partial output is never
evidence of success. Keep these directories under ignored local state and include
toolchain configuration among the inputs. Run its controls with
`python3 dev/scripts/test-acceptance-stage.py`.

### Recovering native compilation after a reset

For a full native acceptance build, set `CYRUS_NATIVE_CHECKPOINT_DIR` to an ignored
host directory and run `dev/scripts/build-native-recovery.sh /host/path/cyrus`. The output
must be named `cyrus`; it can live directly in the checkpoint directory to avoid
keeping a second executable copy. The script requires hauler and preserves the
inherited Cargo target directory. Completed Cargo units are retained as they are
reported, using the same hashed unit recovery as browser builds. Restoration
never replaces existing shared compilation files; Cargo checks their freshness.
Cargo events spool to the host independently of unit compression. A successful
Cargo exit seals the final executable before waiting for the unit archive backlog,
so slow checkpoint compression cannot defer executable retention. The wrapper
still waits for the recorder and reports recorder failures.
The final executable retains its permissions and is reusable only with matching
source, compiler, Cargo and build environment. A failed/interrupted build cannot
publish a final executable checkpoint. Run the reset/recovery controls with
`python3 dev/scripts/test-native-build-recovery.py`.

`just run-native` uses this recovery path and mounts the retained executable
read-only. Its default is `magic/build-tools/native/cyrus`; set
`CYRUS_NATIVE_EXECUTABLE` consistently for launch and matched capture to change
it. Every successful build or restore writes adjacent capture provenance.
Capture verifies the current source and executable hash against the running
container, so an old executable cannot borrow a new build record.

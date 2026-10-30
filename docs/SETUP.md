# SETUP — running cyrus and the original

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
just run                 # splash, title, then the book menu
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
scene that polls it. Run native cyrus with a real-time audio sink: ALSA's `null` device drains faster than real time, which
shortens every voiced line (**Verified:** the START story from the last kill to the world change took
30 game seconds with it and 43 with a paced sink, the original's 41 to 45). In the dev container,
prefix the command with `paced-audio.sh`, which starts a PulseAudio null sink behind ALSA's default device.

`dev/scripts/world-pursuit.py --port 7117 DRINT EXTPALAC` exercises combat actors' lost-sight routing:
it places the selected player (`place_player`) next to the nearest living combat actor, then holds it
far away, and prints failed actors, a world-level script error and how many actors reached
lost-sight timing and routed pursuit (exit 1 on a failure). The `teleport` command moves only the
render transform, so it cannot be used for this. Run one world per process.

`dev/scripts/native-audit.py --scripts game-config/cache` lists the natives that the exported
installed scripts call and that no Rust source mentions (exit 1 if any): a forgotten handler shows
up there, though a mention does not prove the handler works.

In the browser build, `?capture=1` publishes `window.__cyrusCapture`; setting
`window.__cyrusInspectScripts = true` adds the first 64 script actors (the `scripts` command's
data). `dev/scripts/start-duel-web.js` evaluated in the page plays the same duel with DOM key
events (a software-rendered tab runs at about 2 frames per second, so its key holds are long).

`dev/scripts/original-duel.py` plays the same duel in the original under DOSBox-X with the same bot
policy (xdotool keys; positions, yaw and health read from the emulator's memory by a reader kept
under `magic/tools/` and archived in redguard-decomp, because it holds executable addresses). The
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

`dev/scripts/new-game-compare.py` loads START, waits for the idle player to die, chooses NEW in the
death book (INTRO.SMK plays first and is skipped with Enter; the install's parent must hold the CD
image) and compares the script globals, HUD and inventory with the first start. Measured: inventory,
compass, power-ups and flags are identical; only timing-driven values differ (frame counters, health
mid-fight, the retained death position, the script choice highlight).

Headless mode creates no camera or audio backend. Control state reports a null
flycam position; player input, preload, world export and protocol commands remain
available. Use a windowed run for flycam and screenshot checks.

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
follow camera remain unverified; use the independent debug flycam for inspection.
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
tags, either when pushed or dispatched from `main` with a `tag` input such as
`v0.2.0`. Dispatching from `main` saves the build caches where the next release
can restore them; caches saved by a tag-push run are visible only to that tag.

Run `Bump Version & Tag` on `main` with a version such as `0.2.0` to update the
shared workspace version and lockfile, commit, and atomically push the commit
and tag, then dispatch `Release` from `main` for that tag. It uses the built-in
workflow token and needs no repository secret. Repeating a matching version/tag
retries the release without another version commit. Alternatively, update the
version and lockfile locally and push the version commit and tag.

`Release` checks that every workspace package matches the tag and runs lint and
test gates in parallel while all platforms build. Both verification jobs reuse
the matching CI caches. Nothing is published unless every job succeeds.
Verification and native Linux builds remove preinstalled SDKs only when less than
12 GiB is free; browser builds still free space for their additional swap. The macOS
Intel build runs on an Apple Silicon runner. Browser release builds use two Cargo
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

Local browser builds use the `wasm-dev` profile, which inherits `wasm-release`.
Both use 16 codegen units and thin LTO. Releases trade a larger download for
shorter compilation compared with the former single-unit profile.
Set `CYRUS_WASM_PROFILE=wasm-release` to build with the release profile name.

Without a `scene` query, the page runs startup through the book menu. Native and
browser hosts compose this presentation through `cyrus-startup`; scene graphics
and listener poses remain in `cyrus-render`, and playback remains in `cyrus-audio`.
Native `--scene`, `--skip-startup`, `--new-game`, `--movie`, `--skip-movies`, `--water`, `--fog`, and `--sky` map directly
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
the Windows Glide splash; the full DOS overlay contains the older shield logo.
`--splash dos` plays that overlay animation from `glide2x_emu.ovl`; startup export
caches both variants when the overlay is present ([DIFFERENCES.md](DIFFERENCES.md)).
The [preservation contribution](https://github.com/michidk/redguard-preservation/blob/main/docs/formats/GLIDE-SPLASH-DLL.md)
documents both paths. Enter, Space, or Escape advances startup.
`--skip-startup` bypasses the splash while required menu preparation remains behind the title. `--new-game`,
`--movie INDEX` and `--load-slot SLOT` (native) skip the splash and drive the book once it is open, as the
entry's key press would; `--skip-movies` makes New Game, from the book too, enter its world without the
introduction movie. `--scene` loads a world directly and excludes all three. MCP `skip_startup` goes to the menu; `load_scene {name}` enters
a world from startup or the menu. Control `state` reports the current app state.

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
the same way, before the next world loads (implemented, not run live). Native Quit closes the book and exits. Browser Quit
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
Escape returns to the menu after a loading error. Full gameplay and story
initialization remain pending. Menu export writes `cache/menu/book.menu` and an rgpre-generated
status font, plus the installed effects bank under `cache/sounds/` for browser menu
playback without loading a world. Runtime redraws authored book frames and neighboring-page text during
startup camera entry, submenu turns, and the closing Quit sequence. Navigation waits for
motion to settle; MCP drivers must wait for `state.menu.ready` after each page
change. Timing uses the startup presentation cadence rather than original update
performance. Book/page
motion and navigation feedback play installed effects through the shared sound
path on native and browser builds. **Verified:** the original default book menu has no pointer
hover/click selection. Mouse/joystick rebinding remains unsupported. Lighting,
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
its target for a later eligible stage through the shared `cyrus-script` core.
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

Re-export worlds to regenerate `.environment` sidecars. F1 provides independent
Faithful/Modern buttons for Water, Fog, and Sky. All default to Faithful.

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
the view. F1's distance control selects Modern Fog; Original resets the distance
and returns Fog to Faithful. Water and Sky are unaffected.

**Verified:** Faithful water follows preservation's vertex selection, waveform,
bias, spatial phase, and BIOS-rate clock. Modern water keeps the same selected
vertices and installed texture, with smooth crossing waves, small texture
ripples, and a subtle view-dependent sheen. These are optional artistic effects,
not recovered engine behavior. Returning to Faithful restores the original
wave at the current clock tick. Terrain placement, base heights, and lighting
retain the existing export limitations.

## The original game

```sh
just retro               # DOSBox-X + Voodoo emulation in dev/docker/retro, VibePod preview "cyrus-retro"
```

The retro runner needs a writable copy of the GOG `app/` directory (the game
writes saves); `dev/scripts/run-retro.sh` makes one under `magic/retro-run/` and
copies DOSBox-X's `glide2x_emu.ovl` next to `RGFX.EXE` so the 3dfx build
renders through the emulated Voodoo. Sound is off in the container.
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
| `--log <FILTER>` | `RUST_LOG` | `info` | console filter, for example `warn` or `info,cyrus_game=debug` |
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

Press F1 and use **Flycam** to switch between inspection
and the player pose. Flycam starts disabled unless `--view` or `?view=` supplies
an inspection pose. WASD moves it, right mouse looks,
and Space/Shift moves up/down; its speed is adjustable in the same menu.
Changing fidelity does not toggle flycam. Turning it off preserves the player
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
sticky contextual failure. Automatic actor eligibility, simulation timing,
complete native handlers, progression and resumable saves remain unavailable.

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
and omitted values are zero. It accepts numeric literals only, not symbols; a missing marker reports a
placement error and keeps the player where it was. `item add,id` gives the selected player one item through
the session inventory, the same acquisition path as the `AddItem` native (**Inferred** from the console reference;
the original handler was not compared). The other `item` operations (`player`, `show`, `sub`, `drop`) report
that they are unsupported. Other catalog entries report unavailable owners.

Browser `savecheats` persists to localStorage `cyrus-cheats`. F1 Cheat files
imports and exports CHT using the shared codec. A valid explicit import can
recover malformed local state; failed imports and saves retain existing state.

**Verified:** `neversaydie` supplies the documented distance override during
automatic world updates and explicit script steps, unless `distance_override`
is supplied by the caller. Other gameplay effects report unsupported.
**Unknown:** the original gameplay effect of `oddjob`; its raw state still
supports toggling, assignment and persistence. These commands do not enable
unsupported gameplay systems.

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
[guarded bundle](https://github.com/michidk/redguard-decomp/tree/0d596e97c624388e0080ea7a1dd9077c82126495/research/supplements/2026-10-13-stage0-owned-preview/cold-fixtures) and the [fixture workflow](../dev/fixtures/README.md).
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

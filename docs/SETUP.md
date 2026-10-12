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

## Link the install

```sh
ln -s /path/to/GOG/extracted/app/Redguard redguard   # or: export REDGUARD_DIR=...
```

The link is gitignored. cyrus only reads from it.

## Run

```sh
just run                 # splash, title, then the book menu
just run --skip-startup   # skip splash, prepare menu assets behind the title
just run --scene START
just run --scene 27       # WORLD.INI ID selects the night island variant
cargo run -p cyrus -- --export-all-worlds # rebuild all worlds and exit
cargo run -p cyrus -- --export-startup    # export startup, menu, starting world, cinematics and HUD for the browser
just run-headless        # no window: preload + scene export + control server
just run-native          # in dev/docker/dev with a virtual display, no preview: control port 7117 for MCP and compare
```

Headless mode creates no camera or audio backend. Control state reports a null
flycam position; player input, preload, world export and protocol commands remain
available. Use a windowed run for flycam and screenshot checks.

Settings, caches and screenshots live in `cyrus-config/`.

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
player-entry error. Dynamic ground support, locomotion and the original follow
camera remain unsupported; use the independent debug flycam to inspect the world.

## CI artifacts

The `Build artifacts` workflow packages the native game for Linux x64,
Windows x64, macOS Intel, and macOS Apple Silicon, plus a browser bundle.
It also includes `cyrus-viewer` and its browser worker when the viewer crates
are present in the checked-out revision. The viewer currently lives on its
separate development branch; game revisions without it contain only the game.
Browser artifact builds compile all available applications and the viewer worker
in one Cargo invocation so they share dependency features and compiled libraries.

Full hosted artifact builds exceed the ten-minute per-commit budget. Test and
lint CI still runs on every push and pull request. Artifact builds run only for
version tags and manual dispatch. Actions artifacts expire after seven days.
Pushing a version tag matching `v*` builds its revision and creates
a draft GitHub release with all archives and `SHA256SUMS` after every platform
succeeds. Review the draft's assets and notes before publishing it; publishing
does not rebuild them. A manual dispatch with `release_tag` set to an existing
tag also attaches artifacts and creates a draft when no release exists.

Archives contain application code and crate usage instructions. They contain
no game files, exported previews, or local state. Native executables still need
the player's GOG install. Browser bundles need locally exported game assets;
the asset viewer needs privately served original files as described in its
crate README. Downloading an artifact does not establish runtime or fidelity
support on that platform.

## Browser build

```sh
rustup target add wasm32-unknown-unknown && cargo install wasm-bindgen-cli   # version from Cargo.lock
just run-web             # VibePod preview "cyrus-web"; ?scene=START picks a scene
```

Without a `scene` query, the page runs startup through the book menu.
Export startup first with `cargo run -p cyrus -- --export-startup`. The exporter
reads the installed `3DfxSpl2.dll`, `system/STARTUP2.GXA`, and the configured
`gui_low_font` from the install; all generated images stay in
`cyrus-config/cache/startup/`. Native windowed launch generates this cache when
needed. The splash DLL is selected from `Redguard`, with the sibling GOG `DOSBOX`
directory as a fallback. **Verified:** its metallic logo and orange swoosh are
the Windows Glide splash; the full DOS overlay contains the older shield logo.
The [preservation contribution](https://github.com/michidk/redguard-preservation/compare/main...fix/gog-splash-resource)
documents both paths. Re-export browser startup caches after updating. Enter, Space, or Escape advances startup.
`--skip-startup` bypasses the splash while required menu preparation remains behind the title. `--scene` loads
a world directly. MCP `skip_startup` goes to the menu; `load_scene {name}` enters
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

Changes persist to `cyrus-config/menu.json` on native builds and localStorage
under `cyrus-menu` in browsers. Installed defaults apply when no local state
exists. Malformed state disables edits without overwriting it; failed saves
leave the previous runtime values intact. The install stays read-only.
Saved Sound preferences apply to native effects, speech, and CD music; the book
menu plays its installed CD track. Cinematics consume subtitle and interlace preferences.

Movies browses the initially unlocked entries and installed previews. Native
playback reads the configured movie file or GOG `game.gog` beside the install
into memory and decodes it in-process; the native virtual-display runner mounts the CD image read-only. It decodes video and the first embedded audio
stream, uses the installed subtitle definitions and GUI font, and applies the
saved subtitle/interlace preferences. A key press advances to the next installed
skip point, or finishes after the last one. Playback returns to the same menu
selection. Native playback does not write movie data to `cache/`.
For browser playback, `--export-startup` also exports the browsable compressed
Smacker files, first-track WAV audio, subtitle definitions, and configured font
into `cyrus-config/cache/movies/`. The browser downloads the selected cinematic
on demand and decodes video in-process, using the same skip, subtitle, interlace,
and menu-return rules. Re-export startup and rebuild the browser after updating.
MCP `state.menu.movie` reports loading, filename, frame, and total frames. The
installed `force_movies` setting exposes all cinematic entries. Progression
unlocks remain unavailable. Native Quit closes the book and exits. Browser Quit
attempts to close the tab; when the browser refuses, the menu remains usable.
New Game enters the `WORLD.INI` starting world and marker. Load currently starts
the same fresh world; it does not read or overwrite saves. Save and Continue
remain grayed without a resumable world. World entry shows the installed
destination artwork and red percentage bar until required assets and dependencies
are ready. Escape returns to the menu after a loading error. Full gameplay and
story initialization remain pending. Menu export writes `cache/menu/book.menu` and an rgpre-generated
status font, plus the installed effects bank under `cache/sounds/` for browser menu
playback without loading a world. Runtime redraws authored book frames and neighboring-page text during
startup camera entry, submenu turns, and the closing Quit sequence. Navigation waits for
motion to settle; MCP drivers must wait for `state.menu.ready` after each page
change. Timing uses the startup presentation cadence rather than original update
performance. Re-export browser caches for the authored-model schema. Book/page
motion and navigation feedback play installed effects through the shared sound
path on native and browser builds. **Verified:** the original default book menu has no pointer
hover/click selection. Mouse/joystick rebinding remains unsupported. Lighting,
camera-plane clipping and runtime gameplay thumbnails remain unsupported.

With `?scene=...`, the page loads `cyrus-config/cache/<SCENE>.glb`. Run
`cargo run -p cyrus -- --export-all-worlds` to rebuild every WORLD.INI entry
and its map-stem alias. Use `?scene=27` for a specific world variant or
`?scene=START` for a map. Map stems select the lowest world ID using that map.
Native scene loads regenerate the export; browser-only sessions need the
export command after loading changes. The browser bootstrap downloads only the
WebAssembly engine. Startup/menu and world downloads occur inside the running
engine behind installed artwork. The title remains visible until the book is
ready; world-entry progress counts completed preparation and required asset groups,
without a minimum loading delay. The browser keeps the WebAssembly engine
and downloaded GLB files in Cache Storage, then checks their timestamp and size
before reusing them. Only locally exported assets are served; the install directory is not exposed.

### Animation data

Re-export worlds to generate `.animations` and `.environment` sidecars beside
each GLB. Native and browser scene loading retain animation groups, model
references, frame geometry, original animation section payloads, and environment
inputs. Re-export for animation schema 2 and source-mapped GLB geometry. Explicit
group playback accepts supplied simulation ticks and updates private model
meshes. Re-export again to retain script payloads and version-2 typed SOUP
program descriptors plus the independent version-2 `.scripts` sidecar with actor RASC origins. Explicit scripted playback accepts a caller-selected default,
executes authored PushAnimation tasks, and applies their mesh frames. End stores
its target for a later eligible stage through the shared `cyrus-script` core.
Control `state.scripts` reports cache readiness, program and unsupported counts,
and bounded unsupported examples. Loaded scripts do not imply live execution.
Automatic world activation, host timing,
interrupts, synchronization channels and other script operations remain unsupported. Missing animation models
are listed in the `.animations` asset's `unsupported` map.

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

Press F1 and use **Flycam (leave player in place)** to switch between inspection
and the player pose. Flycam starts enabled. WASD moves it, right mouse looks,
and Space/Shift moves up/down; its speed is adjustable in the same menu.
Changing fidelity does not toggle flycam. Turning it off preserves the player
position. Initial placement uses the installed player marker; locomotion and
the original follow camera remain unsupported.

Native player input reads `KEYS.INI` and its user overrides. The TCP/MCP `state`
response reports held `player_input` actions and the separate `flycam` position.
Direct browser scene launches load exported keyboard defaults and saved local
Controls overrides. Re-export worlds to include the keyboard defaults. Locomotion
remains unsupported.

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

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
just run --skip-startup   # skip splash/title to the book menu
just run --scene START
just run --scene 27       # WORLD.INI ID selects the night island variant
cargo run -p cyrus -- --export-all-worlds # rebuild all worlds and exit
cargo run -p cyrus -- --export-startup    # render startup and menu assets for the browser
just run-headless        # no window: preload + scene export + control server
just run-native          # in dev/docker/dev with a virtual display, no preview: control port 7117 for MCP and compare
```

Settings, caches and screenshots live in `cyrus-config/`.

### Player placement

Direct scene inspection uses available marker zero for initial player placement.
Native, headless, and browser loading consume the same marker metadata. Re-export
worlds after updating to populate exact marker coordinates in `.animations`
and installed movement/terrain inputs in `.player`.
Unavailable markers report a player-entry error. Ground settling, locomotion,
and the original follow camera remain unsupported; use the independent debug
flycam to inspect the world.

## Asset viewer

`just viewer --install /path/to/Redguard` launches the separate native asset
viewer. It requires an explicit install path or `REDGUARD_DIR`.
Build with `just viewer-web-build`, without an install. Run
`just run-viewer-web /path/to/Redguard` to serve original files at runtime.
The browser parses them in a worker and generates selected previews in memory.

The game and viewer share `target/web`, the loader, Cache Storage, and
`cyrus-config/cache`. The viewer server also serves an explicit read-only install.
See [cyrus-viewer](../crates/cyrus-viewer/README.md) for controls and limits.

## Browser build

```sh
rustup target add wasm32-unknown-unknown && cargo install wasm-bindgen-cli   # version from Cargo.lock
just run-web             # VibePod preview "cyrus-web"; ?scene=START picks a scene
```

Without a `scene` query, the page runs startup through the book menu.
Export startup first with `cargo run -p cyrus -- --export-startup`. The exporter
reads the sibling `DOSBOX/glide2x_emu.ovl`, `system/STARTUP2.GXA`, and the configured
`gui_low_font` from the install; all generated images stay in
`cyrus-config/cache/startup/`. Native windowed launch generates this cache when
needed. Keep the GOG `DOSBOX` directory next to `Redguard`, or the supplied
Glide overlay within `Redguard`. Enter, Space, or Escape advances startup.
`--skip-startup` bypasses splash/title presentation and export; menu export still runs. `--scene` loads
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
menu plays its installed CD track. Native cinematics consume subtitle and interlace preferences.

Movies browses the initially unlocked entries and installed previews. Native
playback reads the configured movie file or GOG `game.gog` beside the install
into memory and decodes it in-process; the native virtual-display runner mounts the CD image read-only. It decodes video and the first embedded audio
stream, uses the installed subtitle definitions and GUI font, and applies the
saved subtitle/interlace preferences. A key press advances to the next installed
skip point, or finishes after the last one. Playback returns to the same menu
selection. Nothing is written to `cache/`.
MCP `state.menu.movie` reports loading, filename, frame, and total frames. The
installed `force_movies` setting exposes all cinematic entries. Browser movie
playback and progression unlocks remain unavailable. Quit exits; New Game, Save, Load,
and Continue remain unavailable, with Save and Continue grayed without a
resumable world. Menu export writes `cache/menu/book.menu` and an rgpre-generated
status font. Runtime redraws authored book frames and neighboring-page text during
startup camera entry, submenu turns, and the closing Quit sequence. Navigation waits for
motion to settle; MCP drivers must wait for `state.menu.ready` after each page
change. Timing uses the startup presentation cadence rather than original update
performance. Re-export browser caches for the authored-model schema. Lighting,
camera-plane clipping, book sound cues, mouse selection, and runtime gameplay thumbnails
remain unsupported.

With `?scene=...`, the page loads `cyrus-config/cache/<SCENE>.glb`. Run
`cargo run -p cyrus -- --export-all-worlds` to rebuild every WORLD.INI entry
and its map-stem alias. Use `?scene=27` for a specific world variant or
`?scene=START` for a map. Map stems select the lowest world ID using that map.
Native scene loads regenerate the export; browser-only sessions need the
export command after loading changes. The browser keeps the WebAssembly engine
and downloaded GLB files in Cache Storage, then checks their timestamp and size
before reusing them. Nothing from the install itself is served.

### Animation data

Re-export worlds to generate `.animations` and `.environment` sidecars beside
each GLB. Native and browser scene loading retain animation groups, model
references, frame geometry, original animation section payloads, and environment
inputs. Re-export for animation schema 2 and source-mapped GLB geometry. Explicit
group playback accepts supplied simulation ticks and updates private model
meshes. Re-export again to retain script payloads and typed literal-task/End
program descriptors. Explicit scripted playback accepts a caller-selected default,
executes authored PushAnimation tasks, and applies their mesh frames. End stores
its target for a later eligible stage. Automatic world activation, host timing,
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
The browser currently supports inspection only; keyboard bindings are not yet
exported for it.

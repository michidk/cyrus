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
textures, and indexed FNT glyphs. Up/Down navigates eligible entries; Enter or
the configured B key in `KEYS.INI` activates them. Quit exits. Other actions
report that they are unavailable; Save and Continue are grayed because there
is no resumable world. Menu export writes `cyrus-config/cache/menu/`, including
selection images, a descriptor, and an rgpre-generated font for status text.
The menu currently displays settled geometry. Opening/closing motion, lighting,
menu audio, mouse selection, runtime gameplay thumbnails, and neighboring-page text remain unsupported.

With `?scene=...`, the page loads `cyrus-config/cache/<SCENE>.glb`. Run
`cargo run -p cyrus -- --export-all-worlds` to rebuild every WORLD.INI entry
and its map-stem alias. Use `?scene=27` for a specific world variant or
`?scene=START` for a map. Map stems select the lowest world ID using that map.
Native scene loads regenerate the export; browser-only sessions need the
export command after loading changes. The browser keeps downloaded GLB files in
Cache Storage and checks their timestamp and size before reusing them. Nothing
from the install itself is served.

### Animation data

Re-export worlds to generate `.animations` and `.environment` sidecars beside
each GLB. Native and browser scene loading retain animation groups, model
references, frame geometry, original animation section payloads, and environment
inputs. Re-export for animation schema 2 and source-mapped GLB geometry. Explicit
group playback accepts supplied simulation ticks and updates private model
meshes; automatic world activation, timing and script-triggered transitions
remain unsupported. Missing animation models
are listed in the `.animations` asset's `unsupported` map.

### Sky, fog, and water

Re-export worlds to regenerate `.environment` sidecars. Native and browser
builds show the BSI sky in Faithful and Modern graphics. **Verified:** sky
texture selection, palette, and static plane projection follow preservation's
sky documentation. Old sidecars lack the plane inputs and must be rebuilt.
Sky scrolling, tilt, and the sun remain unsupported. `sky_disable=1` in the
install's `SYSTEM.INI` suppresses the sky. **Verified:** uncovered pixels use
`world_fogrgb`, the Glide background-clear color. Re-export old sidecars to
replace legacy palette background colors and the black fallback.

Scene near/far clipping comes from the install. **Verified:** the shipped
configuration uses 7 and 3800 engine units, with a separate sky range. F1 has a
render-distance control and Original reset. Editing the distance selects Modern
graphics; Faithful always uses the installed world range. Both graphics modes
apply the original scene and sky fog tables without rescaling the fog curve.
Re-export `.environment` files to include clipping and fog inputs.

Water waves run in both graphics modes. Re-export worlds after this update;
old water sidecars have incompatible masks. **Verified:** vertex selection,
waveform, bias, and spatial phase follow preservation's water documentation.
A BIOS-rate clock advances from application startup and persists across scenes.
Its absolute phase is not synchronized with a DOS session. Terrain placement,
base heights, and lighting retain the existing export limitations.

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

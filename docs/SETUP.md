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
just run                 # window
just run -- --scene START
just run -- --scene 27    # WORLD.INI ID selects the night island variant
cargo run -p cyrus -- --export-all-worlds # rebuild all worlds and exit
just run-headless        # no window: preload + scene export + control server
just run-native          # in dev/docker/dev with a virtual display, no preview: control port 7117 for MCP and compare
```

Settings, caches and screenshots live in `cyrus-config/`.

## Browser build

```sh
rustup target add wasm32-unknown-unknown && cargo install wasm-bindgen-cli   # version from Cargo.lock
just run-web             # VibePod preview "cyrus-web"; ?scene=START picks a scene
```

The page loads `cyrus-config/cache/<SCENE>.glb`. Run
`cargo run -p cyrus -- --export-all-worlds` to rebuild every WORLD.INI entry
and its map-stem alias. Use `?scene=27` for a specific world variant or
`?scene=START` for a map. Map stems select the lowest world ID using that map.
Native scene loads regenerate the export; browser-only sessions need the
export command after loading changes. The browser keeps downloaded GLB files in
Cache Storage and checks their timestamp and size before reusing them. Nothing
from the install itself is served.

### Sky and water preview

Re-export worlds to generate their `.environment` sidecars, then open
`?scene=ISLAND&graphics=modern` for animated water and the world's sky image.
Native rendering uses the same implementation when graphics is set to Modern.
The water mask and wave parameters come from the install. Sky projection,
wave timing, and centering are approximate; Faithful graphics stays static.

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

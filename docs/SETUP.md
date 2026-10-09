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
just run-novnc           # in docker/dev with a virtual display, served via VibePod preview "cyrus"
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
export command after loading changes. Nothing from the install itself is served.

## The original game

```sh
just retro               # DOSBox-X + Voodoo emulation in docker/retro, VibePod preview "cyrus-retro"
```

The retro runner needs a writable copy of the GOG `app/` directory (the game
writes saves); `scripts/run-retro.sh` makes one under `magic/retro-run/` and
copies DOSBox-X's `glide2x_emu.ovl` next to `RGFX.EXE` so the 3dfx build
renders through the emulated Voodoo. Sound is off in the container.

## Driving the game from an agent

The game listens on `127.0.0.1:7117` (`--control`, `CYRUS_CONTROL_ADDR`, or
`settings.toml`). `crates/cyrus-mcp` wraps it as MCP tools; Claude Code picks
it up from `.mcp.json`, Codex from `.codex/config.toml` (trusted project).

```sh
printf '{"id":1,"cmd":{"type":"state"}}\n' | nc 127.0.0.1 7117
```

## VibePod notes

There is no root in a VibePod. Bevy's system headers were unpacked into
`~/.local/sysdeps` and `~/.cargo/config.toml` points `PKG_CONFIG_PATH` there.
Previews: `cyrus` (new game via noVNC), `cyrus-retro` (original).

# cyrus (binary)

The native game executable. It parses the command line, finds the read-only
install, assembles the Bevy app from the assets, world, game and presentation
plugins ([cyrus-startup](../cyrus-startup/README.md), cyrus-render, cyrus-audio)
and runs it. It owns native local state (settings, preferences, cheats, console,
save slots) through `cyrus-config`, and applies the TCP control commands that
`cyrus-control` frames. The browser counterpart is [cyrus-web](../cyrus-web/README.md).

## Usage

```sh
cargo run -p cyrus                              # splash, title, then the default start
cargo run -p cyrus -- --scene START             # load a world directly
cargo run -p cyrus -- --headless --scene 27     # no window: preload, export, control server
```

From a prebuilt archive, run `./cyrus --install /path/to/Redguard` (`cyrus.exe`
on Windows) from a writable directory. Without `--install` the game tries
`REDGUARD_DIR`, the remembered location, then default local/GOG folders, and
opens an install picker when none works ([setup](../../docs/SETUP.md#browser-build)
lists the folders). Launch options share their names with the browser's URL parameters:

| Flag | Meaning |
| --- | --- |
| `--install PATH` | Original install, read only |
| `--scene STEM\|ID` | Load a map stem or WORLD.INI ID directly |
| `--view=x,y,z,qx,qy,qz,qw` | One-shot inspection camera pose for the first world (unit quaternion) |
| `--skip-startup` | Skip the splash and open the book menu |
| `--new-game`, `--movie INDEX` | Start New Game, or play a Movies-page cinematic (zero-based MENU.INI index), from the open book |
| `--skip-movies` | New Game without its introduction movie |
| `--volume PERCENT` | Master volume 0–100 for this launch, not saved; use `--volume 0` in automated runs |
| `--mode`, `--fog`, `--sky`, `--music` | `faithful` or `modern`; `--mode` sets every area including modern lighting, per-area flags override it |
| `--ray-tracing on\|off` | Solari ray tracing under Modern lighting, in builds with `--features raytracing` |

Native-only flags:

| Flag | Meaning |
| --- | --- |
| `--headless` | No window or renderer |
| `--control ADDR` | Control server address (default `127.0.0.1:7117`, env `CYRUS_CONTROL_ADDR`) |
| `--load-slot SLOT`, `--list-slots` | Restore the newest revision of a save slot through the book; print slots as JSON |
| `--export-scene --scene NAME`, `--list-worlds` | Prepare one world and exit; print accepted world names |
| `--export-startup`, `--export-all-worlds` | Write startup or every world to the browser cache and exit |
| `--audit-worlds` | Headless audit of every WORLD.INI entry (see below) |
| `--log`, `--log-file`, `--log-dir` | Log filters and directory (`RUST_LOG`, `CYRUS_LOG_FILE`, `CYRUS_LOG_DIR`) |

`--new-game`, `--movie` and `--load-slot` imply `--skip-startup` and conflict
with `--scene` and with each other. Flags override `settings.toml` in the config
directory. Backtick opens the world picker, flycam and fidelity controls; F12
opens the cheat console in a world. See [Playing](../../docs/PLAYING.md) for controls.

## Modules

| Module | Responsibility |
| --- | --- |
| [main](src/main.rs) | Argument parsing, install selection, app assembly, default startup choice |
| [scene](src/scene.rs) | Background scene export; cache references commit only after preparation succeeds |
| [session](src/session.rs), [session/slots](src/session/slots.rs) | Off-live session load preparation; save slot files under `slots/` |
| [settings](src/settings.rs), [preferences](src/preferences.rs), [bindings](src/bindings.rs) | `settings.toml`, local preferences, saved key bindings |
| [cheats](src/cheats.rs), [console](src/console.rs) | `REDGUARD.CHT` and `REDGUARD.CPF` in the config directory, `CONSOLE.LOG` |
| [control](src/control.rs) | Applies control commands; `control/` holds frame capture, input scheduling and other diagnostics |
| [audit](src/audit.rs) | `--audit-worlds` |
| [debug](src/debug.rs), [subtitles](src/subtitles.rs), [rendering](src/rendering.rs) | World picker sources, speech-to-subtitle adapter, software-adapter renderer setup |

## Notes

- The install is never written. Local state lives where `cyrus-config` decides;
  local cheat and console files take precedence over the installed ones.
- Default startup reads `SYSTEM.INI [debug] final_version`, `MENU.INI [general]
  ignore_autosave` and the native autosave: a fresh final-version start plays
  the Introduction, a valid autosave opens the book.
- Save slots hold cyrus session documents. Original `SAVEGAME.SAV` and `.TSG`
  files are not load inputs ([#93](https://github.com/michidk/cyrus/issues/93)).
- Control commands and their wire contract, including frame and mixer capture
  (`CYRUS_MIXER_CAPTURE_DIR`), are documented in [cyrus-control](../cyrus-control/README.md).
- Headless assembly omits the camera and audio backend; rendering never depends
  on the audio backend.

## World audit

`cargo run -p cyrus -- --audit-worlds` loads every numeric WORLD.INI identity,
including palette variants, through the normal preload, scene export and script
preparation, and writes `report.json` (schema version 1, sorted, no timestamps)
and `report.txt` under `magic/artifacts/world-audit/`. Each diagnostic names its
world, source file, field, reason and count. Categories can describe the same
request, so do not add them up as independent defects; an absent failure does
not prove an unexercised operation works. A failed world is recorded, the rest
are attempted, and the command then exits unsuccessfully.

## Testing

```sh
cargo test -p cyrus
just verify cyrus
just run-native --fixture dev/fixtures/<world>.json   # matched viewpoint, see dev/fixtures/README.md
```

Tests that need the install skip without one and say so.

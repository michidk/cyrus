# cyrus (binary)

The game executable. Parses arguments, finds the install, assembles the Bevy
app from assets, world, game and presentation plugins, and runs it.

```sh
cargo run -p cyrus -- --scene START         # window
cargo run -p cyrus -- --headless --scene 27 # preload, scene export and control server only
cargo run -p cyrus -- --export-all-worlds   # write every WORLD.INI world to cyrus-config/cache
```

Without `--scene`, a windowed launch runs startup through the book menu.
`--scene` and headless launches retain direct scene loading.
`--skip-startup` bypasses the splash and waits behind the title for required
menu/HUD preparation. New Game and the temporary Load fallback export the
installed starting world in a worker and enter after its dependencies are ready.

Windowed launches show the shared install picker when their explicit, remembered,
or discovered game-files location is unusable. Continue validates and remembers
the location in `cyrus-config/install.json`, shared with the viewer. Selection,
background preparation and gameplay use the same window and process. Headless,
world-listing and export commands keep contextual command-line errors.

Flags: `--export-startup` exports startup, menu, starting world, movies and HUD
for the browser and exits; `--install PATH` (else `$REDGUARD_DIR`, then the remembered location, then default local/GOG folders), `--scene STEM|ID`,
`--headless`, `--skip-startup`, `--control ADDR` (control server, default `127.0.0.1:7117`).
Settings come from `cyrus-config/settings.toml`.
With a prebuilt archive, extract it and run `./cyrus --install /path/to/Redguard`
from a writable directory. On Windows the executable is `cyrus.exe`.
The Linux archive uses Ubuntu 24.04 system libraries.
In any windowed launch, press F1 to choose a world, set flycam speed, and
change runtime fidelity. The menu remains available across startup and world entry.

This crate owns native settings and preference storage, binding initialization,
scene preparation, debug world options and control-command application.
`cyrus-control::server` handles TCP framing. Headless assembly omits the camera
and audio backend; windowed assembly adds the shared presentation plugins.

Shared browser/CLI options use exactly the same names:

| CLI | Browser URL | Meaning |
| --- | --- | --- |
| `--install PATH` | `?install=URL` | Read-only original install, local directory or install-service URL |
| `--scene START` | `?scene=START` | Load a map stem or WORLD.INI ID directly |
| `--view=x,y,z,qx,qy,qz,qw` | `?view=x,y,z,qx,qy,qz,qw` | Initial scene-space inspection camera pose; requires `scene` and a unit quaternion |
| `--skip-startup` | `?skip-startup=true` | Bypass the splash |
| `--water modern` | `?water=modern` | Water fidelity mode |
| `--fog modern` | `?fog=modern` | Fog fidelity mode |
| `--sky modern` | `?sky=modern` | Sky fidelity mode |

Mode values are `faithful` or `modern`. Native flags override saved settings.
Native-only flags include headless/control and export commands. `--export-scene
--scene NAME` prepares one world and exits; `--list-worlds` prints accepted names
as JSON. The browser install service uses these existing asset paths without
starting a game window.

[cyrus-startup](../cyrus-startup/README.md) supplies splash, title, the book menu,
settings, cinematic playback and world-entry loading. Hosts add it alongside
cyrus-render and cyrus-audio.

Native cheat state loads after install selection. `cyrus-config/REDGUARD.CHT`
takes precedence over the installed `REDGUARD.CHT`; if both are absent, values
start at zero. A malformed or unreadable local file blocks cheat changes without
falling back to the install. TCP/MCP `cheat` supports toggles, raw values, `oracle`
and `savecheats`. Saving writes only the local file and preserves unnamed slots.
To reset, remove the local file; installed state is imported again if present.
For an explicit all-off local state, set the named values to zero and save.
Browser persistence and the original console UI are not implemented.

`cargo run -p cyrus -- --audit-worlds` runs without a window and writes
`magic/artifacts/world-audit/report.json` and `report.txt`. It enumerates numeric
WORLD.INI identities, including palette variants, and reuses preload, scene export
and the normal script preparation systems. Each diagnostic retains its world,
source file, field, reason and affected count. JSON schema version 1 uses sorted
worlds and diagnostics, without timestamps or timings, for diffs between runs.
Exports used by the audit remain in that artifact directory.

The report describes loading observations. It does not supply invented actor
eligibility or animation ticks, or execute speculative native calls. It separately
counts actors whose animations were not initialized. No observed failure does not
establish support for an unexercised operation. A world export failure is retained,
the remaining worlds are attempted, and the command exits unsuccessfully after
writing both reports.

For matched original/native/browser viewpoints, use
`just run-native --fixture dev/fixtures/<world>.json` and the
[fixture workflow](../../dev/fixtures/README.md). Captures stay under `magic/`.
The audit's script inventory is a static census. Runtime unsupported counts are
observed preparation/execution failures whose existing error says `unsupported`;
script decode failures and runtime failures have separate counts.
Placement counts include repeated missing rope links. Asset counts count failing
source references, not unique filenames. Preload diagnostics use a null world
because the coverage scan does not establish a world reference.

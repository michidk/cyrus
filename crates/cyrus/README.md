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
Menu requests start preparation in Loading, after the attempt record is reset.

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

Native document storage writes an exclusively created sibling candidate, syncs
its complete bytes, then publishes it with one rename. Failed publication leaves
the accepted destination intact and cleans up only that write's candidate.
Callers own document validation. This shared byte-storage boundary does not yet
provide gameplay saves or full restoration after a restart.

Shared browser/CLI options use exactly the same names:

| CLI | Browser URL | Meaning |
| --- | --- | --- |
| `--install PATH` | `?install=URL` | Read-only original install, local directory or install-service URL |
| `--scene START` | `?scene=START` | Load a map stem or WORLD.INI ID directly |
| `--view=x,y,z,qx,qy,qz,qw` | `?view=x,y,z,qx,qy,qz,qw` | First-world scene-space inspection camera pose; accepts menu entry and requires a unit quaternion |
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
takes precedence over the installed file; absent files start at zero, then
SYSTEM.INI replaces task, animation and node-marker defaults. Malformed local
state blocks commands without falling back to the install. `savecheats` writes
only the local file and preserves unnamed slots. Remove it to restore installed
state on the next launch. Startup defaults still apply.

F12 opens the console in a world; F12 or Escape closes it. Type a cheat name,
`name = integer`, `oracle`, or `savecheats`. Gameplay also accepts a typed cheat
name followed by Enter. TCP/MCP `cheat` retains its explicit raw-value API;
`console {open?, line?}` operates the shared editor and `state.console` reports it.
Unsupported commands and gameplay effects report their limits.

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
Placement counts include repeated missing rope links. Asset counts count failed
loader requests, including actor-name fallbacks for empty authored model fields.
They do not establish that an authored asset is missing, and are not counts of
unique filenames. A skipped placement and an asset failure can describe the same
request; do not add these categories together as independent defects. Preload
diagnostics use a null world because the coverage scan does not establish a
world reference.

Native `state.scene_ready` reports that the current rendered world has finished spawning. `InGame` alone does not establish that its asynchronous scene dependencies have loaded.

Console preferences restore from `cyrus-config/REDGUARD.CPF`, then the installed
file when local state is absent. `console save` writes only the local CPF;
`console write` exports retained output to `cyrus-config/CONSOLE.LOG`. Malformed
preferences remain visible in `state.console.preference_error` and block edits
and saves. `console show` reports settings and any load error.

The native host connects speech metadata to rendering through
`subtitles::SubtitleRequest` and selects the rendering camera for audio listener
updates. These adapters keep rendering independent of the audio backend.

`hold_texture_frame {world, image, frame}` holds a zero-based texture frame for
matched captures. `frame: null` resumes automatic requests from the selected
frame without replacing its deadline. `state.textures` reports loaded image
names, frame counts, selected frames and holds. The requested world must be
ready, and the image/frame must exist. World replacement clears holds. These
inspection controls do not change ordinary playback or establish cadence parity.

Native diagnostic `session_load {path}` reads a cyrus document and prepares its
installed current/visited worlds and service sources off-live. Poll
`session_load_result` until it reports `committed` or a contextual error.
Preparation alone is not success. Only one native load may be pending, and its
terminal result remains available across repeated polls. Acknowledge it with
`session_load_result {acknowledge: true}` before another request. Capture commands reject while
that load owns the boundary. These diagnostics do not enable menu Load or
establish original save eligibility. Installed rendered restoration and revisit
acceptance remain unfinished.

# cyrus (binary)

The native game executable. It parses the command line, finds the
install, assembles the Bevy app from the assets, world, game and presentation
plugins ([cyrus-startup](../cyrus-startup/README.md), cyrus-render, cyrus-audio)
and runs it. It owns native local state (settings, preferences, cheats, console,
save slots) through `cyrus-config`, and applies the TCP control commands that
`cyrus-control` frames. Presentation captures retain application state at render extraction, including
loading and menu transitions. Presentation receipts include the active world-entry overlay opacities in units of 1/256 at extraction, so fade steps can be paired without estimating them from screenshot brightness; unrelated UI backgrounds are excluded. Gameplay captures additionally require a ready world. Script reload preparation starts after the installed loading presentation enters Loading, retaining the requested marker and heading. The browser counterpart is [cyrus-web](../cyrus-web/README.md).

The native Load list also discovers original `SAVEGAME.NNN` slots under the
selected install's `SAVEGAME` directory through published rgpre parsing. These
entries are read-only. The native adapter retains the complete typed slot, including
visited worlds and journal, and prepares every interpreter, native service,
allocation and host binding off-live. The shared complete-session boundary
validates all resource owners before publishing the replacement together.
Unsupported state rejects the load; the partial-import operation remains disabled.
From the startup menu or a different world, the host first prepares the saved
world's presentation and pauses it in the Load book. This provides process and
presentation inputs, while every saved logical owner is replaced. Before that
preliminary entry, the host checkpoints the previous session or startup owners.
Rejection cancels pending preparation and restores the checkpoint, with slot/world
context retained in the error.
Original entries cannot overwrite or delete an installed save. Install-backed listing tests
use `REDGUARD_ORIGINAL_SAVE_INSTALL=/path/to/Redguard` and report a skip when absent.

The public adapter regression uses an existing replay-controlled native process:
`python3 dev/scripts/verify-original-slot.py --port 7117 --slot SLOT --world WORLD
--frame-scale MEASURED_Q8 --output magic/slot-acceptance`. It selects the original
slot through the book, captures complete admission state without advancing the
world, then admits one explicit measured update and retains both session documents.

## Usage

```sh
cargo run -p cyrus                              # splash, title, then the book menu
cargo run -p cyrus -- --scene START             # load a world directly
cargo run -p cyrus -- --headless --scene 27     # no window: shared gameplay and control server
```

From a prebuilt archive, run `./cyrus --install /path/to/Redguard` (`cyrus.exe`
on Windows) from a writable directory. Without `--install` the game tries
`REDGUARD_DIR`, the remembered location, then default local/GOG folders, and
opens an install picker when none works ([setup](../../docs/SETUP.md#browser-build)
lists the folders). Launch options share their names with the browser's URL parameters:

| Flag | Meaning |
| --- | --- |
| `--install PATH` | Original install directory |
| `--scene STEM\|ID` | Load a map stem or WORLD.INI ID directly |
| `--view=x,y,z,qx,qy,qz,qw` | One-shot inspection camera pose for the first world (unit quaternion) |
| `--skip-startup` | Skip the splash and open the book menu |
| `--new-game`, `--movie INDEX` | Start New Game, or play a Movies-page cinematic (zero-based MENU.INI index), from the open book |
| `--skip-movies` | Skip every movie except those opened from the Movies menu and the ending |
| `--volume PERCENT` | Master volume 0–100 for this launch, not saved; use `--volume 0` in automated runs |
| `--mode`, `--fog`, `--sky`, `--music` | `faithful` or `modern`; `--mode` sets every area including modern lighting, per-area flags override it |
| `--ray-tracing on\|off` | Solari ray tracing under Modern lighting, in builds with `--features raytracing` |

Native-only flags:

| Flag | Meaning |
| --- | --- |
| `--headless` | No window; shared scene, gameplay and cinematic assembly, with a scheduled app loop |
| `--headless-audio` | Use the real audio output device with `--headless` for paced mixer capture; ordinary headless runs consume sources silently |
| `--replay-step-micros N` | Positive diagnostic host-clock duration per app update; requires `--headless` |
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
| [cheats](src/cheats.rs), [console](src/console.rs) | `REDGUARD.CPF` in the config directory, `REDGUARD.CHT` in the game folder, `CONSOLE.LOG` |
| [control](src/control.rs) | Applies control commands; `control/` holds frame capture, input scheduling and other diagnostics |
| [audit](src/audit.rs) | `--audit-worlds` |
| [debug](src/debug.rs), [subtitles](src/subtitles.rs), [rendering](src/rendering.rs) | World picker sources, speech-to-subtitle adapter, software-adapter renderer setup |

## Notes

- Settings, saves, caches and logs live where `cyrus-config` decides. Native cheat persistence writes the installation's `REDGUARD.CHT`, shared with the original game; console preferences remain in the config directory.
- Default startup opens the book after the splash and title. Starting a world requires
  New Game, loading a save, or an explicit launch shortcut.
- Save slots hold cyrus session documents. Original `SAVEGAME.SAV` and `.TSG`
  files are not load inputs ([#93](https://github.com/michidk/cyrus/issues/93)).
- Control commands and their wire contract, including frame and mixer capture
  (`CYRUS_MIXER_CAPTURE_DIR`), are documented in [cyrus-control](../cyrus-control/README.md).
- `--headless --new-game --skip-movies` starts the installed New Game world without
  opening a window. Omit `--skip-movies` to decode and play the intro on its normal
  clock before world entry. Scripted control input follows the same sampling and
  ordinary-update path as windowed gameplay.
- Headless gameplay retains the shared scene and render plugins because they supply
  world instances, model admission and the camera view used by gameplay. Graphics
  backends are disabled, so no GPU or display server is required. Audio sources are
  consumed silently on the host clock, preserving speech completion and sink controls
  without an output device. Optional native mods remain disabled in this host.
- The public `headless_new_game` integration test starts the executable without
  display variables, checks the installed initial world, and checks ordinary-update
  and game-clock progress under held input. [Playthrough regression replay #322](https://github.com/michidk/cyrus/issues/322)
  tracks recorded chapter checkpoints.

## World audit

`cargo run -p cyrus -- --audit-worlds` loads every numeric WORLD.INI identity,
including palette variants, through the normal preload, scene export and script
preparation, and writes `report.json` (schema version 1, sorted, no timestamps)
and `report.txt` under `magic/artifacts/world-audit/`. Each diagnostic names its
world, source file, field, reason and count. Categories can describe the same
request, so do not add them up as independent defects; an absent failure does
not prove an unexercised operation works. A failed world is recorded, the rest
are attempted, and the command then exits unsuccessfully.

The control port's `trace_player` request applies placement and starts a bounded
logical trace in one command. It records the initial pose before simulation and
one snapshot after every completed ordinary update, without network polling gaps
or changes to simulation timing. See [the protocol](../cyrus-control/README.md).

## Testing

```sh
cargo test -p cyrus
just verify cyrus
just run-native --fixture dev/fixtures/<world>.json   # matched viewpoint, see dev/fixtures/README.md
```

Tests that need the install skip without one and say so.

The control `scripts` observation includes a read-only `flags` snapshot with ordered
installed definitions, current values and independent latches, plus `player_position_q8` from the selected
player’s effective map pose. Reading it runs no
script operations and does not change the world.

## Recorded replay chain

Run `REDGUARD_DIR=/path/to/Redguard CYRUS_CONFIG_DIR=/writable/state just replay-chain`
to start a fresh headless New Game, run the ordered chapter fixtures in
`dev/fixtures/chapters/`, then run every admitted `segments/*.replay.json` in a
separate process. Both directories use filename order. Supply fixture paths to run a selected sequence from
New Game. `just replay <chapter.replay.json> <label>` also starts from New Game. Later
chapters require their predecessors through `replay-chain`.
The runner retains a source-verified executable through the native recovery
build and cargo-hauler. `CYRUS_REPLAY_BINARY` selects an already
built executable. Each config directory has an exclusive run lock and retains
process logs and checkpoint observations. Default discovery shares one generated
asset cache across its sequential launches, avoiding one world export per segment.

Staged segments in `dev/fixtures/segments/` start from a session that a chapter or an
earlier segment captured with a `capture_session` step (the tavern segments resume
`tavern-entry`, captured at the end of chapter 002); `CYRUS_REPLAY_SESSIONS` selects the
session directory and `replay-chain.sh` sets it for one run.

Fixtures in `dev/fixtures/pending/` are written from installed script facts but not yet
run with a current binary. `replay-chain` does not run them and the nightly run ignores
them; script tests only validate their shape. Record new work under `pending/`,
using real keyboard inputs and bounded predicates. Check world, named flags,
inventory and position tolerance, plus zero script failures. Run the candidate
with the same predecessor/session and timing it will use in the chain. Admit it
by moving it to `chapters/` for an uninterrupted continuation, or `segments/`
for a fresh launch. Name it so its session producer sorts before its consumers.
Run `python3 -B dev/scripts/test-replay-chain.py` to validate admission and session
references, then run the default chain with the install before pushing.

Regenerate sessions by running the default chain with a new writable
`CYRUS_CONFIG_DIR`; it captures the chapter and segment staging points again.
For a selected segment, set `CYRUS_REPLAY_SESSIONS` to that run's `sessions/`
directory and supply the fixture path. If it reports a missing session, run its
producer first. Never commit generated `.session` files or reuse them across a
gameplay change without regenerating and checking their producer.

Use `just replay-chain --keep-running` to record the next chapter from the
passing session. Its control address is printed and written to
`CYRUS_CONFIG_DIR/chapters/control-address` for default discovery, or
`CYRUS_CONFIG_DIR/control-address` with explicit fixtures. The process and run lock remain owned by
the runner until the game quits.

A chapter document contains version `1`, `step_micros`, and `steps`. Timing is
an explicit replay input. `--headless --replay-step-micros N` supplies the same
positive host-clock duration to every app update, including audio playback,
through Bevy's manual time source. Normal launches use the real host clock.
All chapters in a chain must supply the same duration. `ReplayHold` freezes
ordinary gameplay between scheduled input boundaries; speech preparation also
holds replay progression until its resources are ready. Simulated BIOS ticks
come from this manual clock, so host load time cannot advance combat pacing.
Normal play retains its host clock and Faithful defaults. This controls replay
pacing; it does not claim a complete playthrough or original-game parity.

Each input step contains `input`, a `schedule_input` event list with offsets in
ordinary updates, balanced key presses/releases, and an initial offset of zero.
A step ending in a world transition also declares `transition_to`. Losing the
old world's input boundary is accepted only after that target world becomes
ready and advances ordinary updates. Remaining input steps in that chapter are
then skipped and its checkpoints still run. Other schedule errors fail the run.

A bounded `repeat` contains keyboard input steps, a repetition `limit`, and an
`until` expectation for world and/or named flags. The opening repeats attacks
until both pirates are dead, matching the native recording strategy. An input
step may specify `when_health_below` for a recorded healing strategy. Failure to
reach the expected condition exhausts the bound and fails the replay. The final
inventory checkpoint checks the retained nonconsumable items; the initial
checkpoint also checks the starting healing-item count.

A `steer` step declares `world`, a three-coordinate `position_q8` target,
`tolerance` in Q8 units, and an input-iteration `limit` of at most 2000.
It reads position and heading between short keyboard schedules, turns with
Left/Right alone, and walks with Shift+Up. It stops within the horizontal
position tolerance or fails at the bound, on death, or on an unexpected world.
An optional `transition_to` accepts an ordinary-input boundary only after the
declared destination world is ready. An immediately following checkpoint uses the observation that reached the
waypoint, before later animation frames advance. Vertical position is checked
by that checkpoint. Use intermediate
waypoints around obstacles; steering does not find paths or change game state.
An optional `gait` selects `walk` (the default, Shift+Up) or `run` (Up alone).
Use walking on narrow approaches and running on open ground.

A `modal_input` step waits for the declared world’s scripted examine screen,
retries at most 32 balanced presses of the declared key across observed app
frames, and requires ordinary updates to resume after it closes. The viewer
can ignore input before it samples its initial key release. This handles the harbour letter without bypassing the item viewer.

Each checkpoint asserts a world, named quest/global flag values and latches,
gameplay globals, inventory counts, and effective player position in map Q8
units. Dictionary expectations select fields; the position uses Euclidean
distance and an explicit tolerance. Fixtures contain keyboard inputs, supplied
timing and expected observations, without game data or saves. The runner fails
if no fixtures exist. Hosted CI tests the comparator without an install; actual
route replay requires the player's install.


Combat `fight` steps use an explicit policy. `taps` attacks and samples health
between short idle waits. `busy-gate` faces and approaches the opponent, then
attacks only while the opponent is busy. `idle-tap` uses the same facing and
range checks but attacks only from the player's armed idle animation. Every
policy has a game-clock bound and checks player survival and opponent health.
Do not replace those predicates with a fixed host sleep.

Keep waits chunked: `await_control` advances one ordinary update and rechecks
its game-time/update bounds; ending waits advance five updates at a time before
checking the ending state. Input schedules use ordinary-update offsets, not BIOS
ticks or wall seconds. Split long idle periods into short schedules so control
responses stay inside the transport timeout and script failures are observed
promptly. Asynchronous resource waits poll without advancing ordinary gameplay.

Failures exit nonzero. A checkpoint error names the segment, zero-based step,
and field, for example `sword-walk-extpalac.replay.json:1:
flags/rock_1_down/value: expected 2, got 1`. Inspect its observation JSON and
`process.log` in the failing config directory; do not weaken the expectation
without checking the gameplay outcome. CI runs an admitted segment against
synthetic public observations, then deliberately corrupts one expected flag and
requires this failure. Run the same guard locally with
`python3 -B dev/scripts/test-replay-chain.py`; the install-backed chain remains
the gameplay check. Nightly comments link the artifact build and retained logs.

The partial Chapter 1 fixture continues harbour arrival through the mast and
onto the dock. Its two checkpoints assert world, quest flags, global lockout,
inventory counts and position within 4096 Q8 units. It waits for scripted arrival
control release before steering. The northern walking approach follows the
observed original-game bypass; a second segment continues through town to the Draggin Tale in world 15.
The tavern route stops at the entrance interaction range and presses Control
rather than walking through the doorway. Checkpoints also reject any reported
script failure.

The install-backed nightly runs in the maintainer's VibePod on `julian` through the control panel's
command scheduler at `0 3 * * *` (03:00 UTC), named `Cyrus nightly replay chain`.
Its command invokes `bash dev/scripts/nightly-replay.sh` with `REDGUARD_DIR` pointing
to the shared read-only install and `CYRUS_CONFIG_DIR` pointing to persistent
`magic/nightly-replay-chain` state. The schedule uses its own clean worktree at
`magic/nightly-replay-checkout`, tracking main independently of agent worktrees.
The wrapper refuses a dirty checkout before pulling, pulls main, and downloads
the Linux executable from that exact commit's push build in `pr-artifacts.yml`.
It fails clearly if the artifact is unavailable and never substitutes an older
binary or starts a local build. The binary and downloaded packages are temporary;
no executable or build cache is retained in the nightly logs. Completed runs post their result in
one comment of at most 15 lines to [#322](https://github.com/michidk/cyrus/issues/322),
including the commit, exit status and retained log path. View runs in the VibePod
control panel under Schedules. Only replay and nested chapter/segment process logs for the last two runs
remain under `nightly/run-*/`; runtime caches and checkpoint observations are removed
on exit, or on the next invocation after interruption. A retained unfinished-run marker reports
a pod-interrupted run on the next invocation before retrying from New Game.
The schedule registry persists under
`~/.config/vibepod/schedules.json`. The previous T3 agent schedule is disabled.
The wrapper snapshots its shell source before pulling, so updating that file
during a run cannot change the remaining commands of the active run.
GitHub CI validates the committed fixture schemas, comparator and nightly wrapper
without an install.

Replay mode holds a restored session before its first ordinary update. Its
diagnostic update counter restarts at zero, but the saved continuation is already
initialized; only fresh worlds receive the two startup updates. Recorded clock
steps therefore begin at the imported boundary rather than after hidden updates.

Recorded frames accept independent host elapsed nanoseconds for source playback
and presentation. The replay scheduler retains that interval through preparation
holds, and uses it only while admitting its recorded update. Movement scale and
BIOS measurement phase remain separate inputs.

Recorded silent-host audio can use `configure_digital_output` and
`service_digital_output` to deliver independently measured output-half callbacks.
Geometry must be selected before sample admission. Shared servicing consumes
source data and publishes completion without replacing script or sample state.
This acquisition path requires actual callback/admission ordering; elapsed frame
host time alone does not establish digital mixer phase.

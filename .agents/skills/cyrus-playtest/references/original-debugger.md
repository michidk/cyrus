# Original-game debugger

Use this when observations and published preservation facts leave a behavior
unresolved. Keep dumps, traces, tool builds, and research notes under `magic/`.
Publish engine facts in preservation before implementing them, following the
local research methodology's analyst and implementation sessions.

## Use the game console

Read the preservation [console reference](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/console.md)
for the complete command and subcommand catalog, and the
[cheat reference](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/cheats.md)
for activation conditions. The private archive's
[console research page](https://github.com/michidk/redguard-decomp/blob/main/research/console.md)
links the supporting handlers and runtime observations. Keep new traces and
open questions under `magic/` and archive them according to `AGENTS.md`.

**Verified:** the following console rules apply to the analyzed Glide build.
Recheck executable identity before applying them to another build.

1. Open the console with F12. Run `commands` or `?` to list the 82 registered
   command pairs; run `oracle` to list the 13 cheat names and raw state values.
   Enter cheats directly, without a `cheat` prefix.
2. Send one command at a time and inspect both its echoed text and result.
   Input is lowercased. A semicolon ends input; it does not chain commands.
   Interactive entry accepts long names and exact aliases. Programmatic
   entry accepts long names and cheats, but does not resolve aliases.
3. Follow each handler's argument separators. Examples are `load 1,0,0`,
   `item add,ID`, `setflag NAME,VALUE`, and `object attr attr_health = 100`.
   Replace placeholders with verified values before submitting.
4. Prefer decimal or hexadecimal numbers and verify the resulting value.
   Most numeric handlers also accept negative decimals, equates and SOUP
   flag names. An unknown symbol prints a diagnostic and substitutes `1`.
   Percent-prefixed binary digits parse as zero; quoted numeric input packs
   four bytes rather than evaluating a string or expression.
5. Run file-writing commands only in the writable runtime copy. Check the
   catalog for side effects first. `console write` saves retained output to
   `CONSOLE.LOG`; preserve it under `magic/` before another run overwrites it.

### Select the intended object

**Verified:** `object player` selects the player when present.
`object search flag2` searches script names without quotes and selects a
unique match. Inspect the resulting selection before editing attributes.

**Verified:** a direct numeric `object` argument is a memory pointer, not a
placed-object ID. Never substitute an object ID. `object master N` selects
a family master numbered 1 through 255; its invalid-master diagnostic can
display an unreliable number. See the private console page for the runtime
selection observations.

**Verified:** selection and monitoring are separate. `object monitor`
sets the selected object as the monitor target, or clears it when already
monitored. Rope and light selections are separate again. `object die`
enters interactive picking/killing; it does not simply kill the selection.

### Account for misleading help and side effects

**Verified:** use these rules when choosing a debugging command.

| Command | Debugging consequence |
|---|---|
| `get`, `set` | Their console-variable table is empty in this build. Use `getflag` / `setflag` for SOUP flags or `object attr` for attributes. |
| `poke` | Validates bytes and displays memory, but performs no write. |
| `object zap amount` | A reversed presence check rejects a supplied amount. |
| `node showinuse` | Help advertises an unsupported spelling. Use `node inuse`. |
| `show`, `object` | Help omits `show rain/free/map/list/activelist` and `object anim/size`. The slash-separated names here are alternatives. |
| `fxtexturelist`, `fxshowtexture` | The list command requires a start index; the show command without an index disables its preview. |
| `setflag pattern,value,0` | XORs metadata with 239; it does not clear the set bit and flips other metadata bits. |
| `break` | Enters the critical-drive-error path. Use DOSBox-X debugger breakpoints for tracing. |
| `restart video`, `restart xngine` | Restart subsystems, not the game session. |
| `walk` | Generates a PUT-node walk map, then enters the shutdown path. |
| `test`, `test3` | Write and reload live objects through `OBJECT.SAV`. |
| `test2` | Uses internal save/load slot zero around `GRID1.LOG` / `GRID2.LOG` snapshots. |

### Control cheat state explicitly

**Verified:** ordinary state cheats accept raw integers, not just zero, one
and two. For controlled tests, assign `name = 0` or `name = 1`, then check
`oracle`. Record the initial values and restore them afterwards. Do not assume
all zeroes is the normal baseline: `task` and `animation` control engine systems.

**Verified:** gameplay typing toggles zero to one and every nonzero value to
zero. Console toggling changes even values to one and odd values to zero.
Explicit assignment stores the supplied integer. Thus, starting at `2`, a
console toggle gives `1`, while an accepted gameplay toggle gives `0`.

**Verified:** `2` is not double strength. `nodemarker`, `task` and `magiccarpet`
require exactly one for their ordinary checks. `moonraker`, `drevil`, `drno`,
`goldfinger`, `neversaydie` and `yeahbaby` accept any nonzero value.
`animation` mixes both conditions. `oracle` and `savecheats` perform actions.
`neversaydie` makes SOUP `PlayerDistance()` return `9999999`.

**Unknown:** ODDJOB's gameplay effect. Its state handling, listing and
persistence are verified; bounded watchpoint runs without a consumer do not
prove it has no effect everywhere.

For gameplay cheat entry, allow release gaps as well as held presses and
verify that the game sampled the word. **Verified:** the input uses a release
latch. A missed activation does not test the toggle rule. Use `oracle` to
confirm the resulting state before drawing conclusions.

## Establish the instrument

- Check the executable identity against the analyzed version and inspect the
  running actor's attributes. Map defaults may have changed through scripts.
- Verify a working Debug menu and breakpoint command. Debug-related strings in
  an executable do not prove that the installed DOSBox-X supports debugging.
- Heavy-debug breakpoints were observed to work with the Normal CPU core when
  they did not stop under the Dynamic core. Check the core before concluding
  that the game never calls a function. Record the core and emulator build.
- Verify code and data relocation independently. Read the live player pointer
  again after loading a world; identify the actor at each breakpoint before
  interpreting arguments. Shared movement helpers also process NPCs.

## Capture one complete update

Start from a repeatable scene, settled position and heading. Record inputs,
actor attributes, animation state, startup counters and timing inputs. Capture
the relevant call arguments and result, then the final position after the
remaining animation and collision processing. Preserve fractional coordinates.
Repeat startup, steady movement and release as separate cases.

Use native video/state observations for timing. Pauses and a different emulator
core change timing; a debugger trace establishes call order and values. Neither
a rendered frame nor a host input duration proves how many updates ran.
Reject a helper-only explanation when it disagrees with the observation.

## Keep evidence reliable

- Prefer explicit register evaluation or memory dumps over a partially redrawn
  terminal register pane. Preserve carriage returns when rendering a curses
  transcript: decode raw bytes rather than applying newline translation.
  A redraw can repeat old evaluation output even after the log offset advances.
  Tag each register query with a unique echoed expression and match that tag;
  a fresh log chunk alone does not prove a fresh result.
- Verify suspicious numeric types against live values and load/conversion
  instructions. A plausible decompiled integer cast can misrepresent a stored
  float and reverse a collision decision.
- Archive each memory dump and CPU log before the next command overwrites the
  debugger's fixed output filename. Record the command, byte count, scene and
  input state alongside it.
- Verify released keys in the game's sampled input state. A release sent while
  the debugger is paused can leave the game's modifier held. If needed, press
  and release it again while running, then verify the sampled state is clear.
  Release held keys when ending a trace. Stop only the owned research preview
  and its processes; another agent may be using the ordinary original-game
  preview.
- Check persisted artifacts and live processes before restoring tools. Keep
  expensive debugger source, dependencies, object files and binaries on the
  workspace volume, not only in disposable Docker layers. Save resumable build
  commands and configuration with them.
- After an environment restart, verify which processes and build tickets still
  exist. A vanished ticket or a background wrapper exit is not a passed check.
  Keep completed check output with the tested revision and actual artifact path.

# Original-game debugger

Use this when observations and published preservation facts leave a behavior
unresolved. Keep dumps, traces, tool builds, and research notes under `magic/`.
Publish engine facts in preservation before implementing them, following the
local research methodology's analyst and implementation sessions.

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

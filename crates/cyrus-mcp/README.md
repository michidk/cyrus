# cyrus-mcp

MCP server (stdio) that exposes a running game to AI agents. Each tool opens
a fresh connection to the control port, so the server may start before the
game and survives game restarts.

Tools: `ping`, `state`, `screenshot` (returns the PNG), `teleport`, `input`,
`set_fidelity`, `skip_startup`, `load_scene`, `play_sound`, `play_speech`,
`stop_sounds`, `quit`.

`skip_startup` goes to the book menu. `load_scene {name}` enters a world
directly from startup or the menu. Check `state` before driving player input.

Registered for Claude Code in `.mcp.json` (pre-approved by
`.claude/settings.json`) and for Codex in `.codex/config.toml` (trusted
project). `CYRUS_CONTROL_ADDR` selects the game's control address.

`just smoke-mcp` runs a headless game and drives it through this server end
to end; run it after touching the control server, this crate or the player.

In `Menu`, `state.menu` reports the page, installed entries, current selection,
preferences, and Controls capture action. Wait for `ready` before sending menu
input and after each animated page change. Quit closes the book before exiting.
Drive Options and Movies with `input`; the same saved bindings and values
apply after a native restart. Native Movies playback uses the saved subtitle and
interlace preferences and reports progress in `state.menu.movie`. A new key press
advances to the next installed skip point, then returns to the menu. Saved Sound
preferences apply to native effects, speech, and CD music. Browser Movies shares
playback rules using locally exported cinematic assets; it has no control port.

`set_sky_scroll {direction, offset}` applies a captured original sky direction
from 0 through 2047 and two texture offsets in texels from -256 through 256.
Use it after loading the reference world. It synchronizes sky motion for
comparison; it does not synchronize other users of the original random stream.

`scripts {offset, limit}` returns bounded logical actor state. `step_script
{generation, index, stage, frame_scale, nominal_ticks}` selects `animation`,
`entry`, `main`, `interrupt`, or `tasks`.
`tasks` requires an explicit unsigned `frame_scale` with eight fractional bits.
For example, 256 supplies scale one; zero is accepted. Main and interrupt body
steps do not poll existing tasks. These inspection tools do not enable gameplay.

`bind_script_animation {generation, index, default_group, node?}` prepares an
actor once for this world generation. The caller supplies the established default
group. An optional exact scene node name transfers its animation presentation to
the logical actor. Without a node, stepping captures sound intents in the response's `captured_sounds` array instead of playing them. Once any actor is animation-bound,
all steps require explicit `nominal_ticks`, including zero, before execution.
These tools do not determine production eligibility or pacing.

Spatial inspection uses `step_script` stages `prepare_spatial`, `snapshot`,
`transform`, and `families`. Preparation is explicit and transactional. Snapshot
copies the selected actor's local angles; transform refreshes an unattached
matrix; families composes prepared attachments. These operations grant no actor
eligibility. Once spatial state is prepared, executable stages need both
`frame_scale` and `nominal_ticks`, including zero.

Player queries use the current `PlayerPose`, not the inspection camera.
`distance_override` optionally overrides runtime `neversaydie` state;
`fallback_position` supplies original Q8 coordinates when no player exists.
Missing required inputs fail. Attached queries read the last composed pose.
Unattached linked-position selection remains unsupported in the script host.

`cheat {name, value?}` applies a named toggle or explicit unsigned integer value.
Without a value, `oracle` lists state and `savecheats` writes the complete CHT file.
`state.cheats` reports readiness, load errors, raw values and each effect's support.
Unsupported effects retain their values but do not change gameplay. This is an
inspection interface. `console {open?, line?}` opens/closes or submits to the
shared F12 editor. `state.console` exposes its output and selection.

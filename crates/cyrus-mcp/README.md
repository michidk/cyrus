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
{generation, index, stage, frame_scale}` selects `main`, `interrupt`, or `tasks`.
`tasks` requires an explicit unsigned `frame_scale` with eight fractional bits.
For example, 256 supplies scale one; zero is accepted. Main and interrupt body
steps do not poll existing tasks. These inspection tools do not enable gameplay.

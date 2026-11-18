# cyrus-mcp

MCP server (stdio) that exposes a running game to AI agents. Each tool opens a
fresh [`cyrus-control`](../cyrus-control/README.md) connection, so the server
may start before the game and survives game restarts. It is a thin adapter:
command semantics live in `cyrus-control` and the game host, not here.

## Modules

| Module | Responsibility |
| --- | --- |
| [main](src/main.rs) | Tool definitions (`rmcp`), parameter schemas, one control `Command` per tool |

## Usage

`just setup` registers the server for both agents: Claude Code reads
`.mcp.json` and the generated `.claude/settings.json` enables it; Codex reads
the generated `.codex/config.toml` (trusted project). Both launch
`dev/scripts/mcp.sh`, which runs the built binary and builds it once when it is
missing. `CYRUS_CONTROL_ADDR` selects the game's control address (default
`127.0.0.1:7117`).

Tools:

| Group | Tools |
| --- | --- |
| Session | `ping`, `state`, `skip_startup`, `load_scene`, `quit` |
| Driving | `input`, `teleport`, `screenshot` (returns the PNG), `cheat`, `console` |
| Fidelity and rendering | `set_fidelity`, `set_day_night`, `set_modern_effect`, `set_ray_tracing`, `set_sky_scroll`, `hold_texture_frame` |
| Audio | `play_sound`, `play_speech`, `stop_sounds` |
| Script inspection | `scripts`, `bind_script_animation`, `step_script` |

Commands without a tool (captures, input schedules, save slots, render
inspection) are sent directly over the control port, as the scripts in
`dev/scripts/` do.

## Notes

- Check `state` before driving player input. In `Menu`, wait for
  `state.menu.ready` before input and after each animated page change.
- `skip_startup` stops at the book menu; `load_scene {name}` enters a world
  directly from startup or the menu.
- Script inspection tools step explicit stages only; they never enable
  automatic gameplay.
- When adding a tool, add the `Command` to `cyrus-control` first and keep the
  tool a one-call wrapper.

## Testing

`just smoke-mcp` builds `cyrus` and `cyrus-mcp`, runs a headless game on
`START`, and drives state, input, fidelity and quit through this server. It
needs an install and round-trips every area published in the game state fidelity registry. Run it after touching the control server, this crate or the
player. The crate has no unit tests (`test = false`).

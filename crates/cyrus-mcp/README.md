# cyrus-mcp

MCP server (stdio) that exposes a running game to AI agents. Each tool opens
a fresh connection to the control port, so the server may start before the
game and survives game restarts.

Tools: `ping`, `state`, `screenshot` (returns the PNG), `teleport`, `input`,
`set_fidelity`, `skip_startup`, `load_scene`, `quit`.

`skip_startup` goes to the book menu. `load_scene {name}` enters a world
directly from startup or the menu. Check `state` before driving player input.

Registered for Claude Code in `.mcp.json` (pre-approved by
`.claude/settings.json`) and for Codex in `.codex/config.toml` (trusted
project). `CYRUS_CONTROL_ADDR` selects the game's control address.

`just smoke-mcp` runs a headless game and drives it through this server end
to end; run it after touching the control server, this crate or the player.

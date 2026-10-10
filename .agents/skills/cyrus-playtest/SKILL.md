---
name: cyrus-playtest
description: Run, drive and compare the cyrus Bevy rebuild of Redguard against the original game. Use when asked to run the game, take screenshots, test a scene or controller, compare with the original (DOSBox-X), or verify a gameplay change.
---

# cyrus playtest

Read `AGENTS.md` and `docs/METHOD.md` first. Evidence is a screenshot or a
state dump, from both games where the change claims fidelity.

## Start

| What | Command | Where it shows |
|---|---|---|
| cyrus, window | `just run -- --scene ISLAND` | local display |
| cyrus, no display | `just run-native -- --scene ISLAND` | control port only; `screenshot` over MCP, or `just run-web` for the browser build |
| cyrus, loader only | `just run-headless` | log output |
| original game | `just retro` | VibePod preview `cyrus-retro` |

`vibepod-preview doctor <name>` prints the URL. Open `/vnc.html?autoconnect=1&resize=scale`.
In a shared workspace, stop only the processes and previews this session owns.

## Prove the loop works

`just smoke-mcp` starts the game headless and drives it through the MCP
server over stdio (tool list, state, a key press that moves the debug camera
while leaving the player stationary, a fidelity switch, quit). Run it after touching the control server, the MCP
server or the player. Scenes in the browser build accept a stem (`START`) or
a WORLD.INI index (`?scene=27`).

## Drive cyrus

Prefer the MCP tools (`cyrus` server: `ping`, `state`, `screenshot`,
`teleport`, `input`, `set_fidelity`, `load_scene`, `quit`). Without MCP, the
control port speaks one JSON object per line:

```sh
printf '{"id":1,"cmd":{"type":"screenshot","path":"cyrus-config/screenshots/a.png"}}\n' | nc 127.0.0.1 7117
printf '{"id":2,"cmd":{"type":"input","key":"ArrowUp","action":"press"}}\n' | nc 127.0.0.1 7117
```

Keys are Bevy `KeyCode` names. `tap` presses for one frame. Fidelity areas:
`player_controller`, `camera`, `graphics`; modes `faithful`, `modern`.

## Drive the original

The DOSBox-X window is a browser tab through noVNC: use the T3 `preview_*`
tools to press keys and take snapshots. DOSBox-X's menu offers capture;
debugger availability depends on the build. Keyboard scheme
is in the install's `KEYS.INI`. Save games live in the writable copy under
`magic/retro-run/Redguard/SAVEGAME/`.

For held input, use noVNC's `UI.rfb.sendKey(keysym, code, true)`, wait, then
send the matching release. A browser key tap may be too brief for the original
to sample. Host hold duration does not establish a simulated update count.
Console text can lose characters; send it slowly, handle modifiers explicitly,
and inspect the echoed command and result before treating it as successful.

For runtime inspection, read [Original-game debugger](references/original-debugger.md).

## Compare

Same scene, same position, same inputs. Save both screenshots under
`cyrus-config/screenshots/` (never commit them) and describe the difference
in the commit message: what matches, what does not, what was not checked.

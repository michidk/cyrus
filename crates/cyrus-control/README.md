# cyrus-control

The control protocol between a running game and anything that drives it: one
JSON object per line over TCP, default `127.0.0.1:7117` (`DEFAULT_ADDR`). It
defines the `Command` set, the `Request`/`Response` envelopes, a blocking
`Client` for tests and tools, and the native TCP listener. It depends only on
`serde` and `serde_json`; it has no Bevy dependency. The game host
(`crates/cyrus/src/control.rs` and `crates/cyrus/src/control/`) applies
commands on the game thread, and `cyrus-mcp` exposes them to agents.

## Modules

| Module | Responsibility |
| --- | --- |
| [lib](src/lib.rs) | `Command`, `Request`, `Response`, `Client`, capture boundaries, script stages |
| [server](src/server.rs) | `listen`: TCP framing; hands each `Pending` request to the host |

## Usage

```text
{"id":1,"cmd":{"type":"state"}}
{"id":1,"ok":true,"result":{"state":"InGame","scene":"START","scene_ready":true,"player":{...},"frame":120,...}}
{"id":2,"ok":false,"error":"..."}
```

The game listens on the address from `--control` or its settings.
`CYRUS_CONTROL_ADDR` selects the address for the MCP server and the scripts
under `dev/scripts/`. The doc comment on each `Command` variant is the
reference for its fields. Command groups:

| Group | Commands |
| --- | --- |
| Session | `ping`, `state`, `skip_startup`, `load_scene {name}`, `quit` |
| Input and camera | `input {key, action}` (Bevy `KeyCode` names; `press`, `release`, `tap`), `schedule_input`, `acquire_input`, `flycam`, `teleport`, `reset_camera` |
| Capture | `screenshot {path}`, `capture_presentation_frame {path}`, `capture_gameplay_frame {path, pcm_path?, boundary?}` |
| Fidelity and rendering | `set_fidelity {area, mode}`, `ray_tracing`, `ray_tracing_reference`, `set_day_night`, `set_modern_effect`, `set_sky_scroll`, `hold_texture_frame` |
| Audio and movies | `play_sound {effect, position?}`, `play_speech {label}`, `stop_sounds`, `queue_movie {movie}` |
| Gameplay setup | `place_player`, `trace_player`, `set_attribute`, `mark_dead`, `select_item`, `show_item`, `set_torch`, `cheat {name, value?}`, `console {open?, line?}`, `script_load_world` |
| Saves | `session_capture`, `session_load`, `session_slots` and their `*_result` polls |
| Script inspection | `scripts`, `script_summary`, `bind_script_animation`, `step_script` |
| Render inspection | `bound_light_image`, `rendered_actor`, `effect_geometry` |

`state` reports the app state (`Loading`, `Menu`, `InGame`), `scene_ready`, the
player, fidelity, the clock, and per-screen detail such as `menu`, `console`,
`cheats`, `sounds` (`{playing, paused}`), `textures` and `flycam` (position and
`[x,y,z,w]` rotation).

## Notes

- Check `state` before sending input. In `Menu`, wait for `state.menu` to
  report `ready` before input and after each animated page change.
- `teleport` moves only the presentation; `place_player` moves the logical
  player. Script-world commands need `scene_ready`. `scripts` and
  `script_summary` page through actors (default 32, at most 64 per page) with
  `offset`.
- Inspection commands (`step_script`, `bind_script_animation`, render
  inspection, `schedule_input`, `acquire_input`, `hold_texture_frame`) observe
  or step explicitly. They never enable gameplay, change the simulation clock
  or establish parity with the original on their own.
- Deferred commands (`schedule_input`, `acquire_input`, `capture_gameplay_frame`,
  `capture_presentation_frame`, `rendered_actor`, `effect_geometry`, `trace_player`,
  session operations) reply later or are
  polled; at most one of each may be pending.
- `capture_presentation_frame` freezes the application state and render-frame
  number at extraction, including loading and menu frames. It shares the bounded
  asynchronous readback queue with gameplay capture, without requiring a ready
  world or claiming a gameplay clock.
- `capture_gameplay_frame` freezes the clock, selected actor and presentation
  for the captured frame. With `CYRUS_MIXER_CAPTURE_DIR` set to an empty
  directory on Linux it also returns a mixer `pcm_generation` receipt (see
  [the cpal vendor note](../../dev/vendor/cpal/README.md)).
- `trace_player {x, y, z, heading?, updates}` places the logical player and replies
  after 1 through 255 ordinary updates with `observations`. Offset zero is captured
  atomically with placement; each later offset records a completed ordinary update,
  its generation, absolute update, elapsed Q8 nominal ticks, game seconds, installed
  normal rate and full selected-player snapshot. Spatial coordinates in that snapshot
  are Q8, not map units. Paused frames add no observations. Missing updates, world or
  player replacement, changed clock units and a 120-second deadline reject the trace
  and retain partial observations in the error. This observes the existing cadence;
  equal update ordinals do not establish equal elapsed time between games.
- The browser build has no control port.

## Testing

```sh
cargo test -p cyrus-control
just smoke-mcp   # headless game driven end to end through cyrus-mcp; needs an install
```

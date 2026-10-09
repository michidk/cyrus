# cyrus-game

The game as Bevy plugins, with no assumption about a window or renderer.

| Module | Role |
|---|---|
| `install` | Locate and validate the player's GOG install; read-only. |
| `loading` | Startup state: scans the install and parses every known file through `rgpre` behind a progress bar; the failures are the format-coverage report. |
| `scene` | Resolve a world from `WORLD.INI` (map, terrain, palette), export it to a GLB in `cyrus-config/cache/`, spawn it. Native only. |
| `player` | The player entity and its controllers; `frame_scene` places it on a freshly loaded scene. |
| `fidelity` | The switch between `Faithful` and `Modern` per `Area`; `in_mode` is the run condition. Defaults are faithful. |
| `settings` | `cyrus-config/settings.toml` and the cache directory. |
| `control` | TCP control server speaking `cyrus-control`; used by tests and the MCP server. Native only. |

`fidelity`, `player` and `settings` compile on wasm and are reused by
`cyrus-web`; everything that reads the install, spawns threads or opens
sockets is gated with `cfg(not(target_arch = "wasm32"))`.

Tests in `tests/` use only the public API; the install-backed ones skip when
no install is linked and say so.

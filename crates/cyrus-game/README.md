# cyrus-game

The game as Bevy plugins, with no assumption about a window or renderer.

| Module | Role |
|---|---|
| `install` | Locate and validate the player's GOG install; read-only. |
| `loading` | Startup state: scans the install and parses every known file through `rgpre` behind a progress bar; the failures are the format-coverage report. |
| `scene` | Resolve a world from `WORLD.INI` (map, terrain, palette), export it to a GLB in `cyrus-config/cache/`, spawn it. Native only. |
| `player` | The player entity and its controllers; `frame_scene` places it on a freshly loaded scene. |
| `fidelity` | The switch between `Faithful` and `Modern` per `Area`; `in_mode` is the run condition. Defaults are faithful. |
| `environment` | Static Glide BSI sky in both graphics modes; cached water inputs without playback. Loads exported `.environment` sidecars. |
| `sky` | Static Glide sky material and shader, projected behind scene geometry. |
| `environment_export` | Native extraction of per-world sky pixels, wave parameters, and water masks. |
| `settings` | `cyrus-config/settings.toml` and the cache directory. |
| `control` | TCP control server speaking `cyrus-control`; used by tests and the MCP server. Native only. |

`environment`, `fidelity`, `player` and `settings` compile on wasm and are reused by
`cyrus-web`; everything that reads the install, spawns threads or opens
sockets is gated with `cfg(not(target_arch = "wasm32"))`.

Tests in `tests/` use only the public API; the install-backed ones skip when
no install is linked and say so.

Re-export worlds after updating the sky renderer. Both native and browser builds
render the Glide BSI sky in Faithful graphics. **Verified:** source textures,
matching palettes, and the static plane projection follow preservation's
[sky documentation](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/sky.md).
`SYSTEM.INI`'s `sky_disable=1` suppresses the sky. Indoor worlds remove it.

Sky scrolling, tilting, the sun disc, and water motion remain unsupported.

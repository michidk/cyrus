# cyrus-game

The game as Bevy plugins, with no assumption about a window or renderer.

| Module | Role |
|---|---|
| `install` | Locate and validate the player's GOG install; read-only. |
| `loading` | Startup state: scans the install and parses every known file through `rgpre` behind a progress bar; the failures are the format-coverage report. |
| `scene` | Resolve a world from `WORLD.INI` (map, terrain, palette), export it to a GLB in `cyrus-config/cache/`, spawn it. Native only. |
| `player` | Held keyboard input from `KEYS.INI`, a stationary player entity, and a separate debug inspection camera. Locomotion and the original follow camera are unsupported. |
| `fidelity` | The switch between `Faithful` and `Modern` per `Area`; `in_mode` is the run condition. Defaults are faithful. |
| `environment` | Shared skybox and terrain-water animation, enabled by Modern graphics. Loads exported `.environment` sidecars. |
| `environment_export` | Native extraction of per-world sky pixels, wave parameters, and water masks. |
| `settings` | `cyrus-config/settings.toml` and the cache directory. |
| `control` | TCP control server speaking `cyrus-control`; used by tests and the MCP server. Native only. |

`environment`, `fidelity`, `player` and `settings` compile on wasm and are reused by
`cyrus-web`; everything that reads the install, spawns threads or opens
sockets is gated with `cfg(not(target_arch = "wasm32"))`.

Tests in `tests/` use only the public API; the install-backed ones skip when
no install is linked and say so.

For the browser, export worlds again and use `?scene=ISLAND&graphics=modern`
to enable sky and water. Water uses the world's wave parameters and four-corner
texture classification. Projection, wave speed conversion, and centering remain
approximations; Faithful graphics leaves the terrain static.

The debug flycam toggle moves only the view camera. Turning it off returns the
view to the stationary player pose; it does not enable gameplay. Faithful
keyboard actions preserve held and opposing inputs. Modern input samples WASD,
Space, and held Shift run. Camera-relative movement and the mouse camera remain
pending. The deterministic `player::turn` core is
tested against original heading observations, but gameplay does not yet drive it.

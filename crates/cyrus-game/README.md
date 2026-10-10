# cyrus-game

The game as Bevy plugins, with no assumption about a window or renderer.

| Module | Role |
|---|---|
| `install` | Locate and validate the player's GOG install; read-only. |
| `startup` | Shared splash/title and data-defined book menu; native exporter renders installed Glide geometry into a local image cache. |
| `loading` | Startup state: scans the install and parses every known file through `rgpre` behind a progress bar; the failures are the format-coverage report. |
| `scene` | Resolve a world from `WORLD.INI` (map, terrain, palette), export it to a GLB in `cyrus-config/cache/`, spawn it. Native only. |
| `player` | Held keyboard input from `KEYS.INI`, a stationary player entity, and a separate debug inspection camera. Locomotion and the original follow camera are unsupported. |
| `fidelity` | The switch between `Faithful` and `Modern` per `Area`; `in_mode` is the run condition. Defaults are faithful. |
| `environment` | Static Glide BSI sky and terrain waves in both graphics modes. Loads exported `.environment` sidecars. |
| `fog` | Original scene clipping, Glide fog tables, and a Modern debug distance override. |
| `sky` | Static Glide sky material and shader, projected behind scene geometry. |
| `water` | BIOS-rate wave displacement and shared terrain normals from preservation. |
| `animation` | Loads per-world `.animations` assets with actor metadata and model frames; no automatic playback. |
| `animation_export` | Retains RGM animation metadata and referenced 3D/3DC/ROB frames through rgpre. |
| `environment_export` | Native extraction of per-world sky pixels, wave parameters, and water masks. |
| `settings` | `cyrus-config/settings.toml` and the cache directory. |
| `control` | TCP control server speaking `cyrus-control`; used by tests and the MCP server. Native only. |

`startup` presentation, `animation`, `environment`, `fidelity`, `player` and `settings` compile on wasm and are reused by
`cyrus-web`; everything that reads the install, spawns threads or opens
sockets is gated with `cfg(not(target_arch = "wasm32"))`.

Tests in `tests/` use only the public API; the install-backed ones skip when
no install is linked and say so.

World exports include `.animations` sidecars containing actor animation groups,
RAAN model references, and decoded model frame positions and normals. Missing or
unsupported model references remain explicit in the asset's `unsupported` map.
Original RAHD, RAAN, RAGR and RANM payloads are retained too, including header
fields not exposed by the decoded metadata. They are not interpreted by cyrus.
Embedded ROB models are retained when matching archive payloads agree; conflicting
payloads remain unsupported rather than selecting an archive without evidence.
Native and browser scene loading retain this data. Playback, frame scheduling,
script activation, and mapping frames onto rendered meshes are not implemented.

Re-export worlds after updating the sky renderer. Both native and browser builds
render the Glide BSI sky in Faithful graphics. **Verified:** source textures,
matching palettes, and the static plane projection follow preservation's
[sky documentation](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/sky.md).
`SYSTEM.INI`'s `sky_disable=1` suppresses the sky. Indoor worlds remove it.
**Verified:** the Glide background uses `world_fogrgb`, including beyond the
finite sky plane. Re-export old sidecars to replace their legacy background
colors. The recovered background fact is [published for preservation review](https://github.com/michidk/redguard-preservation/compare/main...fix/glide-background-fill).

**Verified:** water uses preservation's [water documentation](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/water.md):
selected-corner mask, terrain-centered phase, quarter-shifted rotation table,
and half-amplitude bias. Playback uses real elapsed time at the BIOS timer rate, preserving its clock across scenes.
It deforms private copies of exported terrain meshes and recomputes shared
normals across materials. Re-export worlds to replace old water masks.

Absolute DOS clock phase is not synchronized. Terrain placement, base heights,
and lighting retain the preservation export's existing limitations. Sky
scrolling, tilting, and the sun disc remain unsupported.

The debug flycam toggle moves only the view camera. Turning it off returns the
view to the stationary player pose; it does not enable gameplay. Faithful
keyboard actions preserve held and opposing inputs. Modern input samples WASD,
Space, and held Shift run. Camera-relative movement and the mouse camera remain
pending. The deterministic `player::turn` and `player::walk` cores are
tested against original heading and animation-gated startup observations.
Walking supports actor step type 1 and advances startup during suppressed
translation. Gameplay does not yet drive these cores: animation eligibility,
collision, and simulation timing remain incomplete.

Scene rendering uses the installed near/far planes, including a nonzero
`world_back_plane` override. F1 exposes a render-distance override in game units;
editing it switches Graphics to Modern. Faithful uses the original range.
Both modes use the original fog tables and encoded RGB blend. The sky has its
own fog table and clipping range. Re-export environments after this change.
The fog curve follows preservation's [fog documentation](https://github.com/michidk/redguard-preservation/blob/fix/glide-background-fill/docs/engine/fog.md).
Hardware fog dithering and RGB565 quantization are not reproduced.

Default windowed startup plays the original authored 3dfx animation and decoded
640×480 title image, then stops at the book menu. Preload runs concurrently.
Timing is presentation policy; the original map-loading backend is not reproduced.
The title uses the configured FNT font and documented bottom-label positions.
The loading label says “Loading...” rather than reporting original map IDs.
Splash rasterization currently uses supersampling and the largest texture mip;
hardware edge coverage, mip selection, and shadow interpolation need a precise
paired comparison before claiming pixel-exact rendering.

The main menu derives its entries, layout, texture slots and action IDs from
`MENU.INI` through rgpre, following preservation's [book-menu rules](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/menu.md). It renders the settled installed book into selection
images under `cache/menu/`; fonts retain indexed glyph shading on those pages.
Navigation skips grayed/nonselectable/empty entries and wraps. Quit exits; other
actions remain explicitly unavailable. Save and Continue are disabled without a
resumable world. Opening/closing motion, lighting, menu audio, mouse selection,
gameplay thumbnails, and neighboring-page text remain unsupported.

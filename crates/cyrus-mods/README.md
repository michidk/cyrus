# cyrus-mods

Native-only mod support for the game binary, built on
[wasvy](https://github.com/wasvy-org/wasvy) (WebAssembly components on wasmtime).
It is a separate crate so wasmtime stays out of the browser build and out of the
crates that define the game; the browser build never depends on it.

A mod is a folder `game-config/mods/<id>/` with `mod.toml` (`name`,
`description`) and any of:

- `mod.wasm`: a component built against `wit/cyrus.wit` (the mod API) and the
  vendored `wit/deps/wasvy-ecs.wit`. It runs with access to the whole world.
- shader files listed as `[[shader]]` in `mod.toml` (`replaces` = asset path of the
  shader to swap, `file` = replacement in the mod folder). Any shader asset can be
  replaced. Each shader is one slot; if several active mods replace it, the first by
  id wins and a warning is logged.

- a `files/` folder mirroring the game install: its files replace or add to the
  install's for the game-data reader (`cyrus_assets::native::set_overlays`, fed from
  `file_overlays()` by the `cyrus` binary before anything scans the install).
  Case-insensitive match, first active mod by id wins, applied at startup only.

`ModsPlugin` lists the folders in a Mods window while the F11 menu is open and saves
activation to `game-config/mods.json`. Every half second it looks again: a folder
dropped into the mods folder appears in the list, a deleted one is stopped and
dropped, an edited `mod.toml` is reread, and an active mod whose `mod.wasm` or shaders
changed is reloaded. A mod is just its folder; copy it in or out while the game runs.
`register_source` must run before the asset plugin is added.

The API lives on one host entity with one component per hook (`src/api.rs`). The
hooks are generic: named scene values and four-value shader inputs
(cyrus-render's `hooks`, which lists the published and read names), the camera, the player's movement, animation and status,
collision probes, script native calls, health-loss observations, transient particles, settings, memory, a text overlay and input.
`PreUpdate` hands last frame's mod answers to cyrus-render's `ShaderInputs`,
`CameraTakeover` and cyrus-script's `CameraOrbit`, `MovementWish`,
`LocomotionClaimed` and `PlayerRequests`, and publishes this frame's state (including the
selected player's `Player` status), so answers apply one frame
later. Damage batches are read-only for all mods in the current frame; particle emissions
are consumed after Update and retain their submitting world, mod instance and loaded revision.
Particles use scene units, expire on game time and clear on disable, completed reload or world
replacement. Their appearance belongs to the mod. Hanabi simulates and renders directional GPU
bursts; `src/particles.rs` owns validation, instance properties and retirement.
Known combat sources are published alongside damage so mods can direct effects away
from the attacker. Missing sources remain explicit; they are not guessed from the camera.
See [`mods/README.md`](../../mods/README.md#damage-driven-particles) for the complete authoring example,
limits and coordinate conversion. Setting values come from `src/settings.rs`: mods declare them in
`mod.toml`, the Mods window edits them and `mod-settings.json` keeps them. A hook a mod stops answering returns to the original.

The generic camera hook also accepts relative yaw through `camera.turn` and reports
manual look through `camera.manual`. Relative yaw keeps normal camera collision and
player control; mouse/right-stick input wins over an automatic turn on the same frame.

Security: wasvy grants mods inherited stdio and network access. Only run mods
you trust.

```sh
cargo check -p cyrus-mods
just mods          # build mods/* and install them into game-config/mods
```

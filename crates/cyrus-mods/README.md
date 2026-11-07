# cyrus-mods

Native-only mod support for the game binary, built on
[wasvy](https://github.com/wasvy-org/wasvy) (WebAssembly components on wasmtime).
It is a separate crate so wasmtime stays out of the browser build and out of the
crates that define the game; the browser build never depends on it.

A mod is a folder `cyrus-config/mods/<id>/` with `mod.toml` (`name`,
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

`ModsPlugin` discovers the folders at startup, lists them in a Mods window
while the F1 menu is open, saves activation to `cyrus-config/mods.json`, and
reloads an active mod's files when they change (polled every half second).
`register_source` must run before the asset plugin is added.

The API lives on one host entity with `Water`, `Camera`, `Movement`, `Settings`
and `Input` components (`src/api.rs`). `PreUpdate` hands last frame's mod answers
to cyrus-render's `WaterWaves` and `CameraTakeover` and cyrus-script's
`MovementWish` and `LocomotionClaimed` and publishes this frame's state (including the
selected player's `Player` status), so answers apply one frame
later. Setting values come from `src/settings.rs`: mods declare them in
`mod.toml`, the Mods window edits them and `mod-settings.json` keeps them. A hook a mod stops answering returns to the original.

Security: wasvy grants mods inherited stdio and network access. Only run mods
you trust.

```sh
cargo check -p cyrus-mods
just mods          # build mods/* and install them into cyrus-config/mods
```

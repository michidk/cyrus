# cyrus-mods

Native-only mod support for the game binary, built on
[wasvy](https://github.com/wasvy-org/wasvy) (WebAssembly components on wasmtime).
It is a separate crate so wasmtime stays out of the browser build and out of the
crates that define the game; the browser build never depends on it.

A mod is a folder `cyrus-config/mods/<id>/` with `mod.toml` (`name`,
`description`) and any of:

- `mod.wasm`: a component built against `wit/cyrus.wit` (the mod API) and the
  vendored `wit/deps/wasvy-ecs.wit`. It runs with access to the whole world.
- `water_surface.wesl`: replaces cyrus-render's water shading library. One slot;
  if several active mods supply it, the first by id wins and a warning is logged.

`ModsPlugin` discovers the folders at startup, lists them in a Mods window
while the F1 menu is open, saves activation to `cyrus-config/mods.json`, and
reloads an active mod's files when they change (polled every half second).
`register_source` must run before the asset plugin is added.

The API lives on one host entity with `Water`, `Camera` and `Input` components
(`src/api.rs`). `PreUpdate` hands last frame's mod answers to cyrus-render's
`WaterWaves` and `CameraTakeover` and publishes this frame's state, so answers
apply one frame later. A hook a mod stops answering returns to the original.

Security: wasvy grants mods inherited stdio and network access. Only run mods
you trust.

```sh
cargo check -p cyrus-mods
just mods          # build mods/* and install them into cyrus-config/mods
```

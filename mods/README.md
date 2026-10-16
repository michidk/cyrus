# Bundled mods

Native-only WebAssembly mods shipped with cyrus. Each folder is its own Cargo
crate (outside the game workspace) built for `wasm32-wasip2`; `just mods` builds
them all and installs them into `cyrus-config/mods/<folder>/`. Activate them in
the Mods window that opens with F1. The browser build does not load mods.

| Mod | Hooks | Effect |
| --- | --- | --- |
| `modern-water` | water waves, water shader | Smooth crossing waves, texture ripples, sky sheen |
| `flycam` | camera, input | F3 toggles a free camera: WASD, Space/Left Shift, mouse look |

## Writing a mod

A mod folder holds `mod.toml` (`name`, `description`) plus `mod.wasm`, a
`water_surface.wesl`, or both. Copy one of these crates as a start.

**Logic** (`mod.wasm`). The API is `crates/cyrus-mods/wit/cyrus.wit`. Declare a
world that uses the resources you need, exports one function per system and
includes `wasvy:ecs/guest`; `setup` registers the systems and their queries:

```rust
fn setup(app: App) {
    let waves = System::new("waves");
    waves.add_query(&[QueryFor::Mut(Water::type_path())]);
    app.add_systems(&Schedule::Update, &[&waves]);
}
```

- `water`: read `positions()` (cache them per `generation()`), `parameters()`
  and `seconds()`; call `answer(generation, heights)` every frame.
- `camera`: `pose()` is the current view; `drive(pose)` moves it this frame
  and suppresses player input. Stop calling it to hand control back.
- `input`: `pressed` and `tapped` (Bevy `KeyCode` names), `mouse` (pixels),
  `delta` (seconds).

Query `water` and `camera` with `QueryFor::Mut` and `input` with
`QueryFor::Ref`: wasvy 0.0.9 panics the game when a method is called through
the other mutability.

Answers apply one frame after they are written. A hook a mod stops answering
returns to the original behaviour.

**Shading** (`water_surface.wesl`). Implement the two functions of
`crates/cyrus-render/src/water_surface.wesl` with the same signatures: a texture
coordinate offset and the final colour of water fragments. The file replaces
the built-in library, so a compile error shows up in the log and hides terrain
until it is fixed.

Rust mods use `wit-bindgen` through `wasvy_macros::guest_bindings!`, pinned to
the wasvy revision in `crates/cyrus-mods/Cargo.toml`. Other wasvy guest languages
(Python, Go) work against the same WIT.

# Bundled mods

Native-only WebAssembly mods shipped with cyrus. Each folder is its own Cargo
crate (outside the game workspace) built for `wasm32-wasip2`; `just mods` builds
them all and installs them into `cyrus-config/mods/<folder>/`. Activate them in
the Mods window that opens with F1. The browser build does not load mods.

| Mod | Hooks | Effect |
| --- | --- | --- |
| `modern-water` | water waves, water shader | Smooth crossing waves, texture ripples, sky sheen |
| `flycam` | camera, input | F3 toggles a free camera: WASD, Space/Left Shift, mouse look |
| `movement` | movement, player, settings, input | Moves relative to the camera with your key bindings or WASD; optionally owns the locomotion with its own heading and speed |

## Writing a mod

A mod folder holds `mod.toml` plus `mod.wasm`, a `water_surface.wesl`, or both.
Copy one of these crates as a start.

**Manifest** (`mod.toml`). `name` is required; `description`, `version` and
`author` are optional and shown in the Mods window. A mod can offer settings
with `[[setting]]` entries; the window draws them under the mod while it is
active and saves the values to `cyrus-config/mod-settings.json`:

```toml
[[setting]]
key = "speed"            # what the mod reads
label = "Fly speed"      # what the player sees (defaults to the key)
type = "number"          # number: default, min, max
default = 150
min = 20
max = 600

[[setting]]
key = "run_by_default"
type = "bool"            # bool: default
default = false

[[setting]]
key = "style"
type = "choice"          # choice: default, options
default = "smooth"
options = ["smooth", "choppy"]
```

A saved value that no longer fits its declaration (out of range, removed
option, changed type) falls back to the default; a bad declaration stops the
mod from being listed, with the error in the log.

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
- `movement`: `steer(x, y, jump)` every frame moves the player like a tilted
  left stick relative to the camera (x right, y forward; a length below 0.75
  walks, from 0.75 runs). While a mod steers, a quick sword key among WASD does
  not draw the sword.
- `movement.claim(heading, speed, run, jump)`: take over the player's ground
  locomotion for this frame (call it every frame). The original bindings read as
  released and the original turn, walk, run and jump selection do not run; the
  mod faces `heading` (2048 to a turn) and moves `speed` map units per nominal
  update along it, walking or running; `jump` starts the original jump. The engine
  still resolves ground, slopes and collision, and keeps death, dialogue and
  combat; it keeps control while the player is airborne, sliding or hanging, or a
  script moves it, and menus, the console and loading take control back.
  Stopping the claim returns the original controls.
- `player`: `position()` (map units), `heading()`, `view()` (the camera's
  heading, same units), `airborne()` and `speed(run)` (the player's own walk or
  run speed per nominal update, the base for a speed scale).
- `settings`: `flag(id, key)`, `number(id, key)` and `choice(id, key)` read the
  player's values; `id` is the mod's folder name. Read them every frame so a
  change in the window takes effect at once.
- `input`: `pressed` and `tapped` (Bevy `KeyCode` names), `mouse` (pixels),
  `delta` (seconds).

Query `water`, `camera` and `movement` with `QueryFor::Mut`, `input` and `settings`
with `QueryFor::Ref`: wasvy 0.0.11 panics the game when a method is called through
the other mutability.

Answers apply one frame after they are written. A hook a mod stops answering
returns to the original behaviour.

**Shading** (`water_surface.wesl`). Implement the two functions of
`crates/cyrus-render/src/water_surface.wesl` with the same signatures: a texture
coordinate offset and the final colour of water fragments. The file replaces
the built-in library, so a compile error shows up in the log and hides terrain
until it is fixed.

Rust mods use `wit-bindgen` through `wasvy_macros::guest_bindings!`, at the same version as
`crates/cyrus-mods/Cargo.toml`. Other wasvy guest languages
(Python, Go) work against the same WIT.

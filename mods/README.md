# Bundled mods

Native-only WebAssembly mods shipped with cyrus. Each folder is its own Cargo
crate (outside the game workspace) built for `wasm32-wasip2`; `just mods` builds
them all and installs them into `cyrus-config/mods/<folder>/`. Activate them in
the Mods window that opens with F1. The browser build does not load mods.

| Mod | Hooks | Effect |
| --- | --- | --- |
| `modern-water` | water waves, shader replacement | Smooth crossing waves, texture ripples, sky sheen |
| `flycam` | camera, input | F3 toggles a free camera: WASD, Space/Left Shift, mouse look |
| `movement` | movement, player, settings, input | Moves relative to the camera with your key bindings or WASD; optionally owns the locomotion with its own heading and speed |

## Writing a mod

A mod folder holds `mod.toml` plus any of `mod.wasm`, shader files and a `files/` folder.
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
- `movement.tune(jump_height, jump_speed)`: scale the claimed player's jump height
  (and the descent of a fall) and the forward speed of a walking or running jump
  while it stands on the ground; a jump in flight keeps its values. A claimed
  player steers a jump in the air through the heading it claims.
- `collision`: `cast(sx, sy, sz, ex, ey, ez)` queues a segment (map units, original
  axes) and `hit(i)` returns where the i-th segment of the last answered batch
  first met the static geometry the engine gathered near the player or the
  terrain: the point and the unit normal, or nothing. The engine answers while a
  mod claims locomotion; an answer arrives two frames after its cast.
- `natives`: `watch(name)` every frame names a script native (SOUP command, by its
  installed name) to observe; a frame later `count()`, `function(i)`, `actor(i)`
  and `arguments(i)` report the calls made to it, oldest first (the caller's script
  name and its integer arguments; the call that starts a task, not its
  continuation). Observation only: a call is never changed, and it keeps at most
  4096 calls per frame.
- `player`: `position()` (map units), `heading()`, `view()` (the camera's
  heading, same units), `airborne()`, `armed()` (a weapon is drawn or the combat stance is on: a
  claimed player then walks and runs in the fight animations and `speed` returns
  the fight speeds) and `speed(run)` (the player's own walk or run speed per
  nominal update, the base for a speed scale).
- `settings`: `flag(id, key)`, `number(id, key)` and `choice(id, key)` read the
  player's values; `id` is the mod's folder name. Read them every frame so a
  change in the window takes effect at once.
- `input`: `pressed` and `tapped` (Bevy `KeyCode` names), `mouse` (pixels),
  `delta` (seconds).

Query `water`, `camera`, `movement`, `collision` and `natives` with `QueryFor::Mut`, `input`, `settings` and `player`
with `QueryFor::Ref`: wasvy 0.0.11 panics the game when a method is called through
the other mutability.

Answers apply one frame after they are written. A hook a mod stops answering
returns to the original behaviour.

**Shading** (`[[shader]]`). Any shader asset can be replaced; list each one in
`mod.toml`:

```toml
[[shader]]
replaces = "embedded://cyrus_render/water_surface.wesl"   # asset path of the original
file = "water_surface.wesl"                                # in the mod folder
```

The replacement must keep the original's exported functions and signatures (see
the file under `crates/cyrus-render/src/`; the water library has a texture
coordinate offset and the final colour of water fragments). It replaces the
built-in source, so a compile error shows up in the log and can hide what the
shader draws until it is fixed. Each shader is one slot: the first active mod in
id order wins and a warning is logged. Turning the mod off restores the original.

**Game data** (`files/`). A `files/` folder inside the mod mirrors the game install:
`files/fxart/<name>` is read instead of the install's `fxart/<name>`, and a path the
install lacks is added. Names match ignoring case. The install is never written;
the replacement is read from the mod folder, so ship only data you made yourself.
If several active mods supply a path, the first by id wins. The files are read when
the game starts, so toggling such a mod needs a restart (the Mods window says so).
Turning the mod off restores the original, and cached assets rebuild by content.

Rust mods use `wit-bindgen` through `wasvy_macros::guest_bindings!`, at the same version as
`crates/cyrus-mods/Cargo.toml`. Other wasvy guest languages
(Python, Go) work against the same WIT.

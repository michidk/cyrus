# Bundled mods

Native-only WebAssembly mods shipped with cyrus. Each folder with a `mod.toml` is its own Cargo
crate (outside the game workspace) built for `wasm32-wasip2` against the shared SDK in `mods/sdk`; `just mods` builds
them all and installs them into `game-config/mods/<folder>/`. Activate them in
the Mods window that opens with F11. The browser build does not load mods.

| Mod | Hooks | Effect |
| --- | --- | --- |
| `blood` | damage, particles, settings | A brief spray of fading red particles on actual player or actor health loss |
| `modern-water` | scene, uniforms, shader replacement | Crossing waves, ripples, shore foam and wet sand. Water reflections are the base game's Modern Lighting, with or without this mod |
| `flycam` | camera, input, hud, settings | A free camera. Insert toggles a free camera (every F key is taken: F1 to F12 are the original shortcuts and the cyrus menus): WASD, Space/Left Shift, mouse look, a speed setting; the world streams around it, the player stays put and keys typed into the console are ignored; a corner line shows whether it is on |
| `movement` | movement, player, camera, settings, input | Third-person movement like current action games: the mouse orbits the camera, the keys move the player relative to it at full speed at once (`claim`), the animations only show it (`play`); a drawn weapon keeps the player facing the camera |
| `serpent` | script patch (manifest only, no code) | Restores the sea serpent in the Spine that was cut from the game: its installed script is complete but sits behind an endless `End`, which the mod lets fall through |

Modern movement optionally follows travel with the camera after a configurable delay.
Mouse or right-stick camera input takes priority; standing still, jumping and armed
strafing do not recenter. While a direction is held, automatic camera rotation keeps
the travel direction fixed in the world, so holding left does not create a circle.
Changing the held direction or looking manually chooses a new camera-relative direction.
The Mods window offers camera follow, delay (default 0.75 seconds) and follow speed
(default 2 per second). These are mod preferences, not recovered original behavior.

## Writing a mod

A mod is a self-contained folder: drop it into `game-config/mods/` while the game runs and it
appears in the Mods window; delete it and it goes. A mod folder holds `mod.toml` plus any of `mod.wasm`, shader files and a `files/` folder.
Copy one of these crates as a start.

**Manifest** (`mod.toml`). `name` is required; `description`, `version` and
`author` are optional and shown in the Mods window. A mod can offer settings
with `[[setting]]` entries; the window draws them under the mod while it is
active and saves the values to `game-config/mod-settings.json`:

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

**Logic** (`mod.wasm`). Rust mods use the SDK in `mods/sdk` (crate `cyrus-mod-sdk`): typed handles
for every hook below, `Vec3`/`Pose`/`Key` types, settings and memory scoped to the mod, and one line to export it.
Copy a bundled mod as a start (`Cargo.toml` depends on `cyrus-mod-sdk = { path = "../sdk" }`):

```rust
use cyrus_mod_sdk::{Cx, Key, Mod};

struct Hello;

impl Mod for Hello {
    const ID: &'static str = "hello"; // the folder name under game-config/mods
    fn frame(cx: &Cx) {
        if cx.input.down(Key::INSERT) {
            cx.hud.show("hello");
        }
    }
}

cyrus_mod_sdk::export_mod!(Hello);
```

`frame` runs every `Update` with every hook in `cx` (`cx.camera`, `cx.player`, `cx.movement`, `cx.input`,
`cx.scene`, ...). The sections below describe each hook in the host's own terms; the SDK keeps their
units and timing and swaps the raw numbers for types: a camera `Pose`, `Vec3` positions, `Option` where
the host answers an empty list, `Claim` for `movement.claim`, `cx.settings.flag("key")` without the mod id and
`cx.memory.load::<T>("name")` for any type that implements `Persist`. `cargo test` in a mod folder or
`mods/sdk` runs its pure logic on the host.

The raw WIT is `crates/cyrus-mods/wit/cyrus.wit`; wasvy 0.0.11 carries only primitives, `option` and `list`
across it, which is why the typed layer lives in the SDK. Other wasvy guest languages (Python, Go) work
against the WIT directly.

- `scene`: `values(key)` reads a value the game publishes by name, `seconds()`
  its clock. Published today: `"water-wave"` (amplitude, speed, frequency),
  `"water-cell"` (terrain grid cell size in scene units) and `"lighting-modern"`
  (1 while the Lighting area is Modern).
- `uniforms`: `set(key, values)` writes a four-value input the game's shaders read
  (call it every frame; stopping restores the original). Effects run in shaders: a mod
  replaces a shader library and sets its inputs here; the game never hands out its meshes. Read today:
  `"water-surface"` = `[effects, 0, amplitude, cell]`, which turns on the
  shore foam, wet sand and ripples and, with a positive `amplitude`, moves the
  waves in the terrain shader.

- `camera`: `pose()` is the current view; `drive(pose)` moves it this frame
  and suppresses player input. Stop calling it to hand control back.
  `orbit()` each frame lets the mouse and right stick turn the follow camera
  around the player as the Modern camera does, with the cursor captured,
  whatever moves the player.
  `turn(amount)` adds a yaw adjustment to the orbit camera (2048 units per turn,
  positive left), without taking over its pose or suppressing movement. Manual
  mouse/right-stick input wins over the adjustment. `manual()` reports recent
  manual camera input, so a mod can delay resuming automatic movement.
- `movement`: `steer(x, y, jump)` every frame moves the player like a tilted
  left stick relative to the camera (x right, y forward; a length below 0.75
  walks, from 0.75 runs). While a mod steers, a quick sword key among WASD does
  not draw the sword.
- `movement.play(group, interrupt)`: play an animation group on the player (the
  number a script's `PlayAnimation` takes). With `interrupt` the current group
  gives way now; without, the request waits for the current group's exit like
  the original's. `movement.face(heading)` sets the heading (2048 to a turn).
  Both apply after the original controller, only on updates it runs (not while
  a script, death or another owner moves the player), so the mod's choice wins;
  call them each frame you want them.
- `movement.tempo(scale)`: play the player's animation faster or slower this frame
  (markers such as a jump's take-off arrive sooner; arithmetic on the game clock,
  such as the jump's arc, keeps its pace). Applies like `play`.
- `movement.claim(heading, travel, speed, run, jump)`: take over the player's ground
  locomotion for this frame (call it every frame). The original bindings read as
  released and the original turn, walk, run and jump selection do not run; the
  mod faces `heading` (2048 to a turn) and moves `speed` map units per nominal
  update along `travel`, walking or running, from the first frame: the animation
  (forward, backward or a sidestep, by `travel` against `heading`) only shows the
  movement; `jump` starts the original jump. The engine
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
  heading, same units), `airborne()`, `animation()`, `pending()` and `idle()`
  (the playing group, the one waiting to follow it and the one it returns to;
  each one value or empty), `armed()` (a weapon is drawn or the combat stance is on: a
  claimed player then walks and runs in the fight animations and `speed` returns
  the fight speeds) and `speed(run)` (the player's own walk or run speed per
  nominal update, the base for a speed scale).
- `settings`: `cx.settings.flag("key")`, `number("key")` and `choice("key")` read the
  player's values. The SDK supplies the mod id. Read settings every frame so a
  change in the window takes effect at once.
- `memory`: `load(key)` and `save(key, values)` keep a list of numbers between
  frames for the session. wasvy starts a fresh instance of the mod for every
  system call, so a mod's own variables (statics, `thread_local!`) are gone by the
  next frame; keep state here, under keys prefixed with the mod's folder name.
- `hud`: `show(line)` draws a line of text in the top-left corner for this frame;
  call it every frame the text should stay.
- `input`: `cx.input.down(Key::INSERT)` and `tapped(Key::INSERT)` use SDK `Key`
  values; `mouse()` returns pixels and `delta()` returns seconds. The raw WIT
  calls `down` by the name `pressed` and takes Bevy `KeyCode` names.

The SDK queries each hook with the mutability wasvy needs, so a mod never sees it.

Camera, movement and shader answers apply one frame after they are written. A hook a mod stops
answering returns to the original behaviour. Particle emissions run after the current frame's
`Update`; the engine owns their motion and lifetime after emission.

## Damage-driven particles

`mods/blood` is a complete example of an event-driven mod. Its `frame` reads
`cx.damage.events()` and calls `cx.particles.burst(ParticleBurst { ... })`. The mod chooses
droplet sizes, red color, gravity and the spray direction; these are mod preferences. It needs no game assets,
shader replacement, movement takeover or guest state between frames.

A damage event contains the actor's script name, actual health lost and an optional
position in map units with original axes. Combat events also include `source`, the
known attacker position in those units. This is an incoming direction, not a
measured blade velocity or exact wound location. Environmental damage and health
services without an attributable attacker leave it absent. Names can repeat across actor instances;
use them to filter by actor type, not as unique handles. A missing position means
that the health service had no spatial state to publish; skip visual effects for it.
The host captures events when health services reduce health, including scripted,
contact, environmental and fatal loss. Healing, blocked hits and raw attribute edits
are not damage-service events. A later failure does not undo health already written
or its event. Each `PreUpdate` replaces the batch with up to 4096 recent events,
oldest first. All mods see the same batch; reading it does not consume it. Historical
events are not saved or replayed after loading a save.

Particle geometry uses the same **scene coordinates** as camera poses, with Y up.
`position.map_to_scene()` converts a damage event's map-unit point, including the
world origin; do not use point conversion for velocities. Particle velocity is in
scene units per second and acceleration in scene units per second squared. For example:

```rust
use cyrus_mod_sdk::{ParticleBurst, Vec3};
for event in cx.damage.events() {
    let Some(position) = event.position else { continue; };
    let origin = position.map_to_scene();
    let direction = event.source
        .and_then(|source| (origin - source.map_to_scene()).normalized())
        .unwrap_or(Vec3::new(0.0, -1.0, 0.0));
    cx.particles.burst(ParticleBurst {
        position: origin, direction,
        acceleration: Vec3::new(0.0, -9.8, 0.0),
        color: [0.52, 0.025, 0.018, 0.95],
        speed: [3.0, 6.0], radius: [0.025, 0.06], lifetime: [0.4, 0.7],
        spread: 0.38, stretch: 2.4, count: 16,
    });
}
```

The host uses [Hanabi](https://crates.io/crates/bevy_hanabi), the Bevy GPU particle
system. Each burst shares one GPU program with other bursts; parameters are instance
properties. Rounded droplets face the camera and stretch along their velocity.
They retain their color initially and fade at the end. They have no collision and
leave no marks. Motion and expiry use Bevy's virtual game time. GPU preparation does not consume
a burst's particle lifetime. Disable or completed
hot reload removes that mod's effects; changing or unloading the world removes all
effects. Queued bursts retain the submitting world and loaded mod revision.

`burst` returns false for an inactive owner, absent world, invalid input or exhausted
queue. Position, direction and acceleration need three finite values within +/-1e6;
direction must be nonzero and is normalized by the host. Color is sRGB RGBA in 0..=1.
Speed, radius and lifetime are ordered min/max pairs in 0..=1000 scene units per
second, 0.001..=10 scene units and 0.01..=10 seconds respectively. Spread is 0..=1
and bounds each lateral velocity component relative to the forward component;
stretch is 1..=8. Count is 1..=64 per burst. Queued and live limits count particles,
256 per mod and 1024 total; overflow drops whole bursts. The SDK supplies the mod id.
The mod can use two bursts for coarse droplets and a shorter, finer spray.

To build and try the example:

```sh
rustup target add wasm32-wasip2
cargo test --manifest-path mods/blood/Cargo.toml
just mods
cargo run -p cyrus -- --scene START
```

Open F11 and enable **Blood particles** in the Mods window. Its amount and lifetime
settings apply to new emissions immediately. Damage a combatant to exercise actual
health-service events; setting a health attribute directly is not a substitute. Turn
the mod off to verify cleanup. `cargo test --manifest-path mods/sdk/Cargo.toml` checks
the SDK's pure helpers. Mods and this renderer are native-only; a `--headless` game
run does not load mods or render particles.

For an isolated guest/host check, build `mods/blood` for `wasm32-wasip2 --release`, set
`CYRUS_BLOOD_WASM` to the resulting `blood.wasm` and run
`cargo test -p cyrus-mods --lib wasm_blood_particles -- --ignored --nocapture`.
The test loads the real component, supplies a damage event, checks its emissions and
verifies disabling it removes entities and materials. It needs no game install.
Setting `CYRUS_PARTICLE_CAPTURE` to a new PNG path also enables an offscreen GPU
capture; this needs a Vulkan-capable runtime. Cargo reports its output root as
`target_directory` in `cargo metadata --no-deps --format-version 1`; the component is
under `wasm32-wasip2/release/` there. Both `just mods` and this workflow respect an
inherited `CARGO_TARGET_DIR`.


**Shading** (`[[shader]]`). Any shader asset can be replaced; list each one in
`mod.toml`:

```toml
[[shader]]
replaces = "embedded://cyrus_render/water_surface.wesl"   # asset path of the original
file = "water_surface.wesl"                                # in the mod folder
```

The replacement must keep the original's exported functions and signatures (see
the file under `crates/cyrus-render/src/`; the water library has the
waves, ripples, shore foam and wet sand, all zero in the built-in version). It replaces the
built-in source, so a compile error shows up in the log and can hide what the
shader draws until it is fixed. Each shader is one slot: the first active mod in
id order wins and a warning is logged. Turning the mod off restores the original.

**Script patches** (`[[script-patch]]`). A mod can change what an actor's script does in one world
without code and without any game data. It names the actor and what to change:

```toml
[[script-patch]]
world = "ISLAND"          # the map, by file stem (case does not matter)
actor = "SERPENT"         # the actor's script name
fall-through = [4]        # End statements, by offset, that no longer end the script
remove = ["HideMe"]       # natives, by their SOUP386.DEF names, whose calls are skipped
```

- `remove`: every call statement in that actor's script to one of the named natives is skipped,
  as if the script did not make it (a native used as a value inside an expression stays, and the
  arguments of a skipped call are not evaluated). A name that is not a SOUP native stops the mod
  from being listed.
- `fall-through`: each offset names an `End` statement, as the `#offset` labels of a decompiled
  listing do (in decimal). It no longer ends the script's turn: execution continues with the
  statement after it, including code the original script never reaches because the `End` loops on
  itself. An offset that is not an `End` is an error in the log and the world does not load.

A skipped call or a fall-through jump neither waits nor changes any state, so the rest of the
script runs exactly as installed. The scripts are prepared at every world load, so turning the mod
off restores the original script; the patches are read at start-up, so a change takes effect after
a restart (the Mods window says so, like for `files/` overlays). A mod with only a `mod.toml`
needs no `mod.wasm`. The bundled `serpent` mod is the example: the installed ISLAND script of the
sea serpent in the Spine is complete (it waits hidden, shows itself when the player is near,
fights and sets the SerpentDead flag when killed) but sits behind an `End` that loops on itself.

**Game data** (`files/`). A `files/` folder inside the mod mirrors the game install:
`files/fxart/<name>` is read instead of the install's `fxart/<name>`, and a path the
install lacks is added. Names match ignoring case. The install is never written;
the replacement is read from the mod folder, so ship only data you made yourself.
If several active mods supply a path, the first by id wins. The files are read when
the game starts, so toggling such a mod needs a restart (the Mods window says so).
Turning the mod off restores the original, and cached assets rebuild by content.

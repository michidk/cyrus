# Bundled mods

Native-only WebAssembly mods shipped with cyrus. Each folder with a `mod.toml` is its own Cargo
crate (outside the game workspace) built for `wasm32-wasip2` against the shared SDK in `mods/sdk`; `just mods` builds
them all and installs them into `game-config/mods/<folder>/`. Activate them in
the Mods window that opens with Backtick. The browser build does not load mods.

| Mod | Hooks | Effect |
| --- | --- | --- |
| `modern-water` | environment, effects, shader replacement | Crossing waves, ripples, shore foam and wet sand. Water reflections are the base game's Modern Lighting, with or without this mod |
| `flycam` | camera, input, hud, settings, environment | A free camera. Insert toggles a free camera (every F key is taken: F1 to F12 are the original shortcuts and the cyrus menus): WASD, Space/Left Shift, mouse look, a speed setting; the world streams around it, the player stays put and keys typed into the console are ignored; a corner line shows whether it is on |
| `movement` | movement, player, camera, settings, input | Third-person movement like current action games: the mouse orbits the camera, the keys move the player relative to it at full speed at once (`claim`), the animations only show it (`play`); a drawn weapon keeps the player facing the camera |
| `serpent` | script patch (manifest only, no code) | Restores the sea serpent in the Spine that was cut from the game: its installed script is complete but sits behind an endless `End`, which the mod lets fall through |
| `actor-info` | actors, player, hud | A small example of the read-only hooks: shows the nearest visible actor, its health and its distance in the corner |

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
`cx.environment`, `cx.actors`, `cx.effects`, ...). The sections below describe each hook in the host's own terms; the SDK keeps their
units and timing and swaps the raw numbers for types: a camera `Pose`, `Vec3` positions, `Option` where
the host answers an empty list, `Claim` for `movement.claim`, `Water` and `Lighting` for the environment, `cx.settings.flag("key")`
without the mod id, `cx.memory.load::<T>("name")` for any type that implements `Persist`, and
`cx.world_changed`, true on the first frame after a world is loaded or unloaded, for resetting
state kept per world. `cargo test` in a mod folder or
`mods/sdk` runs its pure logic on the host.

The raw WIT is `crates/cyrus-mods/wit/cyrus.wit`; wasvy 0.0.11 carries only primitives, `option` and `list`
across it, which is why the typed layer lives in the SDK. Other wasvy guest languages (Python, Go) work
against the WIT directly.

- `environment`: read-only facts, refreshed every frame. `seconds()` is the presentation clock;
  `generation()` changes whenever a world is loaded or reloaded (0 while none is), so a mod that
  keeps state per world compares it with the value it saw last (the SDK does this for you as
  `cx.world_changed`); `index()` is the loaded world's WORLD.INI index; `lighting()` is `"modern"` or
  `"faithful"`; `water()` is the world's water as amplitude, speed and frequency (world file
  units) plus the terrain grid cell size in scene units, empty when it has none.
- `actors`: the loaded world's actors, read-only, in actor order and refreshed every frame:
  `count()`, then `name(i)` (script name), `position(i)` (map units, original axes, empty before the
  actor has one), `health(i)` and `hidden(i)`. An index outside the range answers a neutral value.
  Indices are valid for one world generation. The SDK gives `Actor` values, `actors.all()` and
  `actors.find(name)`.
- `effects`: parameter blocks for the game's shaders, in named slots. Effects run in shaders: a mod
  replaces a shader library (`[[shader]]`) and fills the slot that library reads; the game never
  hands out its meshes. Fill a slot every frame; stopping restores the original. Slot
  `"terrain-surface"` = `[effects, 0, amplitude, cell]` turns on the surface library's shore foam, wet
  sand and ripples and, with a positive `amplitude`, moves the waves in the terrain shader (the SDK:
  `effects.terrain_surface(TerrainSurface { .. })`).

- `camera`: `pose()` is the current view; `drive(pose)` moves it this frame
  and suppresses player input. Stop calling it to hand control back.
  `orbit()` each frame lets the mouse and right stick turn the follow camera
  around the player, with the cursor captured,
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
- `settings`: `flag(id, key)`, `number(id, key)` and `choice(id, key)` read the
  player's values; `id` is the mod's folder name. Read them every frame so a
  change in the window takes effect at once.
- `memory`: `load(key)` and `save(key, values)` keep a list of numbers between
  frames for the session. wasvy starts a fresh instance of the mod for every
  system call, so a mod's own variables (statics, `thread_local!`) are gone by the
  next frame; keep state here, under keys prefixed with the mod's folder name.
- `hud`: `show(line)` draws a line of text in the top-left corner for this frame;
  call it every frame the text should stay.
- `input`: `pressed` and `tapped` (Bevy `KeyCode` names), `mouse` (pixels),
  `delta` (seconds).

The SDK queries each hook with the mutability wasvy needs, so a mod never sees it.

Answers apply one frame after they are written. A hook a mod stops answering
returns to the original behaviour.

**Shading** (`[[shader]]`). Any shader asset can be replaced; list each one in
`mod.toml`:

```toml
[[shader]]
replaces = "embedded://cyrus_render/terrain_surface.wesl"   # asset path of the original
file = "terrain_surface.wesl"                                # in the mod folder
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

# Bundled mods

Native mods shipped with cyrus, and the guide for writing your own. The browser
build does not load mods. Mods start disabled: open the Backtick menu and select
**Mods** to enable them and edit their settings.

- **Players:** the `cyrus-with-mods-<target>` release downloads contain every
  bundled mod. Extract the whole archive; the mods sit in the `mods/` folder beside
  the programs.
- **From source:** `just mods` builds every mod with a `Cargo.toml` for
  `wasm32-wasip2` against the SDK in `mods/sdk`, copies manifest-only mods, and
  installs them into `mods/<folder>/` beside the debug game build
  (`<target>/debug/mods`), which `just run` uses.

| Mod | Kind | Effect |
| --- | --- | --- |
| `autosave` | Wasm | Periodic saves during eligible gameplay ([below](#save-service)) |
| `grass` | Wasm + shader | Seeded grass blades on green terrain with distance bands and wind |
| `ragdolls` | Wasm | Articulated visual death physics |
| `sway` | Wasm | Palm trees and plants sway in the wind, with Faithful or Modern lighting |
| `blood` | Wasm | A brief spray of red particles when the player or an actor loses health |
| `modern-water` | Wasm + shader | Crossing waves, ripples, shore foam and wet sand (reflections are the base game's Modern Lighting) |
| `flycam` | Wasm | Insert toggles a free camera: WASD, Space/Left Shift, mouse look; optionally the player follows the camera ("Move the player with the camera") or lands where it stops ("Leave the player where the camera stops", a high drop can hurt) |
| `movement` | Wasm | Camera-relative third-person movement with mouse orbit and optional camera follow |
| `actor-info` | Wasm | Small read-only example: nearest visible actor, its health and distance |
| `serpent` | manifest | Restores the cut sea serpent in the Spine (its script sits behind an endless `End`) |
| `one-punch` | manifest | Cyrus's sword attacks (COMBAT.INI `attack65`–`attack71`) deal 100 damage |
| `unofficial-patch` | manifest | Port of Dillonn241's Unofficial Redguard Patch 1.01 as data patches |

Everything these mods add is a mod preference, not recovered original behaviour.
`unofficial-patch` fixes flags and counters nothing set, wrong tests and receivers,
misplaced logbook lines and about ninety typos; it leaves out the Soul Sword
strength fix that the patch's version 1.03 removed. Its
[`mod.toml`](unofficial-patch/mod.toml) explains each change. A test compares the
patched scripts with the patch author's maps (all matched except EXTPALAC, which
was not compared); nothing was played through.

## Writing a mod

A mod is a folder holding `mod.toml` plus any of `mod.wasm`, shader files and a
`files/` folder. Drop it into the `mods/` folder beside the game while the game runs and it appears
in the Mods window; delete it and it goes. Copy a bundled mod as a start.

**Manifest.** `name` is required; `description`, `version` and `author` are shown in
the Mods window. `[[setting]]` entries (`type` = `number` with `default`/`min`/`max`,
`bool`, or `choice` with `options`) appear under an active mod and are saved to
`game-config/mod-settings.json`; a stale saved value falls back to the default. See
[`movement/mod.toml`](movement/mod.toml).

**Logic** (`mod.wasm`). Rust mods depend on the SDK (`cyrus-mod-sdk = { path = "../sdk" }`):

```rust
use cyrus_mod_sdk::{Cx, Key, Mod};

struct Hello;

impl Mod for Hello {
    const ID: &'static str = "hello"; // the folder name under mods/
    fn frame(cx: &Cx) {
        if cx.input.down(Key::INSERT) {
            cx.hud.show("hello");
        }
    }
}

cyrus_mod_sdk::export_mod!(Hello);
```

`frame` runs every `Update` with every hook in `cx`. The SDK handles in
[`sdk/src`](sdk/src) document each hook's units and limits; the raw interface is
[`crates/cyrus-mods/wit/cyrus.wit`](../crates/cyrus-mods/wit/cyrus.wit), which other
wasvy guest languages can target directly.

| Hook | What it does |
| --- | --- |
| `environment`, `world_changed` | Clock, world index and generation, Lighting mode, water parameters |
| `actors`, `player` | Read-only actors (name, position, health) and player state (heading, animation, armed, speed); `place` moves the player |
| `camera` | Read the pose, `drive` it, `orbit` the follow camera, `turn` it, detect `manual` look |
| `movement` | `steer` like a stick, `play` animation groups, `face`, `tempo`, `claim` ground locomotion, `tune` jumps |
| `collision`, `natives` | Segment casts against nearby geometry; observe calls to a named script native |
| `effects` | Fill named parameter slots read by replaced shaders |
| `damage`, `particles` | Health-loss events and GPU particle bursts ([below](#damage-driven-particles)) |
| `saves` | Request a save ([below](#save-service)) |
| `terrain`, `geometry` | Terrain snapshot reads and owned mesh uploads ([below](#terrain-and-owned-geometry)) |
| `lifecycle`, `physics` | Actor handles, deaths and visual rigid bodies ([below](#death-observations-and-owned-physics)) |
| `settings`, `memory`, `hud`, `input` | Mod settings, session memory, a text line, keys and mouse |

Rules every mod must respect:

- Call a hook every frame you want its effect; answers apply one frame later, and a
  hook a mod stops answering returns to the original behaviour.
- wasvy starts a fresh instance per call, so Rust statics do not survive a frame;
  keep state in `cx.memory`, and reset per-world state on `cx.world_changed`.
- Positions from `actors`/`player`/`collision` are map units in original axes;
  camera, particles, terrain, geometry and physics use scene units, Y up.
  `Vec3::map_to_scene` converts points.
- Guest submissions are validated and budgeted; rejected input changes nothing.
- Mods run with whole-world access, stdio and network. Only run mods you trust.

**Shaders** (`[[shader]]`, `replaces` = asset path such as
`embedded://cyrus_render/terrain_surface.wesl`, `file` = replacement). The
replacement must keep the original's exported functions (see `crates/cyrus-render/src/`).
Each shader is one slot; the first active mod by id wins and a warning is logged.

**Data patches** change installed data in memory without shipping any of it; the
install is never written and disabling the mod restores the original. They take
effect after a restart. A patch that does not match is an error, never a silent no-op.

- `[[script-patch]]` (`world`, `actor`): `remove` calls to named natives,
  `fall-through` named `End` statements, `retarget` receivers, and
  `[[script-patch.edit]]` entries (`at` an original statement offset, then one of
  `insert-before`, `insert-after`, `replace`, `delete`, `set-argument`,
  `set-target`, `condition`, `branch-to`). See [`serpent`](serpent/mod.toml) and
  [`unofficial-patch`](unofficial-patch/mod.toml).
- `[[ini-patch]]` sets a key in named sections of an install INI, or replaces a
  fragment; `[[text-patch]]` corrects a fragment of one dialogue line by label. See
  [`one-punch`](one-punch/mod.toml).
- `files/` mirrors the install: its files replace or add to the install's
  (case-insensitive, first active mod by id wins). Ship only data you made.

## Damage-driven particles

[`blood`](blood/src) is the example. `cx.damage.events()` gives up to 4096 recent
health-loss events per frame (shared, not consumed, not saved): actor script name,
health lost, optional position and optional attacker `source`. Names repeat across
actors; a missing position means there is nothing to place an effect at.
`cx.particles.burst(ParticleBurst { .. })` emits camera-facing droplets in scene
units, simulated by [Hanabi](https://crates.io/crates/bevy_hanabi) on game time,
with no collision. Limits: 64 particles per burst, 256 per mod, 1024 total; `burst`
returns false when rejected. Effects clear on disable, reload or world change.

## Save service

`autosave` requests a save after 300 eligible seconds (60–1800, configurable) into
reserved autosave slot one; menus, dialogue, drawn weapons, airborne movement and
death defer it, and a failure retries after 30 seconds. The SDK's `cx.saves`
offers `ready()`, `save()`, `status()` and `error()`; `SaveStatus::Saved` means
storage committed. The guest owns timing and retries; the host rechecks
eligibility and serializes all saves through the existing session storage.

## Terrain and owned geometry

[`grass`](grass/src) is the example. `cx.terrain` reads the loaded terrain as
paged triangles (with UVs and texture IDs) and texture pixels, scoped to a snapshot
revision; `cx.geometry.replace(key, generation, &mesh, material)` uploads an indexed
mesh lit like the scene, until `remove`, disable, reload or world change. A mod can
replace the `deform` function of `embedded://cyrus_render/geometry_surface.wesl`
(as grass does with `grass.wesl`); set `bounds_padding` to cover its displacement.
Clear cached placement when `cx.geometry.lease()` changes. Budgets and error codes
are in [`sdk/src/geometry.rs`](sdk/src/geometry.rs). Do not ship installed images
or mesh data in a mod.

## Scenery tilts

[`sway`](sway/src) is the example. `cx.scenery` lists the loaded world's placed static
models by installed model name with the world position of their base, valid for one
`revision()`. `cx.scenery.tilt(index, rotation)` turns a piece's drawn model about its
base for this frame; a piece no mod tilts rests at its installed placement, and
collision always keeps it.

## Death observations and owned physics

[`ragdolls`](ragdolls/src) is the example. `cx.lifecycle` gives opaque `ActorId`s,
snapshots and `deaths()`, committed death transitions with the captured model,
frame and pose. `cx.physics.claim(actor, &assembly)` attaches guest-built bodies,
joints and vertex weights to a dead actor's frozen mesh; a 120 Hz Rapier solver
moves them against static geometry and dry terrain. It never touches health,
scripts or gameplay collision; hide, revival, reload, save load or world change
release the claim and restore the original mesh. Limits are in
[`sdk/src/physics.rs`](sdk/src/physics.rs).

## Building and testing

```sh
rustup target add wasm32-wasip2
cargo test --manifest-path mods/sdk/Cargo.toml      # SDK helpers; same for any mod folder
just mods
cargo run -p cyrus -- --scene START                  # then enable the mod under Backtick > Mods
```

A `--headless` run does not load mods. For a guest/host check without an install,
build a mod with `cargo build --release --target wasm32-wasip2 --manifest-path mods/<mod>/Cargo.toml`,
point `CYRUS_BLOOD_WASM`, `CYRUS_GRASS_WASM` or `CYRUS_SWAY_WASM` at the built `.wasm` (under
`wasm32-wasip2/release/` in the Cargo target directory) and run
`cargo test -p cyrus-mods --lib wasm_blood_particles -- --ignored` or
`wasm_grass_geometry` or `wasm_sway_tilts_near_plants_only`. `CYRUS_PARTICLE_CAPTURE=<new.png>` adds an offscreen GPU
capture (needs Vulkan).

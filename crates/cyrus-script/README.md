# cyrus-script

Game scripting host, player input and controller rules, the shared simulation
clock and the in-game screens (logbook, inventory, item viewer, island map,
console). It runs SOUP programs from `cyrus-soup` against `cyrus-world`,
`cyrus-collision` and `cyrus-assets`, and takes fidelity, preferences, cheats,
the random stream and application state from `cyrus-game`. It does not render,
play audio, store files or talk to the outside world: the hosts (`cyrus`, `cyrus-web`) and the render, audio, session and
debug crates do that on top of it.

## Modules

| Module | Responsibility |
| --- | --- |
| [lib](src/lib.rs) | `GamePlugin`, paused-gameplay state and the gameplay book/information shortcuts |
| [player](src/player.rs) | Player scene preparation, held input, controller rules and logical movement |
| [script](src/script/mod.rs) | Game-owned SOUP hosts, logical actors, native functions and ordinary updates |
| [script/registry](src/script/registry.rs) | `ScriptWorld`: logical instances in authored order and the ordinary update stages |
| [script/tasks](src/script/tasks.rs) | Actor-owned native task tokens, waits and their persistence |
| [script/sound_dispatch](src/script/sound_dispatch.rs) | Ordered animation-sound admission before actors have presentation bindings |
| [clock](src/clock.rs) | Shared simulation timing and validated timing snapshots |
| [logbook](src/logbook.rs) | Logbook reader layout, scrolling and manual-entry editor |
| [inventory_screen](src/inventory_screen.rs) | Inventory screen list, cursor, layout and lifecycle |
| [item_viewer](src/item_viewer.rs) | Pose, fade and exit of the `ShowItem` modal |
| [island_map](src/island_map.rs) | Island map screen markers and lifecycle |
| [console](src/console.rs) | Console availability and cheat command handlers |

The generated [repository map](../../docs/MAP.md) lists every module. API
contracts, units and preservation references live in the Rust docs beside
their definitions.

## Usage

Hosts add `GamePlugin`, which installs the `cyrus-game` state (application
state, fidelity, preferences, cheats, random stream), `PlayerPlugin`,
`ScriptPlugin` and the console commands:

```rust
app.add_plugins(cyrus_script::GamePlugin);
```

The host supplies Bevy asset, input and state infrastructure and a concrete
`PreferenceStore`. Scripts are driven through `ScriptPlugin`, `ScriptInputs`
and `run_ordinary`; a missing host service returns a contextual error.

## Notes

- Behaviour follows redguard-preservation's engine docs, mainly
  [SOUP.md](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md),
  [player.md](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/player.md)
  and [hud.md](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/hud.md).
  Cite the section in the doc comment of the code you change, not here.
- Logic stays independent of presentation: screens and actors expose state and
  snapshots (`ScriptWorld::actor_snapshot`, death and damage events) that
  render and audio adapters read. Observations never drive gameplay and are
  not serialized.
- Menu, console, logbook and inventory suspension must also suspend the shared
  simulation clock and automatic gameplay input. Inspection adapters set
  `InputSuppressed` before `InputSample`. Player transforms stay independent of
  camera transforms.
- Mod hooks (for example `CameraOrbit` yaw requests) leave the faithful path
  unchanged when no mod is active.
- Natives and properties without complete evidence stay explicitly
  unsupported. [SOUP #4](https://github.com/michidk/cyrus/issues/4) tracks
  remaining work and acceptance.

## Testing

```sh
cargo test -p cyrus-script --lib
cargo test -p cyrus-script --test script_registry   # or any file in tests/
just verify script                                  # tests and clippy for this crate
```

The installed island controller fixture places the player at the NCPLNKE1 plank
and supplies a local map view before the ordinary update. It verifies support
acquisition and the plank's runtime position link at a clamped rendered-frame duration.
The headless host supplies this view explicitly because it has no follow camera.

Installed-data tests (`*_installed.rs` and others) read `REDGUARD_DIR` or the
workspace `redguard` link and report a skip when no install is present; with an
install linked, a skip fails the gate. Runtime changes also need the headless,
replay or original-game comparison described in [METHOD](../../docs/METHOD.md)
and [SETUP](../../docs/SETUP.md).

Held directional jump input applies the documented jump-entry replacement before
calling a movement helper. In particular, Forward+Jump during a low climb leaves
the ledge animation's pending idle request intact rather than queuing Run behind
it. The common jump-key arbitration still runs after directional dispatch.

Deferred selected-player water transitions retain the stored surface from each
support probe. Flushing several transitions preserves their entry/exit sound and
animation selection without replacing the actor's latest surface.

Water-transition sounds retain the support-query position when an accepted walk
moves the player before the queued sound is flushed.

Deferred water/lava effects retain their query-time camera-relative placement,
so walking before dispatch cannot move the splash to the player's later position.

Changing the selected player retires pending water/lava transitions from the outgoing
selection; reselecting the same player preserves pending dispatch.

Deferred transition animation requests retain the support-query motion and torch
selection, so later movement cannot reverse the swim or deep-water exit request.

A finished ledge climb releases its living state even when the restored actor has
no animation receiver, so subsequent ordinary updates can continue.

High-ledge frame-threshold requests preserve excluded receivers' animation owners
and complete without a receiver, just like ordinary animation requests.

The near and far ledge scans each gather a fresh record table. Rejected near
records cannot occupy the farther scan's slots or suppress its climbable ledges.

Ledge fitting truncates signed Q8 trial coordinates toward zero before testing
the body and head spheres, following preservation 0.13.2
([ledge record classification](https://github.com/michidk/redguard-preservation/blob/v0.13.2/docs/engine/player.md#ledge-record-classification)).

Held torches publish their installed model attachment and transient tip light during
the admitted model update. The burn counter alone does not equip a torch; the held
item, completed lighting transition, item system and shell context gate emission.

Ordinary actor view windows clamp to the map edges even when a carried player
and camera leave the authored map, following preservation’s animation traversal rule.

Static-rope follow cameras select the installed upper/lower camera record from
maximum swing and the held link. Their installed aim angles replace player
orientation. Sound services use the resulting published logical camera pose,
independently of an inspection view or render gliding, following preservation's
[SYSTEM.INI camera settings](https://github.com/michidk/redguard-preservation/blob/main/docs/config/system-ini.md#camera).

With `cyrus_script=debug` logging enabled, a failed selected-player forward-support
query records its Q8 current/proposed positions, wrapped angles, live map-unit
collision extents and spheres, ordered candidates, and retained model transforms
and frame identities. The diagnostic leaves the query error and movement unchanged.

Attached rope controls use the accelerated turn accumulators. Downward link
selection uses its own two-link hand target, while initial attachment retains
the unbiased hand target (preservation SOUP.md, rope attachment and controls).

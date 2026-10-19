# cyrus-game

Portable gameplay state, fidelity policy, player input and deterministic
controller rules. Depends on `cyrus-world`, `cyrus-collision`, `cyrus-console`
and `cyrus-assets`. Hosts own installation access, rendering, cameras, audio
playback, storage and external control.

## Integration

`GamePlugin` installs app state, fidelity, `PlayerPlugin` and `ScriptPlugin`.
The host supplies Bevy asset/input/state infrastructure and `Preferences` with
a concrete `PreferenceStore`. Preferences use original action units; successful
persistence precedes runtime application. Fidelity areas default to Faithful.

Player transforms remain independent of camera transforms. Inspection adapters
set `InputSuppressed` before `InputSample`. Menu, console and logbook suspension
must also suspend the shared simulation clock and automatic gameplay input.

## Module guide

| Module | Responsibility |
| --- | --- |
| [player](src/player.rs) | Player scene preparation, held input, controller rules and logical movement |
| [script](src/script/mod.rs) | Game-owned SOUP hosts, logical actors and ordinary updates |
| [clock](src/clock.rs) | Shared simulation timing and validated timing snapshots |
| [random](src/random.rs) | Session random stream shared by scripts and gameplay services |
| [preferences](src/preferences.rs) | Accepted menu settings and host-provided persistence |
| [fidelity](src/fidelity.rs) | Faithful/Modern policy by gameplay area |
| [logbook](src/logbook.rs) | Shared reading-screen state and input |
| [console](src/console.rs) | Console focus, editor and gameplay command registration |
| [cheats](src/cheats.rs) | Named cheat state and host-provided CHT persistence |

API contracts, units, ownership, failure ordering and preservation references
live in Rust documentation beside their definitions. The generated
[repository map](../../docs/MAP.md) lists individual modules.

## SOUP integration

`ScriptWorld` owns generation-checked logical actor identities, authored actor
order, mutable script/control state, spatial relationships, native task slots
and walk-node reservations. `ScriptActors` owns the active session and world.
`ScriptBinding` and `ScriptScene` connect logical state to presentation.
`ScriptStatus` and retained `PlaybackDiagnostics` expose preparation and runtime
failures separately.

`ScriptInputs` lends timing, selected-player state, geometry and host services
to explicit operations. `run_ordinary` composes the supported automatic stages
using `ScriptView` and shared session owners. Calling an explicit operation
does not establish automatic scheduler admission or complete gameplay support.
Missing required ownership and unsupported branches return contextual errors.

The implementation is grouped by responsibility:

- [Registry](src/script/registry.rs), [ordinary scheduler](src/script/registry/ordinary.rs)
  and [tasks](src/script/tasks.rs) own actor dispatch and native continuation.
- [Spatial state](src/script/spatial.rs), [navigation](src/script/navigation.rs)
  and [point movement](src/script/registry/point.rs) own poses, reservations and
  supported route/direct walking phases. Collision and movement services live
  in the corresponding registry modules.
- [Combat state](src/script/combat.rs), [decision rules](src/script/decision.rs),
  [defence](src/script/registry/defence.rs), [attacks](src/script/registry/attack.rs),
  [spacing](src/script/registry/spacing.rs) and [perception](src/script/registry/sight.rs)
  separate retained state, deterministic rules and live receiver services.
- [Dialogue](src/script/dialogue.rs), [HUD](src/script/hud.rs),
  [inventory](src/script/inventory.rs) and [journal](src/script/journal.rs) own
  shared gameplay records; hosts supply playback, drawing and storage.
- [Persistence](src/script/registry/persistence.rs) validates snapshot candidates
  before replacing accepted logical state. Component snapshots do not enable
  player-facing save/load or original SAV import.
- [Camera state](src/script/camera.rs) prepares logical poses; the session host
  must supply live bindings, geometry and publication.

**Verified:** bounded script, movement, combat-service and persistence contracts
exist in the source and tests. Full automatic START combat, selected-player
combat/death, opening-story progression, complete camera services and
original/native/browser acceptance remain **Unknown**. The
[roadmap](../../docs/ROADMAP.md) owns delivery status and remaining work.

## Verification

Run the narrowest relevant suite during development, for example:

```sh
cargo test -p cyrus-game --lib
cargo test -p cyrus-game --test script_registry
```

Installed tests use `REDGUARD_DIR` or the workspace `redguard` link. They report
a skip when the install is absent; the repository gate rejects such skips when
an install is available. Code changes require `just gates` after the final
relevant edit. Runtime changes also require the applicable headless, replay or
original-game comparison. See [METHOD](../../docs/METHOD.md) and
[SETUP](../../docs/SETUP.md) for the verification and launch workflows.

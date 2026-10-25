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
set `InputSuppressed` before `InputSample`. Menu, console, logbook and inventory screen suspension
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
| [inventory_screen](src/inventory_screen.rs) | Inventory screen list, cursor, layout and lifecycle |
| [console](src/console.rs) | Console focus, editor and gameplay command registration |
| [cheats](src/cheats.rs) | Named cheat state and host-provided CHT persistence |

API contracts, units, ownership, failure ordering and preservation references
live in Rust documentation beside their definitions. The generated
[repository map](../../docs/MAP.md) lists individual modules.

## SOUP integration

The [script module documentation](src/script/mod.rs) describes SOUP state
ownership, host services and internal responsibilities. Hosts integrate through
`ScriptPlugin`, `ScriptInputs` and `run_ordinary`; unavailable services return
contextual errors. Explicit script operations do not establish automatic
scheduler admission or complete gameplay support. The zero-argument `Jump` native
completes immediately after admission and its state writes; selected-player
edge and hang services use the caller's receiver-bound collision geometry.
Its contract follows preservation's `player.md#script-jump`.

Created-object records (including projectile records) retain their creator,
namespace and execution order through snapshots. `KillMe` removes their
executable state, but allocator recycling and reuse across a long live session
still require installed original/native/browser acceptance; the current public
tests do not claim that parity.

[SOUP #4](https://github.com/michidk/cyrus/issues/4) tracks remaining work and acceptance.

## Verification

Run the narrowest relevant suite during development, for example:

```sh
cargo test -p cyrus-game --lib
cargo test -p cyrus-game --test script_registry
```

Installed tests use `REDGUARD_DIR` or the workspace `redguard` link. They report
a skip when the install is absent; the repository gate rejects such skips when
an install is available. Code changes require `just check` and `just verify game` after the final
relevant edit. Workspace dependency or shared build-configuration changes also
require `just gates`. Preserve successful checks when their inputs are unchanged.
Runtime changes also require the applicable headless, replay or original-game
comparison. See [METHOD](../../docs/METHOD.md) and
[SETUP](../../docs/SETUP.md) for the verification and launch workflows.

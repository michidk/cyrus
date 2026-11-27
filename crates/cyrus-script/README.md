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
| [script/tasks/completion](src/script/tasks/completion.rs) | Private completion families selected by the exhaustive task-variant dispatcher |
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

Original combat import binds the saved attack and defence selectors to their
respective host executor owners. Their serialized order differs from the host's
retained move order; an absent defence selector does not replace a saved attack
selector or select an invented motion. The format ownership is documented in
[preservation](https://github.com/michidk/redguard-preservation/blob/main/docs/formats/SAVEGAME.md#actor-combat-restoration).

F1/F10 information overlays suspend updates while retaining the player's script binding and displayed animation pose. Closing them resumes that continuation.

Topic menus retain the installed forward/back bindings even when a mod claims
locomotion. Steering mods also supply their configured raw direction through
`movement.steer`, so menu navigation does not depend on a movement claim.
Script-owned dialogue discards orbit input; it does not defer a camera turn
until player control returns.

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
- The first automatic-update failure is retained and logged once with world and
  contextual script error; repeated reports preserve the first cause.
- Logic stays independent of presentation: screens and actors expose state and
  snapshots (`ScriptWorld::actor_snapshot`, death and damage events) that
  render and audio adapters read. Observations never drive gameplay and are
  not serialized.
- `ScriptWorld::world_loop_frame` exposes the retained game counter separately from `ordinary_updates`, which restarts after restore.
- The clock exposes the current measured nominal carry separately from the gameplay step. World-entry fades consume it before the warmup or replay override; it is unavailable until the first clock invocation and is recomputed on each update.
- Model resource loading, collision admission and host service preparation do not advance the
  simulation clock. The first ready update excludes loading time, so animation
  and BIOS ticks do not depend on asynchronous load duration.
- Scripted player alignment retains animation-owned backward walking and sidesteps
  until arrival, using the player support query rather than nonplayer admission.
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

The opt-in `original-save-import` feature provides
`OriginalOwnerContinuation::{decode, decode_code, decode_shell}` for staging a
complete actor, code-owner or shell interpreter continuation from published typed
save views. It validates saved owner identity and code ownership, binds authored locals, checks all callbacks
against explicit prepared native sources, and returns the VM snapshot together
with every slot's invocation words. No call runs and no timer advances during
staging. `NativeTasks::from_original` constructs supported descriptor-only native tasks
without issuing or polling their callbacks and rejects the entire candidate for
a handler requiring additional native-state binding. It covers waits, direct
and routed point commands, timed marker/location moves and animation requests.
`ActorNavigation::from_original` binds the published saved map identity and keeps
all node history. Full load clears reservations separately; the first navigation
validation keeps valid held indices without reclaiming their nodes. Native
invocation words and every remaining task/service owner must be restored before
publishing that candidate. `NativeTasks::from_original_with_words` additionally
restores relative/absolute rotation, local-axis movement and ambient sample
bindings from explicit saved token owners. Active ambient handles require a
matching restored playback owner. `OriginalCodeAllocation::bind` identifies
SMCO owners by the type-two placement allocation counter, excludes coloured
lights and validates the entire saved population against installed code. This owner-level API does not admit a complete original session.

`ScriptWorld::restore_original_bound_animations` binds selected and previous poses
against the installed registration table, receiver frame table and texture variant.
It validates all requested owners before replacing any animation or stored default,
without issuing commands or cues. This component transaction does not publish a
session; the remaining interpreter, native-service and world owners must commit
with it.

The feature-enabled `original_continuation` tests exercise synthetic public imports
and the first explicit task update. `original_owner_installed` additionally reads
an owned save from `CYRUS_ORIGINAL_SAVE` and an install from `REDGUARD_DIR` to
validate every saved scripted owner against installed sources. This source check
does not establish native allocation identity or matched-session acceptance.

The opt-in `original-save-import` feature exposes
`script::original_save::OriginalSaveSubset`, which validates original player world, Q8
position, heading, health, inventory, global flags and retained clock before applying them to an
initialized off-live candidate. This is a limited import API, not full session
restoration. The native Load adapter uses it with a matching native-session baseline. It
restores the saved static supporting actor's pose and attributes, binds it by its
initialized runtime identity, and refreshes its collision model and regional cell.
When that saved support has no script continuation, import cancels baseline
native tasks and clears its interpreter continuations before resuming gameplay.
Missing or mismatched models and scripted, animated or chained supports reject
with context. The imported SVGV clock restores exposed timing, FPS settings,
measurement state, startup countdown and fractional nominal/fine carries; its
host epoch is rebound so loading contributes no elapsed gameplay time.
Animation headers bind to the installed actor groups without replaying sounds
or commands. A retained ShowFrame cursor restores group, cursor, countdowns,
translation/rotation words and frame selection when the current and previous
model/frame bindings agree. Missing actor owners, other pose cursors and distinct
previous model/frame bindings retain the native baseline with contextual warnings.
The returned message explicitly lists unsupported script/task continuations,
remaining actor state, unresolved animation/model and sound bindings,
controller state and journal. Transformed players and flag states outside
the verified unlatched encoding reject with context. Hosts discard the candidate
on error and must display the limitation before publication. Import also refreshes the native displayed-health owner to match the imported health byte.

`REDGUARD_ORIGINAL_SAVE_INSTALL=/path/to/Redguard cargo test -p cyrus-script
--features original-save-import --test original_save` exercises retained installed slots without a window or
game loop, including the wrong-world and unsupported-player rejection paths.
Set `REDGUARD_ORIGINAL_SAVE_DIR=/path/to/private/SAVEGAME` when slots live
outside the read-only install. Without installed slots it reports a skip.

Original-save animation import restores the saved panic-group attribute alongside
its playback default, so the first ordinary default selection preserves the
imported idle pose. Other controller-dependent default branches remain subject
to the limited-import warning.

`ScriptActors::current_is_restored` identifies a saved continuation by its live
generation. Replay hosts use it instead of treating the reset observation counter
as a request to run fresh-world startup updates. Script status exposes this as
`restored`; changing worlds clears it.

A fresh replay hold permits entry-only model/environment initialization and immutable
prelight checkpoint preparation before host ordinary-service admission. Body execution,
automatic traversal and clock/RNG advancement remain held. This lets original-slot
loading capture its destination baseline without releasing update-zero acquisition;
restored and revisited continuations retain their own admission paths.

Original native task construction restores PushAnimation/PushControlAnimation
slot phases against an explicitly bound saved receiver animation. It does not
request an animation or replay cues during import. RTX construction requires a
cleared dialogue owner from `DialogueState::from_original_load`; original loading
clears shared text/audio rather than reconstructing saved speech. Saved RTX words
remain intact and the next explicit update observes the inactive record without
replaying the label. The host must clear cache/audio owners with the dialogue
candidate. These service constructors do not enable complete session admission.

`ScriptWorld::restore_original_actor_states` validates saved identities and all
referenced receivers before replacing spatial/controller values. It retains
independent local, attached, previous, initial and recovery histories, embedded
model transforms, torch counters and detached position-link caches. The camera
has a controller allocation alongside its independent spatial owner. Invalid
bindings leave the existing world unchanged. Attached families and rope
attachments require additional load bindings and are rejected by this operation.
This component transaction does not enable original session admission. The host
must compose it with interpreter, task, navigation, animation and shared service
replacements off-live before publishing a complete session.


The feature-gated `LogicalSessionCandidate::import_original` assembles original
worlds, interpreter continuations, allocation-bound native tasks, navigation,
combat, animation, controller, family/camera caches, saved globals, inventory,
journal and simulation clock on a detached candidate. Every source and allocation
census must match. Unsupported retained process owners reject with context. The
existing candidate changes only after all validation succeeds; import runs no
callbacks or service updates. This logical transaction must still join prepared
audio and rendered host services before complete runtime admission is enabled.

Complete original-session publication retains the saved shell allocation order when
round-tripped through native checkpoints. Publication before presentation suppresses
exit arbitration for that frame; restored requests are evaluated on the next explicit
world update, rather than during admission.

Speech-animation task polling updates activation HUD ownership independently of
prompt drawing: a running `rtxAnim` cycle clears pending prompt text and a completed
cycle clears its override timer. Reset, distance and pending-source early returns
retain that state; `RTXpAnim` does not mutate it. The contract follows preservation's
sound and HUD engine references.

Integration fixtures that need state unavailable through supported gameplay stages
import extension traits from `script::test_support`. This module and the private
activation seeding helper compile only with `test-support`, a feature enabled by
this package's self dev-dependency for both default and original-save-import test
runs. Production dependencies do not enable it. Hosts must use ordinary stages,
property dispatch and validated snapshot admission; the fixture traits are not a
production host interface. Attribute seeds use the existing `set_actor_attribute`
operation instead of a test hook.

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

The logbook reader owns its manual-entry dialog state and keyboard focus. Installed action C
opens an empty byte editor; presentation reads `LogbookScreen::manual_entry()` and
`manual_entry_cursor_visible()`. Acceptance appends a style-9 journal record through the
existing host store before refreshing the reader at newest text. Cancellation leaves the
journal unchanged. Editing retains the original 64-byte cap, ASCII uppercasing and shifted
CR/VT controls. Cursor phase starts hidden and toggles only on changed sampled BIOS ticks
divisible by six, without replaying skipped ticks. These rules follow preservation's manual
logbook contract. Native and browser hosts share the editor and persisted journal owner.

Verified: inventory closing waits for Inventory and Escape release before sampling
Activate or Enter once to commit the cursor selection. It then waits for Walk,
Activate and Enter release in order. Earlier waits do not restart when their keys
are pressed again. The candle timer clears after the first two waits, before selection is sampled.
The shared modal lock and paused virtual time remain held until the final wait
finishes. Selecting an item does not use it. This follows preservation's
[inventory exit contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/hud.md#exit).

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

Verified: killed map pickups retain their removal state across world restoration.
Public standalone ordinary main and hook stages skip killed records, so restored pickups cannot
restart their acquisition scripts. Competing activation callers retain one owner
until its own `End` or `Deactivate` releases it; the surviving pickup keeps
its own continuation. These rules follow preservation's
[killed-object removal](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#killed-object-removal)
and [activation prompt](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/hud.md#activation-prompt).

Verified: `PushAllItems` replaces the owned words of consecutive identifiers starting
at zero through the shared saved-count owner. Repeated calls preserve the first
saved words; a missing identifier retains earlier replacements and fails before
caller continuation. Closed processing skips lookup. Selection and acquisition/drop
effects are unchanged. This follows preservation's
[indexed iteration](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#pushallitems-indexed-iteration).

Verified: `PushItem` saves the first owned count and replaces only the current count;
`PopItem` restores it once. Repeated pushes keep the first saved word, and subsequent
pops return the current count. Ordinary item use requires a signed-positive current
count, so high-bit replacement words fail ownership before item actions and
script/drop effects. See preservation
[item-use eligibility](https://github.com/michidk/redguard-preservation/blob/main/docs/config/item-ini.md#using-an-item).
Session snapshots retain the saved word and pushed flag; older snapshots start
without pushed counts. Closed item processing skips lookup. This follows preservation
[PushItem](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#pushitem-saved-value-replacement)
and [PopItem](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#popitem-saved-value-restoration).

Verified: `SubItem` applies signed count subtraction without drop flags, dependent
removal or selection changes. Its wrapping normalization preserves the signed
minimum operand, and closed item processing skips lookup. This follows
preservation's [subtraction contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#subitem-signed-subtraction).

Verified: `SpacePressed` reads the fixed Space key from the shared script input
sample without consuming it. Repeated calls remain true while Space is held,
and release clears the result. The common input sampler owns suspension. This
follows preservation's [Space query contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#spacepressed).

Verified: `IsLightingTorch`, `IsUnlightingTorch` and `IsLightingOrUnlighting`
query the supplied receiver after inventory, control and shell gates. Action state
precedes animation-group fallback, and the combined query tests lighting first.
The queries leave receiver state unchanged, following preservation's
[torch predicate contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#islightingorunlighting).

Verified: direct `MoveMarker`, `MoveLocation` and `MovePosition` steps release
hanging selected players through the shared ledge transition before retaining
phase zero and lifecycle selector seven. Nonselected hanging receivers retain
the pending move without selected-player services. Falling and queued motion
bypass release. Synchronous sound failures retain the release transition's
partial state. These rules follow preservation's
[direct point contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#direct-point-moves-movemarker-movelocation-moveposition)
and [ledge release](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/player.md#ledge-hang-release).

Verified: node-point movement with motion direction 15 runs the selected player's
shared ledge update before retaining its falling/queued pending exit. Player type
one and other receivers skip the ledge service. The held next node still chooses
pending phase one or zero. This follows preservation's point-entry contract;
ledge transition failures precede pending lifecycle and collision writes.


Verified: armed attacks blocked by a protected actor suppress opponent-unarmed
taunts while the retained shared dialogue-cache entry remembers a sample,
including a stopped channel. Clearing that sample allows ordered taunt selection
again. The check precedes random selection and presentation, following
preservation's `engine/player.md` armed attack input contract.

Public property dispatch routes motion, action, response and AI fields to the
receiver's owning state. Byte writes retain the low eight bits; signed word
properties retain the low sixteen bits. `LastCollide` accesses only the response
word's low byte and preserves its other marks. `Sflags` uses model collision
metadata independently of `DisableScript`; missing owners fail explicitly.
Verified: the field contracts follow preservation's
[SOUP properties](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUPPROPS.md).

`GotoLocation` composes receiver location lookup, arrival admission and the
retained AI selector/location/delay refresh through the native task caller.
Its explicit refresh does not enable autonomous selector-four movement or add
an ordinary AI timer stage. Automatic caller placement and steering remain
unsupported until their complete contracts are established.

Verified: `IsBouncing` reads the receiving actor's motion state and returns one
for states 27 through 32 inclusive, following preservation's
[query contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#isbouncing).
It requires no selected-player, animation or geometry service.

`MovePos` reads receiver position and playback speed through the public native
caller and returns the horizontal proximity predicate without moving the actor.
Verified: refusal ordering and wrapping time-scaled thresholds follow preservation's
[MovePos contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#movepos).
This predicate alone does not admit automatic selector-four progress.

Verified: the explicit mode-eleven progress service composes point facing, the
strict heading boundary, the shared `MovePos` predicate, synchronous forward
movement and pending-position registration before retained-candidate clearance.
Slide state and the full collision-response word control mode/delay reset, per
preservation's [progress contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#ai-mode-eleven-progress).
Unknown: automatic selector-four admission and scheduling. The explicit service
requires an already admitted mode-eleven receiver and does not complete its task.

Verified: the public AI destination-clearance query consumes retained collision
candidates with polygon sidedness disabled. Ordinary collection uses local actor
position even when clearance resolves an attached or linked position, following preservation's
[clearance contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#ai-destination-clearance).
It leaves AI state and node reservations to its caller. The point-movement
visibility query keeps its separate polygon-sidedness behavior.

Verified: `GetMovementSpeed` reads the receiving actor's current animation group
and returns its unscaled movement attribute byte, following preservation's
[query contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#getmovementspeed).
Absent animation and groups outside that mapping return zero.

Verified: the ordinary ground probe follows preservation's
[terrain sink recovery](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/player.md#terrain-sink-recovery)
order: terrain recovery, actor bounce, then queued motion before support sampling.
Terrain recovery is inhibited during a camera transition. Actor-bounce states
retain their support across ordinary updates; handled bounce or queued motion
ends the probe before support sampling.

Verified: selected-player walk-off recovery restores the retained stable position
and enters the selected jump service before forcing its recovery group and running
queued arbitration. It uses the same selected edge gates as ordinary jump entry.

Verified: ordinary falling resolves and stores the regained support's surface
before landing damage. Water surfaces suppress fall damage; dry support retains
the configured hurt and fatal thresholds, following preservation's
[ordinary landing decision](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/player.md#ordinary-landing-decision).

The [script module documentation](src/script/mod.rs) describes SOUP state
ownership, host services and internal responsibilities. Hosts integrate through
`ScriptPlugin`, `ScriptInputs` and `run_ordinary`; unavailable services return
contextual errors. Explicit script operations do not establish automatic
scheduler admission or complete gameplay support. The zero-argument `Jump` native
completes immediately after admission and its state writes; selected-player
edge and hang services use the caller's receiver-bound collision geometry.
Its contract follows preservation's `player.md#script-jump`.

Verified: a nonselected receiver in hanging state 15
clears its stable-position-valid flag and completes without starting motion;
selected-player hang release retains its own admission gates.

Verified: the pre-script queued/falling stage carries backward motion 3 through
ordinary backward displacement, including startup counters, animation suppression,
stance admission and the exact queued-3 retained heading. It shares the existing
translation arithmetic, following preservation's
[backward displacement contract](https://github.com/michidk/redguard-preservation/blob/ca1fe612a5aa024206dfa0ee8d1a3c4ff0eb81cc/docs/engine/player.md#pre-script-queued-and-falling-stage).

Verified: refused scripted directional steps retain body-response marks for
forward and sideways movement, and a transition mark for backward movement.
Player alignment consumes these marks when choosing its fallback. The auxiliary
response bytes remain separate, following preservation's
[directional refusal contract](https://github.com/michidk/redguard-preservation/pull/153).

Verified: ordinary actor and actor-style static overlap give the selected player
zero mobility while the shared alignment result or lockout word is nonzero,
without changing its weight attribute. A nonzero alignment result also preserves
its current position on rollback; lockout alone restores its previous position.
The actor-pair override applies in either role, after small-contact handling,
following preservation's [weighted overlap contract](https://github.com/michidk/redguard-preservation/blob/d86699d/docs/engine/player.md#weighted-actor-overlap-response).

Installed `multitask PlayAnimation(10, 0)` calls retain a native animation request
across main-thread loops and session restore. Function-form requests complete
immediately. Other task-form operands remain explicitly unsupported.
Verified: the installed BAT slot retention follows preservation's
[PlayAnimation contract](https://github.com/michidk/redguard-preservation/blob/8bfcb816d92d83df285ca553136f0bba2213185b/docs/engine/SOUP.md#playanimation).

Created-object records (including projectile records) retain their creator,
namespace and execution order through snapshots. `KillMe` removes their
executable state before a later created-object update can resume its main thread.
Verified: a killed created receiver cannot resume after its cancelled PlayerLineUp
or publish completion, following preservation's
[killed-object removal order](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#killed-object-removal).
Allocator recycling and reuse across a long live session
still require installed original/native/browser acceptance; the current public
tests do not claim that parity.

Verified: `KillMe` clears both body and equipped-weapon presentation. Later model
and weapon publication skips killed receivers. `Resurrect`
does not republish the removed geometry, following preservation's
[lifecycle contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#killme-and-resurrect).

Named lookup retains the first namespace registration after a created receiver
loses its executable runtime. A lookup of that killed receiver reports an explicit
unsupported error, including after restore; it does not select a later duplicate
or expose the retained pose as live behavior. The namespace lifetime follows
preservation's [registry contract](https://github.com/michidk/redguard-preservation/pull/160).
The original's result when dereferencing the freed record remains Unknown.
Direct actor-handle reads, mutations and execution also reject a retired created
receiver. Logical variable targets use the same lifetime check, including after
restore. Pending-kill receivers remain accessible until created-object cleanup;
killed map records remain addressable. Retained records stay available to bounded
diagnostics and snapshot storage. Logical variable identifiers do not establish
parity with the original's raw actor references.
Restore rejects a created executable saved without runtime and without its killed
flag, before returning the replacement world. Retained lineup task references also
reject retired created receivers before replacement; allocated live and pending-kill
references remain valid.

Verified: visible created objects maintain retained lights and flats before script
dispatch, following preservation's [ordinary maintenance contract](https://github.com/michidk/redguard-preservation/pull/160).
Script-created effects first advance on the next eligible update; hidden created
objects retain their phases without consuming modulation random values.

Created-object updates discard an already killed receiver's executable runtime.
A receiver killed by its ordinary hook also skips its main program. Saved killed
continuations obey the same exclusion after restoration, following preservation's
[created-object lifecycle](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#removal-of-killed-created-objects).

Automatic selected-player weapon validation also releases the retained attachment
when its item is no longer an owned weapon, so rendering cannot keep the old sword.

Projectile endpoint contact selects the target's initialized body spheres.
Verified: `attr_no_sphere_object` suppresses those spheres. Vertex-bound RAVC volumes belong to
a separate overlap owner and do not block projectile contact. The optional RAFS
descriptors are absent from installed maps; their distinction is documented in
preservation's [sphere data contract](https://github.com/michidk/redguard-preservation/blob/main/docs/formats/RGM.md#rafs-actor-fsphere-data).
Verified: reaction/death class 2 selects the tertiary MPSZ body when combat state
and that body exist; class 1 and absent tertiary bodies retain the ordinary shape,
following preservation's [contact-body selection](https://github.com/michidk/redguard-preservation/blob/786f6c6/docs/engine/player.md#contact-body-shape-selection).
Live installed impact parity remains unverified.

Verified: revisiting a retained world restores its logical actors and script
continuations, then places both player records at the destination marker, following
preservation's [player entry contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/player.md#initial-player-animation-setup).
Whole-session restoration retains the saved player pose.

Session and retained-world restoration validate saved `PlayerLineUp` references
before returning a replacement. An absent actor or the currently selected player
cannot be restored as the retained reference. Valid references keep their saved
actor index across generation replacement, following preservation's
[alignment ownership contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#player-alignment-ownership-and-targets).

Verified: logical sessions retain the selected player's rope jump latch separately
from per-world rope records, following preservation's
[saved global owner contract](https://github.com/michidk/redguard-preservation/blob/8f692dd2e2c9a9ea86a20a7eb74c7acac7cef39f/docs/formats/SAVEGAME.md#rope-jump-latch).
A legacy session with an active selected-player rope attachment and no latch is
rejected before replacement; a legacy detached session defaults the latch to false.
Temporary world revisits do not restore this session owner. Jump release remembers
the released rope for its cooldown even after restoring an attachment. Other rope
controller history and complete original load-policy parity remain unverified.

Verified: actor prelighting seals nonmoving static actors after fresh or retained
entry initialization and before ordinary updates. It retains the resolved static
model, integer map-unit translation and model-owned floating matrix. Snapshots
rebuild that checkpoint from restored owners, following preservation's
[entry checkpoint contract](https://github.com/michidk/redguard-preservation/blob/d500b0e/docs/engine/actor-prelighting.md).
Missing legacy transform owners produce a contextual error instead of a guessed
lighting pose.

Automatic world scheduling prepares its logical environmental-effect pool from
matching installed environment metadata before entry/model work. Pending metadata
pauses model work, the simulation clock and the player idle counter; GPU readiness
does not control logical preparation. Existing live or restored pools retain their
slots. Explicit low-level callers supply their own effect service.

Authored object-light projection resolves its offset through the current retained
actor pose. Snapshot material preparation uses the same projection without
advancing scripts or refreshing families.

Verified: retained light modulation follows preservation's
[modulation maintenance contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/model-lighting.md#modulation-maintenance).
Eligible ordinary actors maintain lights and flats before animation and script
dispatch; separate objects maintain lights after script dispatch. Positive light
amplitudes consume the shared session random stream in intensity-then-radius
order. Enabled records with negative amplitudes after low-word conversion remain
explicitly unsupported and fail before light maintenance changes state or consumes
random values. Disabled records retain the inactivity cleanup path.
Snapshots retain each channel's phase; the session retains the shared random state.
Verified: `FxPhase` interpolates the receiver's current RGB words through retained
native task scratch, following preservation's
[phase contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#fxphase).
Concurrent tasks retain separate accumulators and directions across world snapshots.
A zero span still creates the record and reports completion. The initial call
retains its task slot until the next task pass applies completion.
Signed division overflow is explicitly unsupported. The native task slots retain
four words; the separate elapsed/point value is refreshed before operand evaluation.

Verified: `FxFlickerDim` subtracts one shared random draw from the receiver's
starting RGB words, wraps each result to a signed word, and clamps below -255,
following preservation's
[colour modulation contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#fxflickerdim).
Authored type-six lights retain their starting colour independently of current
phase/flicker output. Snapshots validate this base against its authored source.
Dynamic/type-four records and older snapshots without an established base remain
explicitly unsupported for dimming. Zero and minimum signed arguments also fail
without consuming random state instead of entering the original's reduction loop.

Verified: `FxLightSize` configures the same radius-modulation channel as `LightSize`,
keeps the low sixteen bits of both arguments and resets its phase without enabling
the record. Its multitask call finishes immediately without retaining a task slot,
following preservation's [effect contract](https://github.com/michidk/redguard-preservation/blob/6d97244c6c89269508f5ec2e7c39aebd4cbc5b8d/docs/engine/SOUP.md#fxlightsize).
Verified: `FxFlickerOnOff` retains a multitask that consumes one shared random draw
on each call. A zero draw toggles current radius between zero and its starting
value, following preservation's [radius toggle contract](https://github.com/michidk/redguard-preservation/blob/c2abc5dab1230e5c71b9a18e484a98f05eddb8a0/docs/engine/SOUP.md#fxflickeronoff).
Normal light maintenance retains its separate radius clamp. Snapshot restoration
preserves the task and receiver record. Nonpositive bounds remain explicitly unsupported; a zero bound enters the
original's nonterminating reduction.
This logical maintenance does not add ordinary actor lights to renderer submission.

Verified: the production SOUP `End` notification releases activation ownership
when the ending actor owns it, following preservation's
[End contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#end-0x05).
The pending prompt and activation holdoff survive this owner release.
Successful activation also consumes the held action input until release, independently
of the owner. Quick-key and armed action handlers honor that mark, following
preservation's [activation input contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/hud.md#compass).

World transitions retain their exit outcome until the destination registry is prepared.
The departed world runs no further ordinary updates or ordinary audio services while
the host loads its destination, following preservation's
[world request contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#session-requests-loadworld-wongame-pushplayer-and-popplayer).

Verified: refused selected-player scripted directional checks retain the directional
blocked response for collision-age maintenance without moving the player, following
preservation's [walking support contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/player.md#ordinary-forward-walk-support-checks).

Verified: selected-player forward running in `anim_run` retains animation-owned
movement after key release until the movement marker or idle transition stops it,
following preservation's [release continuation contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/player.md#continuation-after-input-release).
The per-update called flag prevents a second movement step when the controller
already moved the player.

Ordinary simulation restores axis locks without publishing unattached matrix changes.
The next model-presentation phase refreshes the retained matrix; explicit orientation
operations keep their own refresh behavior. **Verified:** preservation
[ordinary publication contract](https://github.com/michidk/redguard-preservation/pull/155).

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

Verified: point route initialization preserves current and pending animation playback,
so walking can continue while a new route is selected. The animation-stop operation
leaves playback unchanged, following preservation
[point initialization](https://github.com/michidk/redguard-preservation/pull/156).

Verified: held-node facing turns toward the node without the coordinate-facing
arrival snap, preserving the final approach heading before walking claims a node.

Verified: accepted compass item use consumes the shared HUD press, so the later
HUD input pass preserves the toggle. Releasing the action key rearms it,
following preservation's [compass contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/hud.md#compass).

Verified: slope turns publish the selected player's orientation matrix and angle history
immediately through the shared turn service, following preservation's
[ordinary slope response](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/player.md#ordinary-slope-response).

Ordinary hosts can read literal RTX-family labels from the prepared script call
table before simulation begins. This includes dormant script definitions and
leaves actor tasks, random state, dialogue records and sample allocation unchanged.
Dynamic label operands are resolved only when their script calls execute.

Verified: hanging state 15 uses the dedicated ledge input branch. Other ledge
states retain ordinary directional helper dispatch, allowing forward animation
requests to remain queued during a climb without ordinary translation, following
preservation
[controller dispatch](https://github.com/michidk/redguard-preservation/blob/b1111cac9dc49f63e505373ecb39bbbe64dc768e/docs/engine/player.md#ordinary-controller-dispatch).

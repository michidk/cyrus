# cyrus-script

Game scripting host, player input and deterministic controller rules, the shared
simulation clock and the in-game screens. It runs programs from `cyrus-soup`
against the world, collision and asset crates and uses `cyrus-game` for fidelity,
preferences, cheats, the random stream and application state. Hosts own installation
access, rendering, cameras, audio playback, storage and external control.

## Integration

Verified: `drno` suspends the entire ordinary update of nonplayer actors, including
response timers, movement and combat. The selected player and camera continue;
turning it off resumes the retained actor state. This follows preservation's
[cheat update rules](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/cheats.md).

Verified: a nonzero weapon-holding attribute admits the ordinary armed controller.
The held walk modifier selects armed walking; the retained walk-mode toggle does
not change armed dispatch. This follows preservation's
[controller dispatch](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/player.md#ordinary-controller-dispatch)
and [weapon predicate](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#playerarmed).

Verified: the base selected-player controller rebuilds banked orientations, following
preservation's [player orientation rules](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/player.md#upright-actor-orientation).
The transition back to zero bank also rebuilds so the last banked matrix cannot retain
tilt. Upright player turns and ordinary nonplayer actors retain incremental rotation.

Verified: held-direction turn acceleration updates during rope attachment even while
ground movement and collision are bypassed. Releasing a direction clears its retained
speed before ordinary movement resumes. This follows preservation's
[control-animation input contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#pushanimation-and-taskstop-animation-ownership).

Verified: `XResetCamera` releases the selected player's script, hook, task, object
and combat inhibition along with its tasks. Falling and queued motion remain owned
by the ordinary controller and collision response. Unrelated control bits survive.
This follows preservation's [camera reset contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#xresetcamera).

Verified: directional motion publishes direction and movement counters without an
animation receiver. Only a current animation's suppression flag prevents translation;
stored deep-water surfaces still halve the distance. This follows preservation's
[directional displacement rules](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/player.md#directional-ordinary-displacement).

Verified: static-model ground response uses `FallDamage` and `FallHold`, independently
of `Collide`. Animated actors retain collision-mask admission. Static models run
the support/ground stage without the animated obstacle and overlap passes, following
preservation's [ground-stage rules](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/player.md#ground-stage-by-model-kind).

Verified: falling static models retain the full support probe distance and do not
require a measured FPS sample. The shortened high-FPS probe belongs to animated
falling actors, following preservation's
[vertical-motion rules](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/player.md#queued-and-falling-vertical-motion). Undamaging static
landings clear falling and publish support height without an animation receiver;
only the animation request is skipped, following the shared
[request receiver gate](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#playanimation).

`GamePlugin` installs the `cyrus-game` state (application state, fidelity, preferences,
cheats, random stream), `PlayerPlugin`, `ScriptPlugin` and the console commands.
The host supplies Bevy asset/input/state infrastructure and `Preferences` with
a concrete `PreferenceStore`. Preferences use original action units; successful
persistence precedes runtime application. Fidelity areas default to Faithful.

Player transforms remain independent of camera transforms. Inspection adapters
set `InputSuppressed` before `InputSample`. Menu, console, logbook and inventory screen suspension
must also suspend the shared simulation clock and automatic gameplay input.
Marker placement waits from the replacement request through host preparation, so
the destination marker never changes the retiring player's pose or support state.

Mods can request orbit-camera yaw through `CameraOrbit`; manual mouse/right-stick
look takes priority over its relative turn. The request does not take over the camera
pose or suppress player movement. With no mod request, faithful camera behavior remains
unchanged.

Verified: mod-claimed facing rebuilds the model orientation from its absolute angles,
so successive yaw and running-bank changes cannot accumulate a sideways tilt. Claimed
airborne updates relax the running bank toward zero. The original controller retains
its incremental rotation path.

The logbook reader owns its manual-entry dialog state and keyboard focus. Installed action C
opens an empty byte editor; presentation reads `LogbookScreen::manual_entry()` and
`manual_entry_cursor_visible()`. Acceptance appends a style-9 journal record through the
existing host store before refreshing the reader at newest text. Cancellation leaves the
journal unchanged. Editing retains the original 64-byte cap, ASCII uppercasing and shifted
CR/VT controls. Cursor phase starts hidden and toggles only on changed sampled BIOS ticks
divisible by six, without replaying skipped ticks. These rules follow preservation's manual
logbook contract. Native and browser hosts share the editor and persisted journal owner.

Verified: inventory publishes a draw pulse on each changed BIOS tick after opening
release, including the exit-request update. Missed ticks admit one draw; release
waits admit none. Presentation uses this pulse for the shared candle animation.

Verified: inventory closing waits for Inventory and Escape release before sampling
Activate or Enter once to commit the cursor selection. It then waits for Walk,
Activate and Enter release in order. Earlier waits do not restart when their keys
are pressed again. The candle timer clears after the first two waits, before selection is sampled.
The shared modal lock and paused virtual time remain held until the final wait
finishes. Selecting an item does not use it. This follows preservation's
[inventory exit contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/hud.md#exit).

Verified: item use shares the receiver torch-transition predicate. Action states
33/34 refuse use only with an animation owner; without one, they do not block
ordinary inventory effects. Explicit use retains an attached rope. These gates
follow preservation's `IsLightingOrUnlighting` and item-use contracts.

Verified: `ItemUsed` reads a wrapping completion counter independently of item
ownership. Completed use, including compass use, increments it after script and
drop effects. Refusal and burning-torch dousing leave it unchanged. New inventories
start at zero; session snapshots preserve the word. Legacy snapshots without use
history retain an unavailable counter instead of inventing past uses. This follows
preservation's [ItemUsed contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#itemused).

Verified: ordinary collision admits a rope terminal from the published camera position
sampled when the map window rebuilds. It uses the rope base in all three axes and excludes
a Manhattan distance of 4096 map units or more. Preload keeps all terminals available.
World snapshots retain the sampled position so a same-cell restore keeps the same candidates.
The same retained membership gates rope amplitude service, swing and attachment climbs.
Excluded ropes preserve their dynamic state and resume on the first admitted update.
Verified: link and terminal geometry retains the incoming swing inputs published before
amplitude service and oscillator advancement. Collision bounds, face transforms, attachment
placement and rendering read that publication. Snapshots retain it independently of the
advanced oscillator, following preservation's [rope update order](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#rope-update-order).
This follows preservation's [transient collision rules](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/player.md#live-and-transient-collision-candidates).

Verified: admission records the attachment without placing a pending grab.
Subsequent attached updates place the player. A player retains the approach height while the rope grab
animation's wrapping frame progress is below three. Horizontal hand placement
continues. Admission chooses the nearest eligible link from the actor position plus the authored
hand vertex, without a link-length offset. Later placement uses that node and grip height. The public rope
drivers have an explicit unarmed positive scenario requiring grab, held attachment
and jump detach; diagnostic timing does not establish contact/clank parity.
Verified: jump release admits ordinary movement and collision on that update after removing the
attachment. The retained rope jump latch protects the regrab countdown while falling with Jump held.
Carried displacement reads the retained speed before attached controls run; a directional release
therefore first carries its new speed on the following update. Attachment admission follows the
ordinary controller, so release and repeated jump updates advance the regrab countdown on that
same update. The countdown pauses when both queued and falling motion are absent. This follows
preservation's [rope update order](https://github.com/michidk/redguard-preservation/blob/fd87586bee7ab2bfeffc0d96b8d91e3c1eabff3c/docs/engine/SOUP.md#rope-attachment).

## Module guide

| Module | Responsibility |
| --- | --- |
| [player](src/player.rs) | Player scene preparation, held input, controller rules and logical movement |
| [script](src/script/mod.rs) | Game-owned SOUP hosts, logical actors and ordinary updates |
| [clock](src/clock.rs) | Shared simulation timing and validated timing snapshots |
| [logbook](src/logbook.rs) | Shared reading-screen state and input |
| [inventory_screen](src/inventory_screen.rs) | Inventory screen list, cursor, layout and lifecycle |
| [console](src/console.rs) | Console focus, editor and gameplay command registration |

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

Verified: `PressKey` applies `InKey`'s letter and modifier rules and consumes the key it
reports: a held key answers once and then reads as not held until it is released and pressed
again, while a matching latest event keeps answering and a failed modifier check consumes
nothing. The consumed letters are input state on the script registry, not saved gameplay
state. Unknown: whether the original sets the key state again on keyboard repeat. This follows
preservation's [PressKey contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#presskey).

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
Verified: its pending task suppresses the ordinary native hook while the task
system is enabled, following preservation's
[script-stage contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#ordinary-update-gates).
Ordinary hook/main updates retain mode eleven without admitting direct movement.
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

Verified: explicit direct-destination admission tests retained clearance only for
a receiver outside mode eleven with zero delay. A clear destination releases
held walk nodes before selecting mode eleven, following preservation's
[admission contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#ai-destination-clearance).
It leaves arrival checks, further progress and scheduling to its caller.

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
Grounded forward walking does not continue while queued jump or falling motion is
active; the ordinary airborne response retains ownership until landing.
The installed Modern keyboard regression checks standing, walking and running
jumps through release and landing.

Ordinary simulation restores axis locks without publishing unattached matrix changes.
The next model-presentation phase refreshes the retained matrix; explicit orientation
operations keep their own refresh behavior. **Verified:** preservation
[ordinary publication contract](https://github.com/michidk/redguard-preservation/pull/155).

[SOUP #4](https://github.com/michidk/cyrus/issues/4) tracks remaining work and acceptance.

## Verification

Run the narrowest relevant suite during development, for example:

```sh
cargo test -p cyrus-script --lib
cargo test -p cyrus-script --test script_registry
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

Verified: `AttachMe(group)` dispatches ordinary non-master receivers. Zero and the
current slave/master identity return zero without spatial state. Other targets
detach first, then compare the full signed argument against master bytes. A
unique target converts the receiver into its frame, retaining the effective
pose until the next family update. The current `NoSlaveRot` attribute controls
angle and matrix conversion. Missing targets retain detach effects. This follows
preservation's [AttachMe contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#attachme).
Master receivers and duplicate target masters remain explicitly unsupported
because separate ordinary/master memberships and original tree traversal order
are not represented. Original-instruction witnesses cover control flow with
controlled matrix services; natural native/browser gameplay is not established.

Verified: the installed `address` property yields the resolved actor identity for
Me, Named, the selected Player and Camera without requiring spatial state. The
VM preserves it through local copies, native operands and object-local targets.
These targets validate generation and reject retired created actors instead of
resolving `MyId` through namespaces. World restore explicitly rebinds saved
reference locals; malformed saved identities fail before replacing the world.
This follows preservation's [reference encoding](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#reference-name-encoding).
Verified: `WaitLineUp(actor)` resolves its typed actor argument and stays pending while mask 2
of that actor's alignment flag (script-control mask 2, set by the player lineup natives) is set,
completing on the task pass after the flag clears; it changes no actor and survives a restore.
An actor without a control record reads clear (inferred). References
in arithmetic or global flags, raw pointer integers, temporary separate-actor
addresses and pool-slot reuse remain unsupported; no numeric host-address encoding is exposed.

Verified: `OffsetLocation(source, x, y, z)` accepts a typed actor reference, resolves
source position before receiver position, transforms the offset with the source
actor matrix and writes wrapping sums only to the receiver local position. It
returns zero without updating history, registering a map update or publishing
camera movement. Captured references survive generation rebinding on restore;
retired sources fail without advancing the caller. Position selection retains
slave alternate and linked-position precedence. Explicit SetRotation and
ResetRotation rebuilds publish the attached source matrix immediately, retaining
its resolved position until the next family refresh. Matrix conversion uses the
existing nearest-even transform contract. This follows preservation's
[OffsetLocation contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#offsetlocation).
Original instruction witnesses and public native/browser synthetic callers cover
the bounded behavior; natural authored gameplay parity remains Unknown.

Verified: `PlayerTilt(angle, strength, divisor)` runs as a persistent multitask
through direct and routed script calls. It reads the selected player's retained
support actor, total receiver X/Z bounds, effective bearing position and local
strength distance on each invocation. Whole nominal ticks scale its signed
snap/add steps; successful changes rebuild orientation and angle history.
The contract is published in preservation's
[PlayerTilt reference](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#playertilt-receiver-orientation).
Signed angle sums, history and save/restore retain the full 32-bit value. Rotation
mode one uses the shared Euler builder; other modes use the ordinary orientation
builder. Slave receivers report original diagnostic 137 before changing the pose; original
divide faults also remain explicit errors. The original diagnostic console recovery
mode is unavailable. Original instruction witnesses and synthetic public native/browser
callers bound acceptance; natural installed gameplay parity remains Unknown.

Verified: `DeleteBroadcast()` advances only the receiver's broadcast selection
through direct and routed VM calls and returns zero. It uses channel zero of the
retained messaging context, independently of the actor's class-message cursor,
and leaves message entries and senders unchanged. Actor snapshots preserve the
full selection byte and default older saves to zero; restoring a world retains
the message writer and rebinds saved actor identities. The contract follows
preservation's [broadcast selection advance](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#deletebroadcast-selection-advance)
and [messaging lifecycle](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#messaging-context-lifecycle).
The production owner always has its initialized eight-entry channels. Missing
context diagnostic recovery is unavailable. Synthetic public callers bound
acceptance; natural installed gameplay parity remains Unknown.

Verified: `GetBroadcastSender()` returns the sender of the receiver's selected broadcast
entry (channel zero, at the broadcast selection) as a typed actor reference through the
host's actor result, or zero when nothing is selected or the entry was never written. It
consumes and changes nothing and ignores the class-message cursor. The reference is returned
as stored, not checked against the live world; a selection outside the eight-entry queue is a
contextual error. The contract follows preservation's
[broadcast sender pointer query](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#getbroadcastsender-query).
Only combat messages reach channel zero until the broadcast send natives exist; natural
installed gameplay parity remains Unknown.

Verified: `ReceiveBroadcast()` applies the `ReceiveMessage` admission rule to the receiver's
broadcast selection over channel zero. It returns zero with no selection; otherwise it skips
entries addressed to another receiver by advancing only the broadcast selection (wrapping at the
eight-entry limit) and returns the first entry addressed to the receiver or to no one with its
value unchanged, leaving that entry selected. A selected slot never written is accepted with
value zero. Entries, senders and the class-message cursor are untouched, and a selection outside
the queue is a contextual error before any change. The contract follows preservation's
[ReceiveBroadcast entry selection](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#receivebroadcast-entry-selection).
Natural installed gameplay parity remains Unknown.

Verified: `GetBroadcastSenderId()` returns the signed 16-bit runtime identifier of the sender of the
receiver's selected broadcast entry (channel zero), or zero with no selection, consuming and
changing nothing. The original reads the sender without a null, bound or liveness check; here a
selected slot never written, a selection outside the queue and a sender from a replaced world are
contextual errors. The contract follows preservation's
[broadcast sender identifier query](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#getbroadcastsenderid-query).
Natural installed gameplay parity remains Unknown.

Verified: `IsBroadcastWaiting()` returns one exactly when the receiver's broadcast selection
differs from channel zero's no-selection index, and zero otherwise; it scans, consumes and changes
nothing. A selection outside the queue is a contextual error. The contract follows preservation's
[broadcast availability query](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#broadcast-availability-query).
Natural installed gameplay parity remains Unknown.

Verified: `SendBroadcast(value)` appends an entry to channel zero, the queue every receiver's
broadcast selection reads: the calling actor is the sender, there is no recipient and the value is
stored unchanged; the write cursor then advances, wrapping at the eight-entry limit, and the newly
indexed slot is cleared. A zero value returns before anything is read or written. It shares the
writer with `SendMessage`, so class-zero message entries and broadcasts live in one queue. The
contract follows preservation's
[SendBroadcast entry write](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#sendbroadcast-entry-write).
Natural installed gameplay parity remains Unknown.

Verified: `ReplyBroadcastSender(value)` answers the sender of the receiver's selected broadcast entry
on channel zero: unless the value is zero, nothing is selected, or the selected sender is null or
the receiver itself, it appends an entry with the receiver as sender, that sender as recipient and
the value unchanged, advancing the write cursor (wrap at the eight-entry limit) and clearing the
newly indexed slot. The receiver's own broadcast selection does not move. It shares the writer
with `SendBroadcast`. The contract follows preservation's
[ReplyBroadcastSender entry write](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#replybroadcastsender-entry-write).
Natural installed gameplay parity remains Unknown.

Verified: `DistanceFromLocation(location)` returns the length from the receiver's current local
coordinates to the selected location, selecting as `DistanceToLocation` does (a negative or
oversized selector selects the start) and converting through the shared mixed-precision helper:
an out-of-range length returns the signed minimum rather than failing. The selector equal to the
location count is a contextual error because the original reads past the table. The contract
follows preservation's
[location selection](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#distancefromlocation-location-selection-and-distance-helper-boundary)
and [numerical helper](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#distancefromlocation-numerical-distance-helper).
Natural installed gameplay parity remains Unknown.

Verified: `AtPos(x, y, z)` shares the ordinary arrival helper with `GotoLocation`: the target and the
receiver's position shift right eight bits, the receiver's positive Y extent is added to its height,
and the receiver arrives when the rounded distance is strictly below its movement speed times 384
shifted right eight bits. Arrival stores the target with its low eight bits cleared (Y included),
registers the receiver in the changed-actor list and returns one; otherwise nothing changes and the
result is zero. A named receiver is moved while the caller continues. The list holds each actor
once and its capacity is the installed `max_remap_objects` (at least 32); the first actor past it
fails with the original's error 196 after its position store. Every registration site (task movers, late
placement, arrival, bounce, ledge and lineup movers) registers through one list owner that raises
the error inside the registering call, after the call's own position stores; the installed
`attempt_recover=no` makes the original terminate there, so nothing after the call runs. The contract follows preservation's
[AtPos position arrival](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#atpos-position-arrival)
and [changed-actor list](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#changed-actor-list).
Natural installed gameplay parity remains Unknown.

Verified: `MoveAboveMe(x, y, z, angle)` places its receiver relative to the shared native-command source
(the actor of the most recent ordinary interpreter entry, whoever the receiver is): the source position, resolved as
attached, linked or local, plus the offset turned by the source's matrix, lowered by the receiver's
positive Y extent and raised by the installed `standing_height` shifted left eight bits. The
selected player receives the pending placement that `MovePlayerAboveMe` uses; any other receiver is
written directly with `angle & 2047` as its yaw and registered in the changed-actor list. A slave
source reports the original's diagnostic 137 before anything changes. The contract follows
preservation's [MoveAboveMe](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#moveaboveme).
Natural installed gameplay parity remains Unknown.

Verified: `PointAt()` turns the receiver to face the shared native-command source (the actor of the most
recent ordinary interpreter entry); a call on the source itself has no difference. The yaw is the original's bearing of the
truncated differences between the two actors' stored model coordinates, written directly to the
receiver's yaw without a matrix refresh or animation request; a call on the running actor itself
has no difference and yaws to zero. The contract follows preservation's
[PointAt](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#pointat-native-boundary).
Natural installed gameplay parity remains Unknown.

Verified: `PrintParms` and `PrintStringParm` print the receiver's script name and their arguments
(eight signed decimals, or the evaluated string) on the first free console line, and `LogParms`
writes the same fields to the engine log at debug level; each returns zero and changes no gameplay
state. The contract follows preservation's
[parameter diagnostics](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#parameter-diagnostics).
The original's unresolved-handle text and 255-byte line bound have no representation here;
natural installed gameplay parity remains Unknown.

Verified: the receiver's AI record is script-readable and writable as the properties `ai_var1` (the
location word `GotoLocation` stores), `ai_var2` and `ai_length` (the two words read sign-extended and
keep the low sixteen bits of a write; a fresh actor reads -1 for `ai_var1` and zero for the rest; the
record is saved with the actor), and `node`, `dnode` and `fdnode` read and write the walk-map
owner's held current, next and final-destination node indices (`ActorNavigation`), so scripts see
what the walk code holds. `HeaderHook()` prints the banner, a description line
(identifier, object name, script) and the node/AI line on the console and returns one without
changing the receiver. The record address in the description line is the pool slot here, the
object name is the actor's namespace, and no AI native writes `ai_var2` or `ai_length` yet; `node_map` stays
unsupported. The contract follows preservation's
[AI record properties](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUPPROPS.md#ai-record-properties)
and [HeaderHook diagnostics](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#headerhook-diagnostics).
Natural installed gameplay parity remains Unknown.

Verified: `message_index` and `broadcast_index` read and write the receiver's message and broadcast
selection bytes (unsigned, low eight bits kept) over the cursors the message and broadcast natives
use, and any byte now survives a save: the natives report a selection outside the eight-entry queue
when they use it. See preservation's
[message selection properties](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUPPROPS.md#message-selection-properties).

Verified: `last_pos.*`, `start_pos.*` and `last_angle.*` read and write whole 32-bit values of the
previous position (what `SetPosition` stores with the current one), the saved start position
(what `DistanceFromStart` measures against) and the angle history (what `SetRotation` stores), with no
matrix refresh. See preservation's
[position and angle history properties](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUPPROPS.md#position-and-angle-history-properties).
Verified: `start_angle.*` reads and writes the whole 32-bit creation angles ResetRotation restores
(creation, player placement and the player swap set them with the angle history; SetRotation does not).
`last_valid_flag` and `last_valid_pos.*` are the stable-position record the ordinary recovery restores
from while the flag is set: the flag reads 0 or 1 and a write sets it when its low byte is nonzero, and
the position is whole 32-bit values. See preservation's
[stable-position and creation-angle properties](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUPPROPS.md#stable-position-and-creation-angle-properties).
Verified: `move.*` are the retained motion triple (whole 32-bit values); the rope release stores its
jump-off speed in `move.vz` and queued motion 24 reads it, so one word serves both and old saves restore
their separate rope speed into it. See preservation's
[motion triple properties](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUPPROPS.md#motion-triple-properties).
Verified: `slave_pos.*`, `slave_angle.*` and their `last_slave_*` counterparts read and write the pose the
family update composes for an attached actor and the one before it; the queries read it for an attached
actor, so a write is seen until the next refresh replaces it. See preservation's
[slave pose properties](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUPPROPS.md#slave-pose-properties).

Verified: `task_variable*[n]` read and write the four whole 32-bit words task slot `n` retains for its native
handler (continuation, pause countdown, second continuation, packed command word/phase/counter); a write is
what the slot's next resumption loads. See preservation's
[task slot word properties](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUPPROPS.md#task-slot-word-properties).

Verified: `task_status[n]`, `task_pause[n]` and `task_timer[n]` read and write the slot's blocking byte, pause byte
and 16-bit timer, which the interpreter keeps in slot arrays that survive an empty slot and a save (older saves
migrate). The running actor's own slots are served by its interpreter and another actor's through its stored
runtime; a write only changes the field. See preservation's
[task slot control properties](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUPPROPS.md#task-slot-control-properties).
`task[n]` stays unsupported: its call pointers have no portable encoding.

Verified: `anim.next`, `anim.next_speed`, `anim.frame`, `anim.speed`, `anim.speed_count`, `anim.stop_flag`,
`anim.halt_flag`, `anim.hold_flag`, `anim.loop_count`, `anim.rewind_flag` and `anim.real_frame` read and
write the playback record fields of the receiver's `ActorAnimation` (own or another actor's), so a script write
changes the next production advance (an expired countdown steps the cursor, a wait byte holds playback, the
progress byte completes `WaitAnimFrame`) and is saved with the animation. See preservation's
[animation record properties](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUPPROPS.md#animation-record-properties).
`anim.rep_count` (the repeat byte of the ConditionalDelay/LoopControl commands) and the `anim.pos`/`anim.angle` words
(the translation/rotation commands' retained words, cleared at the next cursor step) read and write like the others.
Writing `anim.current` switches the command table and type flags the next advance interprets and keeps
the cursor, countdowns and markers; -1 leaves no current group, and a group without a table or a write
while none is current stops with an explicit error.
`anim.last_angle` is the rotation history the automatic model refresh copies from `anim.angle`.
`anim.max_group` is the low byte of the exported RAHD control prefix (preservation 0.12; sidecars exported earlier
read as an explicit re-export error). `anim.sound_handle`, `anim.address` (and `anim`), `handle`, `last_handle` and `rope_id` stay unsupported on purpose:
a static census of every installed map file (27 maps, the 29 WORLD.INI entries plus the one map no entry names;
`cargo run -p cyrus-assets --example soup_inventory --features native`) finds no script that reads or writes any of them,
and no call of a `Pipe*` native. `rope` is read 8 times, only to test it against zero, which is supported.


Verified: `ZapObject`, `ZapObjectNoKill`, `ZapPlayer` and `ZapPlayerNoKill` adjust health through the same selected-player
additive and death-owning services `AdjustHealth` uses (`ZapPlayer` names its receiver as the damage source, the
object natives gate on attributes 120 and 97 first) and clamp a non-selected receiver's health byte to 0..255; a
non-selected receiver killed by `ZapObject` takes the recovered death dispatch (the armed branch needs a combat record). See
preservation's [Zap natives](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#zap-natives-and-the-selected-player-health-services).
They return no value (the original's results are pointer residuals), so consuming one is an explicit error.

Verified: `dead` (the killed marker `KillMe` sets; a script that writes it is skipped by later ordinary updates) and
`weapon_drawn` read and write the receiver's killed marker and drawn byte, and `invisible` the draw-only byte the renderer
honours for the body (scripts, animation and collision continue); all read as 0 or 1 here and a nonzero low byte sets them. See preservation's
[killed and weapon-drawn properties](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUPPROPS.md#killed-weapon-drawn-and-invisible-properties).

Verified: `id` reads the receiver's signed runtime identifier word (what `MyId` returns); a write is refused because the
identifier is installed identity that name lookup and restore validation key on. See preservation's
[runtime identifier property](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUPPROPS.md#runtime-identifier-property).

Verified: natural slope response saturates the direction's movement counter without
marking a movement-native call. Ordinary cleanup clears an uncalled counter, so
slide-group movement resumes with its startup acceleration after sliding ends.
The installed production test covers natural admission, counter cleanup, presented
pose agreement and continued movement after the slide. See preservation's
[slide-group movement](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/player.md#slide-group-movement).

Verified: a sliding actor's projected primary wall contact preserves its facing and
marks a wall hit. Grounded heading adjustment requires sliding, falling, queued
motion and living state all clear after the motion transition. A sliding actor's
rollback contact retains its attempted position without adding a wall-hit mark.
Public response tests cover both branches and residual separation continuation,
following preservation's [wall-facing rules](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/player.md#grounded-wall-facing-adjustment).

Verified: ordinary primary, residual and destination body queries select vertex-bound RAVC
spheres instead of initial MPSZ spheres when those records exist, following preservation's
[player collision-shape contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/player.md#actor-collision-shapes).
The routed fixture checks displaced vertices, contact restoration and ordinary-update continuation.

Verified: ordinary RAVC body selection reads current animation vertices for animated
models and scene-owned base vertices otherwise. Primary and destination body
queries read the current selection. Residual response retains the spheres copied
when the ordinary query was prepared, together with its copied angles, and refreshes
only its effective origin. A new update prepares a fresh working record. Working
spheres belong to the ephemeral query, not persisted actor bounds. This follows
preservation's per-update collision shape preparation contract.

Verified: ordinary working collision orientation is copied from the current local
angles when the query is prepared. Previous angle history belongs to movement and
rollback and does not supply this copy. Later script rotation changes primary
collision while residual collision retains the prepared orientation until the next
query preparation, following preservation's per-update shape contract.

Current-schema actor snapshots validate map-light eligibility against the installed
placement before restoration. Hidden material preparation and logical publication
therefore use the same light owners; a saved eligibility flag cannot promote or
suppress an installed light role.

Verified: `StopAllSounds` completes its global sample-stop effect as a statement.
It has no supported numeric result; consuming it in a formula fails after the
single effect and retains that failure without replaying the stop. Other immediate
sound functions retain their documented zero results. This follows preservation's
[SOUP sound results](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/sound.md#soup-functions).

Verified: positional `Sound` uses the current `attr_slave` selector, then linked
or local position, through the shared query-position rule. Retained family
membership alone does not select an attached emitter after the attribute clears.
The start retains its captured position; later emitter movement does not update
an already playing `Sound` sample, following preservation's sound contract.

Verified: successful camera-controller and scripted camera pose publication also
updates the borrowed synchronous sample service before returning. The listener
uses the published logical position/yaw independently of render gliding; failed
camera updates do not publish a replacement. Status-only sample adapters may
ignore listener publication.

Verified: fixed gameplay shortcuts select the original book pages with F2–F6,
request version information with F1 and credits with F10. The information screens
retain the live player. F1, book shortcuts and F9 require `final_version`.
Desktop and browser hosts share F9 keyboard eligibility, including console,
reader, inspection and pending-transition gates. Backtick belongs to the debugger.

Verified: scene teardown clears all shared dialogue records and remembered sample
handles, including inactive records initialized with slot zero, before the next
world runs. Presentation traversal resets while the skip-input latch is retained.
This prevents a later skip edge from stopping an unrelated replacement voice.
Periodic maintenance retains its separate active-record cleanup contract. See
preservation's [world teardown rules](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/sound.md#soup-functions).

Verified: selected-player turn queries return zero without a current model, including
after `HideMe`, before reading spatial state. `PlayerMovementSpeed` returns zero
without animation state and otherwise reads the current group independently of
model presence, following preservation
[player motion queries](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#player-movement-turn-speed-arctangent-and-location-distance-queries).

Verified: authored animation sounds waiting for a scene binding obey logical
`EndMySounds`/`KillMe`, effect-specific stops and `StopAllSounds` before admission.
A stopped start cannot reappear when a node is subsequently bound or recreated.
Unrelated owners and requested effects survive; animation Stop/Forget commands
keep their per-actor order. `sound_dispatch::AnimationSoundQueue` owns this
pre-binding routing, while the audio service owns playback and remembered slots.
The stop rules follow preservation's [sample ownership](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/sound.md#soup-functions).

Verified: selected-player water transitions retain forward swimming and exit animation requests
for both running and walking motion, following preservation's
[water transition rules](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/player.md#water-and-lava-transition-effects).
Backward motion keeps its separate requests; transition sound selection is unchanged.

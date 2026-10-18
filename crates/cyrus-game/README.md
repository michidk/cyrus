# cyrus-game

Portable gameplay state, fidelity policy, player input and deterministic controller
rules. Depends on `cyrus-world`, `cyrus-collision`, `cyrus-console` and `cyrus-assets`; it does not load an install,
open sockets, spawn a camera, or play audio.

`GameConsolePlugin` registers cheat handlers with `cyrus-console` and controls
input availability from `AppState`. The private `console::cheats` module owns
cheat argument parsing. The console crate owns editing and command dispatch; game handlers own cheat state and call host-provided storage.
Cheat assignments resolve named SOUP flags against the active world's live values.
Missing or mismatched script metadata blocks symbolic assignments without changing
cheats. Unknown names in a loaded definition report a diagnostic and use one,
following preservation's console rules. `getflag` reads matching live values,
types and latch status. `setflag` assigns values and can mark the latch; omitted
status preserves it. The original zero-status metadata XOR remains unsupported
and is rejected before any flag changes.

`GamePlugin` installs app state, fidelity, `PlayerPlugin` and `ScriptPlugin`. The host supplies
Bevy's asset/input/state infrastructure and `Preferences` with a concrete
`PreferenceStore`. Menu load errors remain errors; each consumer owns its fallback.
Successful saves precede runtime application. `Preferences::menu()` exposes a
read-only result; `save_menu` is the only way to replace accepted menu values.
Preferences and fidelity remain gameplay modules shared by consumers, without
separate crates.

Player scene loading prepares `PlayerDefinition`, terrain and static collision
queries asynchronously from validated asset records. The world request carries a
sidecar path rather than depending on this game-owned asset. Inspection adapters
set `InputSuppressed` before `InputSample`; player transforms remain independent
of camera transforms.

The logical player's mixed-candidate walking stage uses
`begin_player_movement_update` before animation and
`finish_player_movement_update` after late response. The shared update retains
forward acceptance so continuation cannot move twice, and an update without
accepted movement resets startup. The ordinary scheduler supplies Faithful held
input and shared walk mode when their owners are present. Explicit GroundedTick
updates exclude automatic player movement for that update.
**Verified:** an installed ISLAND headless test covers idle, held forward and
release through the ordinary keyboard scheduler, including logical/presentation
pose agreement. Four-surface native/browser comparison remains open.

Ground-turn, walking startup, wall response and forward support admission are
explicit deterministic APIs. Complete actor eligibility remains unsupported.
`player::controller::select_sheathed_action` selects an action for a caller-admitted
ordinary sheathed-ground state. It requires known lockout, dialogue, topic-menu
and walk-toggle values and Faithful held-key input. It returns early dispatch,
idle, forward walk or forward run without mutating the actor. Unsupported
composite inputs report errors. The caller still owns helper admission,
animation continuation and late ground response; the selector does not enable
automatic keyboard updates.
Supported ordinary map actors use the shared simulation clock. See
[SETUP](../../docs/SETUP.md) and [ROADMAP](../../docs/ROADMAP.md).

`SimulationClock::snapshot` captures complete timing, including settings, smoothing,
warmup, measurement remainder, fractional carry and the exposed update step.
Snapshot version 2 also retains the last completed 18-BIOS-tick measurement
interval count. `measurement_intervals` exposes that count independently of
nominal ticks for the admitted normal-mode idle timer. Older snapshots are rejected.
Automatic clock sampling applies these intervals once to an established player
idle counter; suspended menu, console and logbook time does not advance it.
An unknown initial counter remains unknown until a supported reset establishes it.
Fresh `ScriptActors` sessions establish lockout and idle at zero, following
preservation's player initialization rules. Isolated `ScriptWorld::prepare`
does not establish a new session; world replacement retains the session globals.
`from_snapshot` validates it and returns a replacement rebased to the caller's
current BIOS sample without advancing. The partial menu checkpoint stays separate.
**Verified:** serialization/resume contracts cover epoch changes, warmup, smoothing,
wrap/backward samples and rejected malformed state. Saved settings are retained,
following [preservation's saved clock rules](https://github.com/michidk/redguard-preservation/compare/main...docs/save-clock-state).
Original SAV import, whole-world restore and save-point admission remain unsupported.

`ScriptHud::snapshot_journal` captures accepted text, duplicate-tag history and
the pending notification. `Journal::from_snapshot` validates installed RTX
definitions and returns a separate replacement without replaying `AddLog` or
accessing storage. An installed-data journal document has been verified through
native publication and fresh-process reconstruction, including duplicate-tag and
notification continuation. Inventory and complete session restoration remain separate.

`ScriptHud::snapshot` captures inventory, journal, compass state, held-use latch,
power-up slots and replacement order under one lock. `from_snapshot` validates
the components and returns a separate HUD without storage I/O, startup
acquisitions or effects. Unconfigured state and retained initialization or
journal-load errors prevent capture. The aggregate session owner must validate
the installed asset identity before committing a replacement. These component
APIs do not enable player-facing save/load.
**Verified:** an installed-data HUD snapshot survives native publication and
fresh-process reconstruction, retaining the held-use latch, empty hidden
selection, slots and notification. Corrupt candidates and failed publication
preserve accepted state; altered latch/notification controls fail as expected.

`ScriptWorld::request_timed_combat_taunt` binds non-ordered taunt requests to live
receiver/target state, shared RNG and synchronous native/browser playback services.
It retains the constructor-owned signed period across world snapshots and later
attribute writes. Requests beyond the actor's pointer list reject explicitly;
old world snapshots missing the period are rejected. This service does not enable
the ordinary combat decision stage or complete defence execution.

`DialogueState::snapshot` retains ordered records, formatted text, remaining
lifetimes, skip latch and presentation selection. `from_snapshot` returns a
replacement after validating installed settings and explicit actor/sample
bindings, including inactive retained references. It starts no playback and
issues no presentation or speaker effects. Audio reconstruction and committing
the complete restored session remain host responsibilities.

`SimulationClock` computes FPS-derived movement scale and whole nominal ticks from
validated installed clock settings and caller-supplied BIOS samples. Its twelve-update
warmup, fractional carry, smoothing and suspension rebasing are independent of
rendering. Partial checkpoints retain menu changes to smoothing, measurement
remainder and warmup, and leave the last exposed step intact. World preparation
creates the clock only when absent and rejects settings that differ from the
active game. Entering a new world generation restarts warmup without resetting
measurement or fractions; a menu round trip within that generation does not.
Menu caches carry the same installed settings for startup initialization.
The book menu supplies BIOS-boundary invocations and restores its partial
checkpoint on exit. Supported ordinary world updates use the same clock; automatic player updates
remain unconnected.

`ScriptWorld::ordinary_actors` retains fresh-world cell and persistent membership,
reverse insertion order, and the cached view-centered 18-cell selection. Normal
selection omits the four corners and filters attribute 113 on the outer border.
Deferred movement registrations update cell membership before selection.
Allocation/removal and alternate debug processing modes remain unsupported.

World scene roots carry `ScriptScene`. Exported map nodes automatically bind to
their logical actor by placement index and script name. Bindings expire on world
replacement, including reloads of the same world. `initialize_map_animation`
initializes type-1 animation-table map actors with supplied nominal ticks and retains their
selected frames in the shared SOUP state. `select_map_animation_default` applies
ordinary nonplayer default selection to that same playback and retained frame,
using owned actor state and explicit item readiness and ticks. Activation and
default selection share weapon predicates. Normal world updates initialize and schedule supported ordinary actors;
scene binding itself does not advance animation or scripts.

`ScriptHud` handles installed `ShowBitmap`, `UnShowBitmap` and `AddLog` calls
for ordinary updates and explicit script steps. It owns power-up slots and the
session's 400-tag duplicate table. RTX text is retained in native
`cyrus-config/logbook.json` or browser localStorage `cyrus-logbook`; malformed
state and failed writes leave accepted entries unchanged and trigger no icon.
**Verified:** tag acceptance and slot rules follow preservation's
[script HUD contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/hud.md#script-logbook-entries).

**Verified:** `ScriptHud` also retains the compass display flag and consumed-use latch across
world changes. A shared inventory owns live counts and selection from installed
ITEM.INI definitions. `AddItem`, `DropItem`, `HaveItem` and `SelectItem` use that owner.
Startup acquisitions use the same recursive prerequisite and ordered drop/add
rules as script requests. Admitted quantities remain fixed across nested effects;
removal traverses dependencies only while the source retains a nonzero count.
Empty-selection fallback runs before the admitted count increment and assigns the
selected logical player's weapon attribute synchronously. Routed acquisition does
not assign the receiving NPC. Explicit `check_player_weapon` validates current
item ownership/type and applies shared weapon cleanup. Ordinary scene updates run
this check after player-role animation initialization, before actor traversal. These rules follow preservation's
[selection and weapon contracts](https://github.com/michidk/redguard-preservation/blob/main/docs/config/item-ini.md#empty-selection-fallback).
Cyclic effect traversal reports a contextual error. The renderer reads the installed Activate/Attack binding
and current logical-player control, surface, animation and attribute state.
Explicit activation inputs remain available for inspection. Receiving a compass
does not enable it; a held accepted use toggles only once. Rules follow
preservation's [inventory contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/hud.md#inventory-ownership).
`ActiveItem` reads the selected record without an ownership or visibility filter;
`ActiveItem` and `HaveItem` return zero for a closed inventory. Missing hosts and failed initialization
remain contextual errors. `HandItem` validates the full item ID, then records its
low byte on the receiving logical actor after the documented water, edge, air,
weapon and shell gates. The ordinary scheduler supplies its established non-shell context; explicit
callers supply `ScriptInputs::shell_execution`. Unknown context does not authorize assignment. It changes neither inventory
selection/counts nor weapon attributes, and does not draw attachment geometry.
Initial held-item state remains unestablished until an admitted assignment.
These gates follow preservation's
[held-item contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/attachment.md#held-item-assignment).
`state.script_hud.inventory` reports current ownership and selection;
`inventory_error` retains initialization failures without publishing partial counts.
Acquisition/removal rules follow preservation
[ITEM.INI](https://github.com/michidk/redguard-preservation/blob/main/docs/config/item-ini.md).
The renderer consumes accepted notification requests. Full logbook navigation,
new-game/death resets and save-game integration remain unsupported.
The retained SOUP topic-menu word starts at zero and changes through its native
menu lifecycle, independently of the book menu and dialogue state. RTXp keeps
its native task pending during host asset preparation and installs the reserved
player line only after preparation succeeds.

Run `cargo test -p cyrus-game`. Installed-definition tests require `redguard/` and
report when they skip. `dev/scripts/check-core-boundaries.sh` checks the production
dependency graph without workspace or development feature unification.

The debug flycam toggle moves only the view camera. Turning it off returns the
view to the player pose; it does not enable gameplay. Faithful
keyboard actions preserve held and opposing inputs. Modern input samples WASD,
Space, and held Shift run. Camera-relative movement and the mouse camera remain
pending. The deterministic `player::turn` and `player::walk` cores are
tested against original heading and animation-gated startup observations.
Walking supports actor step type 1, advances startup during suppressed
translation, and resets after an update without accepted forward movement.
An explicit ordinary grounded update now composes animation advancement/default
requests, forward support admission, walking translation, support-height assignment,
ordered wall response, residual separation, and startup reset. It preserves
separate animation invocations with the same caller-supplied nominal ticks.
Selecting its sheathed default first applies the shared current/pending replacement
rules, retaining any frame selected before the ordinary advancement.
With animation enabled, a pending walking request does not advance startup.
An active ordinary walking group can continue after input release without
requesting the group again or replacing its pending idle request, following
[preservation PR50](https://github.com/michidk/redguard-preservation/pull/50).
`PlayerPlugin` consumes explicit `GroundedTick` messages on native and browser.
Each message identifies the logical player in the current script-world generation.
Walking retains startup/support state on the logical actor and uses its authoritative
spatial pose, shared animation and selected file frame. Presentation-only heading
changes do not redirect subsequent steps. It advances animation before selecting static candidates by
the selected model radius, then updates `PlayerPose`, `Transform`, `AnimationPose`
and `SoundSurface`. The player receives a `ScriptBinding`, so later SOUP requests
also update its presentation. Stored surface belongs to the logical actor;
presentation projects it before explicit animation playback in the same update.
Animation cues capture the stored value at emission, including support changes
before walking requests. Failed transitions retain prior surface assignments.
Unsupported object paths have no surface value and stale bindings clear the
component. Authored cues use `SoundCue`; native inspection
recognizes these bindings even on unnamed player entities.
Multiple messages remain separate updates; zero nominal ticks can still move.
Stale asset requests and world identities are discarded. Walking errors stop
stepping until entry changes; logical animation failures persist until world
replacement. Marker changes do not create a second playback or reset script tasks.
The caller still supplies admission, timing, orientation, view and collision policy;
the automatic ordinary adapter uses the shared simulation clock and logical actor
instead of generating GroundedTick messages.

`GroundedWalk::continue_forward` exposes the post-script walking continuation
without running late collision or resetting startup. An accepted earlier action
suppresses a second translation. The explicit composed update uses the same method.
`ScriptWorld::initialize_player_spatial` establishes a player role's generation-bound
pose once; it does not settle ground or admit automatic controller execution.

The ordinary idle default comes from installed `attr_panic_group`, following
[preservation](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/player.md#ordinary-idle-animation-default).
The forward probe uses installed bounds and walk distance with nearest-even
integer rotation, following [preservation PR48](https://github.com/michidk/redguard-preservation/pull/48).
Player caches retain authored frame-source model radii; candidate selection can
resolve the radius by active model name. Missing named radii require re-export.
The upright orientation helper reuses the original movement rotation table.
Callers still supply eligibility, actor orientation, candidates,
surface policy, and shared support state. Falling, water/lava transitions and
special movement states remain unsupported. The ordinary adapter drives the
admitted sheathed walking stages; full player lifecycle remains unfinished.

Scene inspection places the player at available marker zero once the world's
`.animations` and `.player` data match, using preservation's exact map coordinates. `MarkerEntry`
selects a different marker and heading for explicit entries. `PlayerPose` retains
runtime coordinates and the wrapped heading; the transform uses the existing
scene export coordinates. Entry then settles against dry terrain and static models,
using the actor-centered startup query documented in
[preservation PR50](https://github.com/michidk/redguard-preservation/pull/50).
Startup selection preserves earlier equal-ranked contacts, independently of ordinary
walking support. Dynamic/transient support and heading-to-render rotation remain
unsupported; this inspection placement does not enable gameplay. Inspection retains the authored marker and reports unsupported settling when
terrain/model support or the player definition is unavailable. Invalid markers
still produce an entry error. Re-export older assets that lack
`map_position`.
`PlayerEntrySupport` distinguishes successful static settling from retained
inspection placement and clears with an invalid or pending entry. Static settling
alone does not establish dynamic support or controller eligibility.

`placement::initial_animation` applies the selected-player entry requests with
caller-supplied nominal ticks and a synchronous sound handler. It starts with
default group zero, makes both ordinary setup requests, then forces the default.
It retains the selected file frame for presentation, following
[preservation PR50](https://github.com/michidk/redguard-preservation/pull/50).
This explicit entry API does not choose a production clock or enable locomotion.
The ordinary scene host initializes CYRUS then GREMLIN after map animation setup,
using the existing shared ticks and capturing marker coordinates before settling
for initialization sounds. Both roles retain their attribute-controlled combat
association. GREMLIN remains excluded from ordinary updates; the selected player's
scene binding supplies its initial mesh frame without a controller tick.
Initialization failures remain sticky, and world replacement discards old role
bindings. This follows preservation's [role setup order](https://github.com/michidk/redguard-preservation/commit/e0a680f32ecee9f22a410cda297e306a700d37f9).

Grounded animation sound callbacks retain the surface and local Q8 position at
command execution. The runtime converts Play coordinates into the checked i32
transport before later movement and ground response; Stop and Forget do not
require a position. This follows [preservation's animation sound rules](https://github.com/michidk/redguard-preservation/blob/a89de2b/docs/engine/sound.md).
**Verified:** a runtime contract retains the emitting position while the player
finishes the update at a different position. Complete walking/audio parity remains
**Unknown**.

`script::ScriptPlugin` samples the shared nominal clock once per InGame update.
With a matching `ScriptView`, it initializes placed animation-table actors and
flushes deferred movement before traversing the authored camera-centered actor
list. `ScriptView` retains original Q8 camera coordinates for relocation and
regional selection. Supported updates select the
default animation, advance playback, run ordinary hook/main stages, and restore
locks and families. Failures stop the affected actor and remain inspectable.
Collision and combat stages and remaining native services are explicitly
unsupported. **Verified:** an installed ISLAND FLAG2 changes frames through the
normal Bevy Update schedule; this does not establish START or all-world acceptance.
Menu updates suspend traversal and exclude paused host time from clock measurement.
Headless assemblies without a view do not traverse actors automatically.

`HideMe` and `ShowMe` retain receiver visibility without disabling its scripts or
animation. Rendering observes that flag and refreshes static model transforms.
**Verified:** logical return/continuation and mixed static-presentation tests follow
[preservation's visibility rules](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#object-visibility).
Standalone player visibility lifecycle remains unsupported.

`script::ScriptPlugin` also reports script cache preparation and advances explicitly
attached `ScriptedPlayback` with supplied `AnimationTick` messages. Its
`AnimationScript` is a PushAnimation host for the shared `cyrus-script::Runtime`,
not another interpreter. Animation request/count/wait, same-stage continuation,
End restart, synchronous cues and sticky failures remain intact. Interrupt and
synchronization behavior is still unavailable in this animation host.

`ScriptStatus` distinguishes preparation from live execution. Loaded programs,
unsupported counts and bounded examples are visible in native control `state`.
Automatic actor activation, production timing, other native game handlers and
saves remain pending. No current inspection launch enables gameplay.

`ScriptWorld` prepares logical instances in authored placement order, sharing
immutable instructions and global inspection flags while retaining separate
initial locals. Generations reject stale explicit invocations. This is fresh
inspection state, not progression, revisit or save semantics. Preparation does
not grant update eligibility. Removing the prepared world clears this registry.

Explicit model-resource preparation and caller-admitted presentation refresh retain
collision-facing poses separately from logical movement. Hidden actors lose their
current record but retain selected resources; showing a retained static model
refreshes its record immediately. Automatic camera-centered PVO admission reads
retained support before ordinary updates clear it, using a frozen load-time
secondary table. Dry model positioning works with collision enabled; water-specific
position adjustments and ordinary collision response remain unsupported. Matching
world geometry and visibility preparation are required before automatic updates.
The explicit ordinary ground query composes static and live candidates using
stored model poses, excludes the querying actor's own model, and retains the
selected support owner. Actors without a current model record are rejected before
collision-path metadata is required, including model-less standalone roles. It has no selected-player fallback or history. Probe
distance remains caller-supplied. The vertical extent comes from the actor's
initialized collision shape. Shape preparation distinguishes authored MPSZ bounds,
static vertices with installed horizontal scaling, and the model-less single
sphere. Placed-model fallback does not rebuild that shape. Automatic response
and surface transitions are not enabled by querying support.
The explicit ordinary body query shares those admitted models and stored poses,
uses the actor's initial spheres and rebuilt local collision orientation, and retains integer
contacts in original order. Its primary sixteen-contact limit is separate from
each model's thirty-two-contact buffer. Terrain admission and candidate-type
exclusions remain caller-owned; collection does not perform motion response.
Ordinary control owns allocator-zeroed sliding, falling and queued-motion bytes
separately from animation groups. Explicit primary wall response updates grounded
position, heading and result marks from retained ordered contacts. State-specific
motion, overlap, residual separation and support following remain separate;
this primary loop does not enable automatic collision updates. Explicit residual
queries use copied update angles, adjusted common sphere radii and uncapped
static/live collection without terrain. Residual response owns separation and
its result mark. `OrdinaryCollisionQuery` retains actor-bound candidates and copied working angles
prepared before the script wrapper. Ground, primary and residual inputs require
that record, while query origins follow current actor state. They do not rerun
spatial admission. Queries reject another actor or world generation. Ordinary
traversal receives matching scene geometry through `OrdinaryInputs` and prepares
its list before the wrapper. Unsupported queued motion and late response remain
explicit failures. Overlap, support transitions and rollback still need their
own stages before sustained automatic collision updates can run.
The lifecycle follows preservation's
[model rules](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#current-model-presentation).

Native control and MCP `scripts` page through at most 64 actor diagnostics.
`step_script` runs a selected animation, entry, main, interrupt or existing-task stage. The
inspection host supports the researched `GetAttribute`, `GetMyAttr`,
`SetAttribute`, `SetMyAttr`, `SetGlobalFlag`, `ResetGlobalFlag` and `TestGlobalFlag`
services. **Verified:** attribute writes retain the low byte; `SetAttribute`
also maintains reserved collide/weight bytes, while `SetMyAttr` does not. Reads
observe writes in the same stage and routed operands use receiver locals.
`GetMyAttr` and `SetMyAttr` return zero outside the attribute table. Other
out-of-range attribute access remains an explicit failure. Ordinary map actors
own their allocation control state separately from playback. Live map actors
support attribute-44 writes without changing the combat death default. Type-1
animation-table initialization creates that separate default at zero when
attribute 14 is nonzero. Standalone-role attribute-44 writes and dead-actor
walk-node cleanup remain unsupported.
These contracts follow preservation's
[attribute rules](https://github.com/michidk/redguard-preservation/blob/ca28d9d/docs/engine/SOUP.md#attribute-reads-and-writes).
`ScriptActor::attributes` exposes the prepared inspection bytes. Complete actor
lifecycle and remaining live player attribute effects remain pending. Walking and
SOUP share logical-player playback; scene entry now constructs player-role
animation and combat state for the bounded ordinary walking adapter. Complete
installed traversal remains unverified.
`ScriptWorld::snapshot_flags` captures global values and latches with their ordered
installed definitions. `restore_flags` validates the complete snapshot before
replacing both values and latches, without executing scripts or native operations.
**Verified:** public contracts cover native Bool/FlipFlop continuation, mixed
latches, signed values, counter wrap, installed definitions and rejection without
partial writes. These follow preservation's
[global flag records](https://github.com/michidk/redguard-preservation/blob/main/docs/formats/SAVEGAME.md#global-flags).
Original SVGF import, cross-world retention and whole-game restore remain unsupported.

`NativeTasks::snapshot` captures actor-owned native variants, continuation fields
and allocator history. `from_snapshot` returns replacement state after checking
the matching VM's task calls, token census, variant/name/kind/arity agreement and
complete injective token bindings. It preserves paused tasks, targets, traveled
distance, rotation accumulators and started latches without polling or effects.
Animation and dialogue tasks require restored owners; retained ambient sample
slots require explicit channel mappings. The host validates owner/generation
identity and installed sources, restores all actor/world state, and commits the
complete candidate. This API supplies no live partial-task commit.
**Verified:** headless contracts cover wait and motion continuation, allocator
history, binding rejection and PushAnimation without repeated requests or cues.
Audio/dialogue owner restoration, whole-actor/world assembly, storage and original
save import remain incomplete.

**Verified:** self task and multitask `Wait`, `WaitOnTasks`, and
`WaitNonMulti` use preservation's elapsed and unpaused-slot rules. The tasks stage
requires `frame_scale` with eight fractional bits; omission fails before actor
mutation. Native wait tokens belong to each logical instance. Initial waits always allocate;
completion applies on a later task pass, including duration zero and one. Function-encoded
waits remain unsupported because their timing context is not established.
Prepared spatial hosts also support `RotateByAxis`, `PlayerDistance`, and
`PlayerFacing`, `PlayerLooking` and `PlayerArc` with explicit selected-player inputs.
`PlayerLooking` uses the effective receiver position and signed angular tolerance,
including equality, per the [published query contract](https://github.com/michidk/redguard-preservation/compare/main...docs/player-looking-contract).
It does not perform a visibility query. `PlayerArc` uses receiver heading, signed
offset and inclusive angular tolerance, following its
[published query contract](https://github.com/michidk/redguard-preservation/compare/main...docs/player-arc-contract).
Attached poses supply its position and heading; a linked position alone leaves
local heading unchanged. It performs no visibility or distance check.
`PlayerStand` compares the receiving actor
with the selected player's retained supporting-actor identity, following the
[published query contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#playerstand-query).
It performs no collision search and leaves support ownership unchanged. Missing or
stale selected identities reject. Property access resolves installed reference names
and the selected player. `angle.vy` reads the receiver's local heading.
`pos.vx`, `pos.vy` and `pos.vz` read and write signed Q8 local coordinates.
Writes change only the selected coordinate; matrices and effective attached poses
remain unchanged until ordinary processing refreshes them. Missing spatial state,
unknown properties and angle writes reject explicitly. `anim.current` reads the
prepared playback owner's signed current group without advancing it; a prepared
owner with no current group returns `-1`. Missing playback preparation rejects
rather than inferring an absent native animation association. `torch_timer`
reads the shared torch-burning byte and writes only the assigned low byte,
without running torch effects. Animation-property writes remain unsupported.
These rules follow
[preservation's property contract](https://github.com/michidk/redguard-preservation/compare/8c6bdb15c783fca72a6399aa226f25f596585407...fix/soup-property-references).
Older script programs require re-export for full-width references.
Remaining native services are explicit failures. `IsInDeepWater` reads the receiver's stored
surface state, with fresh map/player allocation values and grounded assignments
shared through `ScriptWorld`. It does not issue a separate support query. End notifications with synchronization
channels remain unsupported. These tools do not start New Game.

**Verified:** fresh inspection prepares standalone CYRUS, GREMLIN and CAMERA
instances from installed definitions, after ordinary map actors. They have separate
locals and task slots. Player defaults to CYRUS; Camera uses its own instance.
`ScriptActorSource` distinguishes those roles from map placements. Type-2/type-6
map records consume sequential authored locals without ordinary actor identifiers.
Selected type-two and type-six records run their entry hook and enabled script/task
pass after ordinary actors. Their attributes and control state are temporary; locals,
continuations and tasks persist. Authored type-six lights retain enabled/locked state
and inactivity across replacement. `LightOff` disables an existing light without
allocating one. Separate light maintenance runs even with tasks disabled; its wrapping
byte counter releases disabled records above 32 ticks. Dynamic light creation,
modulation, type-four light control and separate-record movement remain unsupported. Status `instances` counts logical entries,
including standalone roles and unavailable map records.

Native routing supports Me, Player, Camera and installed RANM names with ASCII
case folding. Name lookup uses signed 16-bit runtime identifiers, preserves first
registration of an identifier, and chooses the first matching namespace. Initial
map identifiers follow installed actor-pool capacity, activation flags and placement
order, independently of MPOB object ids. Functions use receiver locals and
attributes; tasks occupy receiver slots while the caller continues. Foreign tasks
retain source instructions. Both runtimes are restored on errors, and failures
remain sticky. These rules follow preservation's
[SOUP contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md).
This is fresh logical preparation. Player switching, variable references, scriptless
receivers, later allocation/recycling, reentrant callbacks, automatic scheduling,
progression and saves remain unsupported.

`ScriptWorld::bind_animation` attaches animation state to the logical instance
only after world/content identity checks. The caller supplies an established
default group. `advance` runs one native hook, animation, ordinary entry, task, or body stage;
these stages remain separate. Once any actor has animation bound, all invocations
require explicit nominal ticks, including zero. Task stages separately require frame scale. Waits and
PushAnimation share one native-token map and the VM's six slots, including routed
receiver tasks. The standalone animation adapter uses the same task handler.
Animation failures are sticky and appear in actor diagnostics.

`ScriptBinding` makes a scene node observe the logical actor's selected frame.
It expires on world-generation or source changes. Presentation does not advance
the script. Native MCP `bind_script_animation` can select a node by exact name;
omitting the node prepares logical inspection only. `step_script` accepts
`animation`, `entry`, `tasks`, `main`, and `interrupt`, with `nominal_ticks` for
worlds with animation-bound actors and `frame_scale` for tasks. Animation sound
uses the unique scene binding; without a binding, inspection captures the intent.
`FlatSound`, `EndSound`, `EndMySound` and `EndMySounds` dispatch synchronously
with the logical receiver identity, including actors without scene bindings.
Control captures these commands and submits them to the sound consumer when present.
Their zero results follow the [preservation sound contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/sound.md#soup-functions).
Positional `Sound` uses the receiver's prepared Q8 position and bypasses
animation surface remapping. Ambient sound, shell/code-object owners,
`KillMe` and the script
`StopAllSounds` result remain unsupported.

`LockoutPlayer` retains its argument's low word in `GameplayGlobals`, clears
remembered topic-choice text and returns zero. Exact one sets the selected
player's attribute 117; other values clear it on the player and registered
actors and clear the activation prompt-override timer. Activation and overlay
updates consume that value immediately. Unknown initial lockout remains absent;
explicit inspection inputs supply the fallback before a native call establishes
it. The host retains gameplay globals while replacing logical world registries.
**Verified:** setter semantics follow [preservation's HUD rules](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/hud.md).
New Game applies zero lockout before world replacement, including when the first
logical world has not yet been prepared. Ordinary reloads retain globals.
**Verified:** New Game follows [preservation's menu rule](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/menu.md).
Topic-menu execution, production HUD inputs and death/load reset ownership remain
incomplete.

`AmbientSound` native slots can use an explicitly supplied synchronous
`AmbientSoundDriver` and sample-reset input. Each slot retains its own channel
handle; elapsed zero and sample resets discard the handle, while `TaskStop`
only removes the task. Audio hosts lend the production mixer through
`OrdinarySoundExecutor` while `run_ordinary` retains game-owned stage order and
logical actor state. Immediate cues and ambient polling share live channels.
Automatic sample-reset inputs still require menu/dialogue/lockout and navigation
owners; absent inputs remain unsupported. Explicit driver tests verify slot and
gain behavior, not audible or automatic all-world ambient parity.

**Verified:** ordinary entry suppresses execution when an unpaused blocking task's
physical operand identity is at or before the saved entry cursor. Direct
interrupts bypass that entry gate, following preservation's SOUP contract.
**Verified:** [preservation PR54](https://github.com/michidk/redguard-preservation/pull/54)
establishes signed group comparison, finite-count writes only before animation
task startup, and distinct model/playback default state. Standalone-role attribute-44 writes and dead-actor navigation cleanup
remain unsupported because their lifecycle owners are not integrated. The inspection default is not a
replacement for original state-dependent selection.

Fresh duplicate root masters on the same source list select the later allocation,
which is first in the prepended list. Unselected roots retain their independent
poses. **Verified:** installed CATACOMB group 3 attaches its slaves to CC#83
instead of rejecting the world. Cross-list duplicates and repeated child-master
groups remain unsupported until their original initialization traversal is available.

`ScriptWorld::prepare_spatial` prepares ordinary map poses and supported attachment
families. Separate map-object paths and standalone role poses remain unavailable.
`ScriptInputs` supplies nominal ticks, frame scale, and current query inputs to
explicit invocations. `RotateByAxis` shares the native task store with waits and
animation, mutating local angles without implicitly refreshing matrices or
attachments. Snapshot, unattached transform refresh, and family refresh are
separate operations. Spatial preparation rejects ambiguous or cyclic families;
attachment-sensitive attribute changes remain unsupported after preparation.

`RotateToAxis`, `sRotateToAxis` and `sRotateByAxis` share that task store and local-angle ownership.
The first integrates shared frame scale toward its target; the second interpolates
from a retained origin using task elapsed input and snaps at completion.
`sRotateByAxis` retains its origin and interpolates a relative delta with distinct
negative-offset rounding before its final snap.
Their arithmetic follows [preservation PR73](https://github.com/michidk/redguard-preservation/pull/73).
`sRotateToAxis` rejects duration one and `sRotateByAxis` rejects duration zero
with contextual division errors instead of crashing.

**Verified:** [preservation PR55](https://github.com/michidk/redguard-preservation/pull/55)
defines rotation task arithmetic, player distance/facing queries, and local versus
composed attachment transforms. Actor inspection exposes both poses and the
previous-angle/delta fields. Attached queries retain the previously composed pose
until refresh. Override and fallback inputs remain explicit. The script world
owns each fresh receiver's absent linked-position record, following
[preservation PR66](https://github.com/michidk/redguard-preservation/pull/66).
Automatic traversal supplies the selected player pose and the live native
`neversaydie` distance override. Hosts without initialized cheat storage use
the installed override from the version 10 script cache.
Dynamic linked-position creation remains unsupported until its state
owner is integrated. Complete lever interaction acceptance remains open.

`Activate`, `TorchActivate` and `Deactivate` share one `ScriptWorld` owner,
pending prompt and timer state, following
[preservation's activation rules](https://github.com/michidk/redguard-preservation/blob/2e03012/docs/engine/hud.md#activation-prompt).
Activation decodes the installed numeric operand as a packed RTX key, with the
first character in the least significant byte, following the
[parameter contract](https://github.com/michidk/redguard-preservation/blob/c51e013/docs/engine/SOUP.md#parameter-type-overrides).
`ScriptInputs::activation` supplies an explicit selected-player identity, actor
conditions, RTX labels, dialogue/lineup state and held use. Missing actor state
fails explicitly. Attributes come from the receiving and selected logical actors;
a bound player animation overrides the supplied animation-group snapshot.
Animation-state presence is separate from its current group; a bound state with no
current group still satisfies the weapon predicates' animation-state gate.
Routed activation owns the receiver. Deactivation clears holdoff for every
receiver but releases only a matching owner. World replacement resets this state.

`activation_overlay` consumes pending text only on a drawable prompt path.
Lockout, holdoff, override and selected-player presentation gates retain it.
`ActivationInputs` may also be installed as a Bevy resource for the HUD and native
inspection adapter. No production system supplies these complete actor conditions
yet, so this does not enable automatic interaction or player switching.

`restore_axis_locks` reads current attributes after an explicit ordinary actor
update and restores locked local position/angle components from `snapshot_spatial`.
Slaves bypass restoration. This stage leaves matrix/family refresh separate and
does not enable automatic scheduling. Rotation-lock writes after spatial
preparation now remain visible to this stage.

`ScriptStage::OrdinaryMain` checks the ordinary map actor's script-control byte
once, then runs entry/tasks/main with supplied timing. `DisableScript` suppresses
later ordinary main stages without clearing tasks or stopping the current
invocation; `EnableScript` resumes them. Both preserve other control bits.
Standalone role control initialization remains unsupported. Native-hook dispatch
is separate and does not use the main-stage suppression bit. This explicit stage
does not admit actors into automatic updates.

`MoveByAxis` and `sMoveByAxis` use the same actor-owned native task slots and
local position as rotation and attachment presentation. The former adds a
quantized per-invocation step; the latter derives displacement from elapsed
task time. Their rounding, target initialization and completion rules follow
[preservation's movement contract](https://github.com/michidk/redguard-preservation/blob/e4e2aca/docs/engine/SOUP.md#relative-axis-movement).
Movement leaves collision, axis-lock restoration and family refresh to their
own stages. An installed ISLAND mechanism test supplies its enabling flag,
runs the authored movement, and checks deferred child-pose refresh. Automatic
admission and moving-platform passenger response remain unsupported.

`MoveToLocation` retains each instance's original home as location zero and
prepares its authored RALC offsets relative to that home. Its task reads the local
or attached effective source position, advances local coordinates with the
original normalization and frame-scale arithmetic, and completes strictly after
the duration. Every invocation queues the actor's deferred map update. Collision
and family refresh remain separate stages. Selectors at the original out-of-range
count boundary report unsupported access. These rules follow
[preservation's authored location movement](https://github.com/michidk/redguard-preservation/blob/c077ed1/docs/engine/SOUP.md#authored-location-movement).

The `ordinary_hook` stage applies the installed task-system switch and the
receiver's native-hook suppression flag. With the task system enabled, every
unpaused task suppresses this hook, including nonblocking multitasks. Paused
slots do not suppress it. `ordinary_main` does no work when the installed task
system is disabled. Direct inspection stages retain their explicit behavior.
These gates follow [preservation's ordinary update rules](https://github.com/michidk/redguard-preservation/blob/e4e2aca/docs/engine/SOUP.md#ordinary-update-gates).
Combat dispatch and complete automatic actor admission remain unsupported.

`AtNodeMarker` reads the receiver's local position and the ordinary marker table
prepared from matching animation metadata. **Verified:** the distance rounding,
inclusive threshold and marker presence follow
[preservation's marker contract](https://github.com/michidk/redguard-preservation/blob/bcc298c/docs/engine/SOUP.md#node-marker-distance).
Installed-map preparation and original-instruction boundary comparisons cover
this explicit query. Routed calls retain the receiver's local attached pose.
Missing/out-of-range markers report contextual errors; distances outside the
signed integer range and alternate-map marker tables remain unsupported.
The installed HIDEOUT hook reports an out-of-range marker when explicitly invoked.
**Verified:** no installed WORLD.INI map assignment selects HIDEOUT.RGM; natural
execution of that hook remains **Unknown**. This does not enable
automatic actor updates.

`GamePlugin` owns one `EngineRandom` resource independently of world replacement.
`ScriptWorld::advance` accepts that stream explicitly for `Rnd` and `QuickRnd`,
including routed calls. Missing state and nonpositive `Rnd` bounds fail without
inventing a result. **Verified:** sequential result/state comparisons against
original instructions cover the arithmetic documented in
[preservation's random contract](https://github.com/michidk/redguard-preservation/blob/2390c5f/docs/engine/SOUP.md#random-functions).
The executable's stored initial state is the default. Complete session seeding,
consumer order and sky integration remain unsupported.

Ordinary actor cell assignment uses decoded MPOB coordinates directly, matching
the unscaled view coordinates passed to `ordinary_actors`. **Verified:** original
allocation-instruction comparisons and an installed PALACE selection test reject
a second fixed-point conversion. Deferred MoveObjectAxis registrations relocate
actors before regional selection; complete actor admission remains unfinished.

**Verified:** explicit `Wander` hooks in worlds without walk-node maps consume
shared random draws, retry candidate selection with the original distance rounding,
and restore excessive displacement from the ordinary-update snapshot. Home comes
from authored placement, before attachment transforms. The installed VILEDOG hook
runs through this path. Navigation availability is prepared from the matching
animation sidecar; nonempty-map routing and steering remain unsupported. This
implements the original empty-map behavior, not automatic actor admission.

`script::PlaybackDiagnostics` retains explicitly attached playback failures after
the failed component is removed. Records identify the world, actor, failing stage
and reason. Consumers may drain them. Recording neither retries the failed effect
nor advances scripts or animations. Logical actor preparation and sticky runtime
failures remain available through `ScriptActors` and `ScriptStatus`.
`ScriptActor::animation_failure` and `animation_initialized` expose retained
animation status separately from SOUP failures through read-only accessors.

## Cheats

`Cheats` owns the current named cheat values and unnamed persistence slots.
The host supplies a `CheatStore` that decodes and encodes through the asset adapter
and preservation. Failed loads stay errors and cannot be overwritten by commands.
Changes remain in memory until `savecheats`; failed saves retain runtime state.
`oracle` lists raw values. Unfinished gameplay effects, including the unknown
`oddjob` effect, are reported as unsupported. The saved `savecheats` slot is zero;
its live value becomes one after a successful write. Explicit import persists
a validated file before replacing live state, including recovery from a bad load.

**Verified:** preservation's [cheat rules](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/cheats.md)
define the `neversaydie` PlayerDistance override. Automatic world updates and
explicit script steps use the shared value; explicit steps may supply an
inspection override. Opening the console suspends automatic actors and their clock.

`ConsolePlugin` owns F12 focus, retained editable commands and report lines,
keyboard editing, history recall and hidden gameplay entry. `cheat_command`
normalizes one console command and supports decimal, signed decimal and hex
assignments. Symbolic/exotic numeric forms and non-cheat commands other than
`console clear` remain unsupported. Console toggles use parity; gameplay toggles
use zero versus nonzero. Hosts share this editor and dispatcher; rendering and
file dialogs stay outside the core.

Ordinary actor overlap reads current logical local positions independently of
stored face-model presentation poses. Static-model actors supply one model-radius
sphere; initial nonstatic shapes retain fractional actor-matrix-transformed
centers and exclude the additional support sphere. Vertex-bound RAVC shapes
report unsupported until their animated vertex owner is connected. Pair queries
reject stale IDs and exclude self. The admitted automatic collision path invokes ordered weighted response.

Explicit grounded ordinary actor-pair overlap response commits weighted X/Z
separation or previous-position rollback to logical actors. Tiny contacts retain
both runtime identifiers and return a pass-ending Touch result. Contact marks
preserve existing transition bits. Other actors receive the original conditional
mark. Selected-player overrides, support-state bypasses and airborne/damage paths
remain unsupported by this adapter; vertex-bound shape and complete movement acceptance remain open.

The explicit retained actor-style overlap pass visits static and live candidates
in prepared order. Static tiny separation still responds; tiny live contact ends
the pass before later queries. Its typed result retains required obstacle-repeat
work across later rollback or misses, and skips that repeat on early Touch.
The ordinary scheduler still stops before unsupported complete late response.

**Verified:** explicit ordinary forward segment response probes previous-to-current effective
motion through the retained face-model list. It selects the closest masked
rank, combines all accepted normals in order, and writes logical X/Z using the
selected first sphere radius while preserving Y and model presentation.
Response-disabled queries report contact without moving the actor. These rules
follow [preservation's forward response](https://github.com/michidk/redguard-preservation/blob/729dc5b/docs/engine/player.md#ordinary-forward-segment-response).
State-fifteen forward probes run the two stored-bounds segments instead and
report no ordinary forward hit. The bounds stage retains independent signed
rank limits per segment, accumulates improving wall normals without normalizing,
and writes all logical axes with body-response bit six. Polygon sidedness is
explicit per query; the normal forward path enables it and the bounds path
bypasses it. Selected-player auto-grab and complete automatic late collision integration
remain incomplete.

**Verified:** the explicit ordinary obstacle wrapper runs its responding,
conditional nonresponding and higher responding probes in order. Motion
transitions share body-response flags with overlap, independently of rollback
collision-transition flags, clear the interrupted motion vector,
and write the fall origin/timer. Queued falls force the published animation
group through captured sound/position dispatch; they do not replace the default
group. The overlap-stage adapter performs the required obstacle repeat after
weighted separation and skips it on an early tiny actor-pair contact. Rules
follow [preservation's wrapper and transitions](https://github.com/michidk/redguard-preservation/blob/f2f8a02/docs/engine/player.md#forward-obstacle-wrapper-and-motion-transition).
**Unknown:** full automatic late-response and selected-player grabbing parity.

`prepare_stable_recovery_query` creates a separate actor-bound admission record.
`query_stable_recovery` accepts model contacts in traversal order and leaves
terrain acceptance without a position write. Terrain water and grid edges remain
explicitly unsupported. The automatic late path composes the supported branches; airborne timing and complete collision acceptance remain open.
`capture_ordinary_rollback` saves the post-advancement receiver, frame and movement
flags. The water-veto host restores a separately retained dry/falling position;
its fresh-allocation value remains unavailable until its owner establishes it.
The audio consumer saves and restores the remembered animation slot in cue order.

The ordinary script wrapper counts prior packed response marks in its byte age,
then clears those marks and the overlap identifier. Stable recovery uses a fresh
candidate query and restores rejected nonplayer updates to their retained stable
position, movement flags and animation snapshot. Its heading change consumes the
shared random stream; age above 16 also runs the bounds response. The automatic
late path applies the published attribute-89 and captured-landing predicates.
A missing surface-veto owner remains an error when that predicate needs it.
**Verified:** public contracts cover byte wrapping, the age-200 cutoff, one shared
heading draw, position/frame rollback and response cleanup on the next script
wrapper. Selected-player recovery and full START acceptance remain **Unknown**.
These rules follow [preservation's stable recovery response](https://github.com/michidk/redguard-preservation/blob/34a2dd8/docs/engine/player.md#stable-recovery-response).

The explicit ordinary slope stage accepts primary support, installed
`slide_speed` and caller-established combat/weapon predicates. It orders local
turning, first-slide forced or continuing ordinary animation requests, Q8
translation and a second retained-candidate support query. It retains sliding
state and accumulated distance. Selected-player camera updates remain
unsupported. These rules follow preservation's
[ordinary slope response](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/player.md#ordinary-slope-response).
Automatic collision-stage composition and New Game intro acceptance remain open.

**Verified:** ordinary support-following uses retained actor ownership and the
support's update-entry position and angles. The unattached nonplayer path carries
the rider through translation and two original rounded matrix transforms, then
adds heading delta before axis locks. Static ownership alone and attribute 24
zero leave the rider unchanged. Selected-player collision admission and
attached/linked support snapshots remain unavailable. Automatic collision
composition still stops before this stage for unsupported collision paths.

**Verified:** automatic ordinary updates now compose forward/overlap response,
primary support, admitted grounded height and surface assignment, stable-position
retention, supported body/residual response, reserved-weight restoration, water
rollback, support-following, axis locks and water-position retention. A synthetic
public-interface test sustains three script updates on retained live support and
checks Q8 height, surface, stable record and restored weight. Airborne timing,
terrain body contacts, slope suppression/combat admissions, conditional stable
recovery and special movement still fail contextually. This does not establish
START intro playback or all-world acceptance.

Ordinary living states 20 and 21 run shared weapon-draw admission after support
and surface assignment, including its weapon cleanup branch. They do not run
the ordinary height/falling response in the same stage. Required inventory,
selected-player and shell inputs come from the shared host. This follows
[preservation's combat ground dispatch](https://github.com/michidk/redguard-preservation/pull/76).

Combat maintenance retains constructor-owned target, action, flags and cooldown
separately from the death-animation default. The supported idle nonplayer stage
restores collision attribute 1 from 119 and applies action/cooldown maintenance.
Attack slots retain all nine authored selections, including holes; defense slots
end at the first zero. `combat_attack` resolves a retained slot against the shared
prepared table and reports missing definitions without selecting a fallback.
These bindings follow [preservation's move-loading rules](https://github.com/michidk/redguard-preservation/pull/77).
Attack decisions and actor-specific motion timing remain unsupported.
The logical actor also retains its prepared combat-taunt list.
`select_ordered_combat_taunt` implements admitted begin-combat, hurt, death and
opponent-unarmed selection against the shared RNG, including death fallback.
It does not establish eligibility, mutate weapon state or present the selection;
timed generic selection remains unsupported. `present_combat_taunt` now performs
the explicit shared cache/dialogue/animation presentation path and returns the
retained dialogue active flag, rather than its lifetime. The actor owns cache and
dialogue indices independently. Playback dependencies must be ready before old
ownership is released; the caller supplies the animation admission gate and
nominal ticks. Full combat decision-making remains unsupported.
Selection follows preservation's
[taunt rules](https://github.com/michidk/redguard-preservation/commit/fcc0314a02ce735185f064449802acee154f47c7).
`player::input::PlayerToggles` retains walk mode and auto-defend across HUD
recreation. The HUD initializes it from installed or saved settings and projects
its values for display; ordinary controller admission remains a separate step.

Selected-player ordinary combat retains entry inhibition, phase and both action
indices. Fresh phase-zero maintenance restores the collision attribute, processes
messages, resets nonzero action selectors and then applies the shared cooldown
stage. Nonzero phases remain unsupported. Explicit scheduler shell state suppresses
combat even when activation inputs are absent.
Shared class queues retain independent actor read cursors, addressed filtering
and eight-entry wrapping. Target/perception, combat messages, decision state 6
and attacks remain explicitly unsupported. Queue reset ownership and complete
START intro acceptance remain open. These rules follow preservation's
[combat maintenance](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#combat-maintenance).


`SoundMaintenance` samples signed whole nominal ticks before ordinary actors and
requests RTX release, dialogue cleanup, sample stopping and walk-node reservation
clearing in preservation order. It exposes a reset only after successful cleanup;
a failed stage retains a contextual error and prevents another update from
silently completing a partial reset. Menu, WaitOnDialog and lockout inputs remain
explicit. Session globals retain the timer across world replacement, with its
counter reset at world-loop entry. The audio adapter runs cleanup before ordinary
traversal and supplies its reset result to ambient tasks. The controller samples
inhibition after its conditional countdown and before pending actions. Missing
service ownership and partial failures stop updates contextually.

Navigation preparation retains cleared ownership bytes for every authored WDNM
map and node. Logical actors acquire or release reservations using their runtime
identifier's low byte. A foreign reservation blocks acquisition, and periodic
maintenance clears every reservation regardless of owner. Route selection and
movement through nonempty maps remain unsupported. These contracts follow
[preservation sound maintenance](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/sound.md)
and [WDNM ownership](https://github.com/michidk/redguard-preservation/blob/main/docs/formats/RGM.md).

Dialogue preparation retains installed settings and zero-initialized active records
in gameplay globals across world replacement. `WaitOnDialog` reads this shared
owner and yields through the ordinary task lifecycle even when no line is active.
Explicit topic-menu lifecycle operations retain the shared state used by sound
maintenance. The installed `menuProc` task is not yet connected to this owner;
engine helper names do not add functions to the installed script definition.
Explicit dialogue records support ordinary allocation, reserved player replacement,
sample-matching cleanup and the ordered record-update stage. Sound-reset cleanup
clears active ordinary and reserved player lines before the host stops samples.
`ScriptWorld` owns
speaker inhibition and retained lockout; the host supplies playback status, volume
and presentation synchronously. Expired speaker identities fail before actor writes.
The combined update accepts a configured skip signal and retained lockout, applies
edge-triggered cleanup, then advances records. Unknown lockout is rejected only
when a fresh skip edge needs its value. `RTXp` native tasks accept an explicit synchronous `DialogueSource`, select the
player as speaker, replace record zero once at zero elapsed time, and wait for
that record to clear. A shared sound reset completes the task before lookup.
The source owns entry lookup, optional shared-channel voice start and installed
REDSEL formatting. The live audio adapter prepares entries asynchronously, then
runs the record stage and cache aging after object updates. Keyboard dialogue
skip uses the configured key_a before Modern movement interpretation; joystick
input remains unsupported. Ordinary RTX creation, topic-menu processing and the
complete automatic maintenance owner remain unsupported, so story progression
is not yet established.

Caller-admitted `beginCombat` native calls synchronously enter lifecycle six,
apply authoritative weapon draw/cleanup rules, set combat state and select a
type-zero taunt through shared RNG. The statement has no supported numeric
result. Explicit world APIs reuse these rules. Ordinary nonplayer calls derive
admission from the selected actor, shared inventory readiness, supplied shell/tick
state, animation-system switch and prepared receiver model. Selected-player calls
still require explicit admission because their death override has no automatic
owner.
Prepared taunt services must own all selected payload dependencies before cache
replacement; missing services fail contextually rather than inventing success.
Repeated entry retains existing combat state without selecting another taunt.
Weapon cleanup publishes a candle reset and the HUD reads known logical weapon
state before supplied activation snapshots. Standalone entry validates the selected
weapon against shared inventory. Complete combat gameplay remains unsupported.

`logbook::Logbook` supplies explicit reading layout and navigation from accepted,
separated text entries. It preserves authored style bytes, byte-based word
breaks, original line spacing, newest-entry positioning and ordered scroll input.
**Verified:** public headless tests cover layout, ordinary/page scrolling,
Home/End, simultaneous controls, bounds and close behavior against
[preservation rules](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/hud.md#logbook-reading-layout-and-navigation).
Existing `ScriptHud::entries` retains accepted text and local-state errors;
the screen decodes physical line separators before constructing a reading session.

`logbook::LogbookScreen` owns installed keyboard dispatch, the reading-session
input lock, and suspension through exit-key release. It restores the simulation
checkpoint against the current real BIOS tick and resumes only the virtual clock
it paused itself. `Logbook` retains text layout, scroll bounds and scroll-model
rotation; startup supplies installed artwork and shared screen rendering.

`ScriptHud::snapshot_inventory` captures all item records without selection repair.
`Inventory::from_snapshot` validates the exact installed definitions and returns an
opaque replacement for session candidate assembly. It directly restores counts,
mutable limits, visibility and selection without startup acquisitions or weapon
callbacks. This internal complete snapshot is separate from the original SVIT
codec. Committing inventory with the rest of a restored session remains unfinished.

The explicit standalone-player initializer also establishes preliminary controller
state. `GameplayGlobals::prepare_player_controller` advances its unsigned nominal-tick
countdowns in published order. Pending action and override execution remain
unsupported and stop before ordinary dispatch. Preparing a logical world alone does
not establish these controller values; save restore must retain them directly.

Selected-player collision preparation accepts the standalone animated definition
and logical pose without a map placement. Explicit model refresh resolves its
actor-local frame, and ordinary support queries can retain live map actors as
support. The selected player remains excluded from other actors' live candidate
lists. This preparation does not itself admit automatic keyboard updates.

The synthetic keyboard scheduler contract now exercises installed-style key sampling,
ordinary sheathed forward dispatch, logical pose projection, authored sound delivery
and release continuation. It sends no explicit grounded tick. Complete installed
keyboard traversal and native/browser four-surface acceptance remain unverified.

Ordinary `Activate` and `TorchActivate` calls sample the current logical receiver,
selected player, installed RTX catalog, topic-menu state and held Activate input.
The renderer consumes that shared prompt with nominal-tick holdoff and item
cycling. Explicit inspection inputs remain available. **Verified:** the retained lineup result
is established by standalone player initialization, following
[preservation's lifecycle contract](https://github.com/michidk/redguard-preservation/pull/80).
`PlayerLineUp` movement remains unsupported and invalidates the retained result;
later ordinary activation refuses an unknown result. This does not establish
complete NPC interaction or conversation progression.

`ScriptActors::snapshot_session` captures the current and retained visited logical
worlds, shared globals, HUD, clock, RNG and gameplay toggles. `LogicalSessionSnapshot::plan` validates
installed content and reserves actor generations above both the saved identities
and the host's live high-water mark. Its actor bindings let service owners prepare
replacements before `finish` constructs `LogicalSessionCandidate`. A failed world
validation leaves every live resource unchanged. Capture requires a completed
scheduler boundary outside world loading and New Game transitions.

This is a logical component of a complete save document. Audio transport,
presentation bindings and host lifecycle must join the same candidate before a
coordinated live commit; the logical API alone does not enable save/load or revisits.
A restored registry adopts matching prepared assets on its first plugin update
without rerunning fresh-world preparation. Its restored generation also suppresses
the normal clock warmup restart. Ordinary world release clears that distinction;
subsequent fresh entries retain their existing initialization behavior.
`player::placement::PlayerPlacementSnapshot` retains projected pose, transform,
marker selection and support status, including inspection failures. Its replacement
bundle suppresses marker settling once for matching prepared asset handles. A later
explicit marker or source change follows ordinary entry behavior. The session host
must validate complete source identity and pair this presentation with its restored
logical player before inserting the bundle.
`PlayerRuntime::snapshot/from_snapshot` retains explicit-update entry state and
failures with explicit player/generation/source bindings. Its replacement performs
no grounded update or initialization. The host must restore it alongside placement
and the logical actor so the next explicit update recognizes the same continuation.

Decoded `PlayerScene` assets retain the complete authored cache fingerprint,
including movement settings, terrain and player definitions. World snapshot
sources can include this identity; a complete gameplay session must supply it
for every retained world. Hand-built inspection scenes without decoded provenance
cannot provide this compatibility check. The fingerprint identifies the decoded
input, so hosts must retain trusted prepared assets rather than mutate their fields.

`SoundMaintenance::restore_world` is a restoration prerequisite. It orders RTX,
dialogue and sample cleanup before the supplied actor restoration, then sets the
sample-reset flag. It does not request periodic walk-node cleanup. Cleanup or
restoration failure remains sticky. The ordinary audio host now initializes entry roles, validates retained actors
off-live, clears services and adopts the retained world before maintenance and
traversal. Unsupported hosts retain an explicit traversal guard. Leaving an
unrestored entry preserves its retained world. Final workspace gates pass with
zero installed-data skips. Installed revisit acceptance remains unfinished.

`WaitAnimFrame` and `WaitPlayerAnimFrame` poll the receiver and selected-player
animation owners respectively, without advancing playback. The registry still
requires its usual explicit nominal-tick input for animation-bound worlds; zero
is sufficient for these tasks. They retain full signed group/frame arguments and compare against the
separate wrapping progress counter, following preservation's
[frame-wait rules](https://github.com/michidk/redguard-preservation/blob/fix/soup-property-references/docs/engine/animation.md#frame-waits).
Unprepared animation owners reject explicitly. A missing selected actor completes
the player predicate. Task operands are reevaluated on subsequent invocations;
completion still follows the VM's normal task-slot scheduling.

`script::camera` prepares logical camera replacements off-live from explicitly
initialized records and a host-supplied geometry reader. It retains ordered
segment hits, contact response, movement limits, terrain correction, pose history
and actor position writes. Terrain queries select one retained or map source for
the whole triangle and reject unavailable retained heights without falling back.
The host must validate actor bindings before publishing the replacement. This
boundary does not initialize missing installed settings, establish camera model
eligibility, bind renderer terrain, or enable automatic/scripted camera updates.
The implemented calculations follow preservation's
[camera rules](https://github.com/michidk/redguard-preservation/pull/95).

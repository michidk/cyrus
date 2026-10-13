# cyrus-game

Portable gameplay state, fidelity policy, player input and deterministic controller
rules. Depends on `cyrus-world`, `cyrus-collision` and `cyrus-assets`; it does not load an install,
open sockets, spawn a camera, or play audio.

`GamePlugin` installs app state, fidelity, `PlayerPlugin` and `ScriptPlugin`. The host supplies
Bevy's asset/input/state infrastructure and `Preferences` with a concrete
`PreferenceStore`. Menu load errors remain errors; each consumer owns its fallback.
Successful saves precede runtime application.

Player scene loading prepares `PlayerDefinition`, terrain and static collision
queries asynchronously from validated asset records. The world request carries a
sidecar path rather than depending on this game-owned asset. Inspection adapters
set `InputSuppressed` before `InputSample`; player transforms remain independent
of camera transforms.

Ground-turn, walking startup, wall response and forward support admission are
explicit deterministic APIs. Automatic locomotion, animation eligibility and a
production simulation clock remain unsupported. See
[SETUP](../../docs/SETUP.md) and [ROADMAP](../../docs/ROADMAP.md).

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
checkpoint on exit. Automatic world invocations and actor updates remain unconnected.

World scene roots carry `ScriptScene`. Exported map nodes automatically bind to
their logical actor by placement index and script name. Bindings expire on world
replacement, including reloads of the same world. `initialize_map_animation`
initializes animation-table actors with supplied nominal ticks and retains their
selected frames in the shared SOUP state. Automatic initialization and scheduling
remain pending; scene binding itself does not advance animation or scripts.

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
Walking retains movement/support state and uses that actor's shared animation and
selected file frame. It advances animation before selecting static candidates by
the selected model radius, then updates `PlayerPose`, `Transform`, `AnimationPose`
and `SoundSurface`. The player receives a `ScriptBinding`, so later SOUP requests
also update its presentation. Authored cues use `SoundCue`; native inspection
recognizes these bindings even on unnamed player entities.
Multiple messages remain separate updates; zero nominal ticks can still move.
Stale asset requests and world identities are discarded. Walking errors stop
stepping until entry changes; logical animation failures persist until world
replacement. Marker changes do not create a second playback or reset script tasks.
The caller still supplies admission, timing, orientation, view and collision policy;
no production clock or automatic keyboard-driven locomotion is enabled yet.

The ordinary idle default comes from installed `attr_panic_group`, following
[preservation](https://github.com/michidk/redguard-preservation/blob/976422b/docs/engine/player.md#ordinary-idle-animation-default).
The forward probe uses installed bounds and walk distance with nearest-even
integer rotation, following [preservation PR48](https://github.com/michidk/redguard-preservation/pull/48).
Player caches retain authored frame-source model radii; candidate selection can
resolve the radius by active model name. Missing named radii require re-export.
The upright orientation helper reuses the original movement rotation table.
Callers still supply eligibility, actor orientation, candidates,
surface policy, and shared support state. Falling, water/lava transitions and
special movement states remain unsupported. Gameplay does not yet drive this
update; production timing, actor initialization and player mesh binding remain
incomplete.

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

`placement::initial_animation` applies the selected-player entry requests with
caller-supplied nominal ticks and a synchronous sound handler. It starts with
default group zero, makes both ordinary setup requests, then forces the default.
It retains the selected file frame for presentation, following
[preservation PR50](https://github.com/michidk/redguard-preservation/pull/50).
This explicit entry API does not choose a production clock or enable locomotion.

`script::ScriptPlugin` reports script cache preparation and advances explicitly
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

Native control and MCP `scripts` page through at most 64 actor diagnostics.
`step_script` runs a selected animation, entry, main, interrupt or existing-task stage. The
inspection host supports the researched `GetAttribute`, `GetMyAttr`,
`SetAttribute`, `SetMyAttr`, `SetGlobalFlag`, `ResetGlobalFlag` and `TestGlobalFlag`
services. **Verified:** attribute writes retain the low byte; `SetAttribute`
also maintains reserved collide/weight bytes, while `SetMyAttr` does not. Reads
observe writes in the same stage and routed operands use receiver locals.
`GetMyAttr` and `SetMyAttr` return zero outside the attribute table. Other
out-of-range attribute access and attribute-44 writes remain explicit failures;
the latter requires model/default-group ownership and cleanup integration.
These contracts follow preservation's
[attribute rules](https://github.com/michidk/redguard-preservation/blob/c5cbfb8/docs/engine/SOUP.md#attribute-reads-and-writes).
`ScriptActor::attributes` exposes the prepared inspection bytes. Complete actor
initialization and live player attribute state remain pending. Walking and SOUP
share logical-player playback, while model-default writes remain unsupported.
Flag numeric values and latches
remain separate. **Verified:** self task and multitask `Wait`, `WaitOnTasks`, and
`WaitNonMulti` use preservation's elapsed and unpaused-slot rules. The tasks stage
requires `frame_scale` with eight fractional bits; omission fails before actor
mutation. Native wait tokens belong to each logical instance. Initial waits always allocate;
completion applies on a later task pass, including duration zero and one. Function-encoded
waits remain unsupported because their timing context is not established.
Prepared spatial hosts also support `RotateByAxis`, `PlayerDistance`, and
`PlayerFacing` with the explicit inputs and limits described below. Remaining
native services are explicit failures. End notifications with synchronization
channels remain unsupported. These tools do not start New Game.

**Verified:** fresh inspection prepares standalone CYRUS, GREMLIN and CAMERA
instances from installed definitions, after ordinary map actors. They have separate
locals and task slots. Player defaults to CYRUS; Camera uses its own instance.
`ScriptActorSource` distinguishes those roles from map placements. Type-2/type-6
map records retain diagnostics for their separate unsupported object paths and do
not consume ordinary actor locals. Status `instances` counts logical entries,
including standalone roles and unavailable map records.

Native routing supports Me, Player, Camera and installed RANM names with ASCII
case folding. Name lookup uses signed 16-bit runtime identifiers, preserves first
registration of an identifier, and chooses the first matching namespace. Initial
map identifiers follow installed actor-pool capacity, activation flags and placement
order, independently of MPOB object ids. Functions use receiver locals and
attributes; tasks occupy receiver slots while the caller continues. Foreign tasks
retain source instructions. Both runtimes are restored on errors, and failures
remain sticky. These rules follow preservation's
[SOUP contract](https://github.com/michidk/redguard-preservation/blob/1242ac1/docs/engine/SOUP.md).
This is fresh logical preparation. Player switching, variable references, scriptless
receivers, later allocation/recycling, reentrant callbacks, automatic scheduling,
progression and saves remain unsupported.

`ScriptWorld::bind_animation` attaches animation state to the logical instance
only after world/content identity checks. The caller supplies an established
default group. `advance` runs one animation, ordinary entry, task, or body stage;
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
Their zero results follow the [preservation sound contract](https://github.com/michidk/redguard-preservation/compare/main...docs/soup-sound-results).
Positional/ambient sound, shell/code-object owners, `KillMe` and the script
`StopAllSounds` result remain unsupported.

**Verified:** ordinary entry suppresses execution when an unpaused blocking task's
physical operand identity is at or before the saved entry cursor. Direct
interrupts bypass that entry gate, following preservation's SOUP contract.
**Verified:** [preservation PR54](https://github.com/michidk/redguard-preservation/pull/54)
establishes signed group comparison, finite-count writes only before animation
task startup, and distinct model/playback default state. Attribute-44 writes
remain unsupported because model state, actor-mode eligibility and navigation
reservation ownership are not integrated. The inspection default is not a
replacement for original state-dependent selection.

`ScriptWorld::prepare_spatial` prepares ordinary map poses and unique attachment
families. Separate map-object paths and standalone role poses remain unavailable.
`ScriptInputs` supplies nominal ticks, frame scale, and current query inputs to
explicit invocations. `RotateByAxis` shares the native task store with waits and
animation, mutating local angles without implicitly refreshing matrices or
attachments. Snapshot, unattached transform refresh, and family refresh are
separate operations. Spatial preparation rejects ambiguous or cyclic families;
attachment-sensitive attribute changes remain unsupported after preparation.

**Verified:** [preservation PR55](https://github.com/michidk/redguard-preservation/pull/55)
defines rotation task arithmetic, player distance/facing queries, and local versus
composed attachment transforms. Actor inspection exposes both poses and the
previous-angle/delta fields. Attached queries retain the previously composed pose
until refresh. Override, fallback and linked-position inputs are explicit; the
script host still rejects unattached linked-position queries until their state
owner is integrated. This does not establish automatic actor scheduling or the
complete lever interaction.

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

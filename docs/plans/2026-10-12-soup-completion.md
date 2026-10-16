# SOUP completion plan

This is the remaining implementation plan for running shipped SOUP content on
native and browser builds. It updates the priorities in the
[original integration plan](2026-10-11-gitnexus-plan-soup-runtime-game-integration.md).
Do not repeat its already implemented crate creation, cache export, or VM migration.

**Verified below means inspected source, not complete game fidelity.** Recommendations are **Inferred**. Unresolved original behavior is
**Unknown** until established from installed data or preservation evidence.
The original planning baseline was cyrus `a5bbaa8`. Runtime verification for
subsequent increments is recorded in the roadmap; unchecked items remain open.

## Current implementation

The first automatic runtime acceptance is New Game into START: eligible actors
must advance their authored scripts and rendered animation without `step_script`
or manually bound playback. Correct world loading and a passing build do not
satisfy this requirement. Repeat the same acceptance across installed worlds.

**Verified:** the implementation is a portable execution core with explicit game
hosts and inspection tools. Normal world loading prepares scripts but does not
automatically execute all eligible actors. `ScriptPlugin` prepares the registry,
updates diagnostics, and advances explicitly attached animation playback. With a
matching camera view, ordinary traversal now runs supported stages automatically.
**Verified:** installed ISLAND FLAG2 advances through normal Bevy updates. START's
collision/combat/intro services and complete all-world acceptance remain open.

- `cyrus-assets/src/script.rs` and `script_export.rs` retain installed definitions,
  initial flags, locals, strings, initialized attributes, native hooks, installed update switches, placement model
  metadata, and decoded programs. The script cache is version 15, programs are
  version 3, and content
  identity is validated. Attribute-load defaults use installed SYSTEM.INI through
  preservation [PR59](https://github.com/michidk/redguard-preservation/pull/59).
  RAHK retains a typed native call, distinct from the VM entry continuation.
  Explicit native-hook dispatch preserves both continuations and existing tasks.
  Automatic hook eligibility and remaining native services remain open.
  Model-path message-group assignment and
  complete initialization remain open.
  Unsupported decoding remains visible.
- `cyrus-script` implements values, control flow, main and interrupt continuations,
  the shared actor return continuation, six task slots, operand reevaluation,
  pause/resume/stop, receiver dispatch, and sticky failures. Callers supply stages
  and timing. Completed effects without established numeric results continue as
  statements and fail contextually when consumed as values, including routed calls.
  Serializable state is not a working save implementation.
- `cyrus-game/src/script/registry.rs` prepares logical map actors and standalone
  player, gremlin, and camera roles. It guards world generations and supports
  Me, Player, Camera, and named receivers. Inspection handlers support attribute
  reads/writes, flag helpers, Wait, WaitOnTasks, and WaitNonMulti.
  Rnd and QuickRnd consume an explicit shared game random resource retained across
  world replacement. Complete random consumer order and sky integration remain open.
  Ordinary map actors also support EnableScript/DisableScript and an explicit
  ordinary-main stage that retains pending tasks while disabled. The installed
  task-system switch suppresses that stage; ordinary-hook admission distinguishes
  paused slots from all unpaused tasks, including multitasks. Task bypass
  preserves the entry gate but skips polling and the main blocking-slot gate.
  Caller-admitted `beginCombat` now shares authoritative weapon cleanup/draw,
  lifecycle entry and ordered taunt presentation with explicit world calls.
  It completes without a portable numeric result. Prepared taunt catalogue,
  selected identity and admission inputs remain explicit; automatic combat
  eligibility and complete combat gameplay remain separate.
- The registry host combines PushAnimation, waits, attributes, and flags on one
  logical actor. Native task tokens are shared, including routed receiver tasks.
  The standalone animation adapter reuses that task handler. Explicit animation,
  ordinary entry, task, and body stages still require caller-established timing
  and eligibility. Explicit spatial preparation adds local/composed poses,
  RotateByAxis, MoveByAxis and sMoveByAxis tasks, ordinary AtNodeMarker queries, and player queries with supplied global inputs.
  Fresh linked-position state is receiver-owned; dynamic linked records and
  attachment changes remain unsupported.
  Admitted walking updates share the logical player's animation and selected
  frame with SOUP, using generation-bound messages and a scene binding. Model
  defaults and complete actor initialization remain separate unfinished work.
  These paths still require the installed mixed-scenario acceptance below.
- Native control and MCP expose bounded `scripts` inspection, explicit animation
  binding, and `step_script`. Shared scene bindings read the logical actor's pose
  and expire with its world generation. World scene roots bind exported map nodes
  automatically by placement index and script name. Animation-table initialization
  retains the selected frame but still requires caller-supplied timing.
  Native and browser load the shared cache
  and game plugin. That wiring does not establish browser execution parity.
- Public-interface tests exist in `cyrus-script/tests/runtime.rs`,
  `cyrus-game/tests/script_registry.rs`, `script_instances.rs`, animation tests,
  and `cyrus-assets/tests/script.rs`. They cover many isolated contracts, including
  task routing, waits, attribute effects, stale identities, and cache validation.

**Verified limitations:** property reads/writes default to unsupported in `Host`.
Variable object references, scriptless receivers, later allocation/recycling,
player switching, reentrant callbacks, model-coupled attribute writes, hooks,
synchronization, production scheduling, most native commands, progression,
revisits, and saves remain incomplete. Decoder unsupported counts do not count
all missing host commands and cannot measure gameplay completeness.

## Existing documentation and roadmap placement

- [Original integration plan](2026-10-11-gitnexus-plan-soup-runtime-game-integration.md):
  architecture, evidence links, verification cases, and final completion criteria.
  Its early current-state description predates the implementation. Its final
  corrections override earlier assumptions about return continuations and tasks.
- [Script README](../../crates/cyrus-script/README.md): current core contract and limits.
- [Game README](../../crates/cyrus-game/README.md): current hosts, routing, and waits.
- [Roadmap](../ROADMAP.md): stage 1 owns automatic world script execution;
  stages 3 and 4 own interaction, persistence, and complete gameplay;
  stage 5 owns full-game verification. The active SOUP task owns the remaining implementation checklist.
- [Method](../METHOD.md): original-game evidence and reproducible comparisons.

**Verified:** bounded automatic keyboard walking has synthetic scheduler coverage
and an installed headless ISLAND idle/forward/release probe with explicit view
context. Complete installed traversal and four-surface acceptance remain open.
The separate player-controller owner now owns movement, input, placement/runtime
and collision work. Runtime/placement snapshot APIs and restored-entry preservation
are checkpointed for handoff; final isolated checkpoint gates remain pending.
Private walking-phase serialization stays with the unfinished SOUP registry snapshot.
SOUP owns the ordinary-stage caller and authoritative registry.
Changes to their shared contract require coordination; explicit walking replays
do not establish complete automatic keyboard acceptance.

## Persistence status and ownership

The detailed roadmap is the current record of landed component contracts. The
unchecked milestones below describe complete behavior, not absence of component
work. VM, native tasks, animation, clock, flags, inventory, journal, dialogue,
HUD, source audio/CD and texture/terrain replacements have landed.

**Verified:** the corrected-export native capture represents current ISLAND and
visited START, including 173 ordinary model associations and the player model.
Fresh-process native load and recapture preserve both worlds after actor-generation
rebinding. The integrated browser diagnostic caller passes CPU-only typed
IndexedDB restore, save and fresh-page reload using matching independent exports.
Integrity-valid current and dormant source mismatches reject without changing
accepted registry, player, HUD or model state. Failed and aborted publication
preserves the accepted document.

Shared aggregate preparation, rendered replacement and coordinated commit remain
an unfinished gameplay checkpoint. Native ISLAND-to-START-to-ISLAND revisits
retain actor/task state, but pre-existing controller/combat and unsupported-service
failures remain. Browser revisits, complete rendered/audio continuation and
end-to-end gameplay restoration remain open. Source-decoder checkpoints do not
include downstream converter/device queues. The native diagnostic load caller
prepares sources off-live and retains completion until acknowledgement.

Active boundaries are recorded in the roadmap. The SOUP owner retains
`cyrus-game/src/script`, shared session assembly and the native load caller.
The storage owner retains browser session storage/caller and publication/reload
acceptance. The controller owner retains `cyrus-game/src/player` and collision.
Preserve existing drafts when taking ownership; do not replace snapshot contracts
or move live state between these owners without coordinating the interface.

Independent work can cover Smacker decoder correctness within `cyrus-smacker`
without changing startup/audio callers, portable VM rejection/continuation tests
within `cyrus-script/tests` without changing host APIs, and retained-evidence
research for unsupported SOUP services. Rendering/HUD comparisons can proceed as
evidence work, but runtime persistence, camera/sky, texture/terrain and actor
prelighting integration currently intersect the aggregate draft. Stopped original
and browser previews remain stopped unless the maintainer authorizes a restart.

## Ordered implementation checklist

The order and milestones below are **Inferred** recommendations. Each item ends
in a reviewable result. Move a command earlier when the selected installed
scenario requires it. Do not substitute guessed behavior or successful stubs.

### 1. Establish the remaining content and evidence inventory

- [x] Extend existing installed export tests/tooling to enumerate every WORLD.INI
  entry, including palette variants and standalone roles. Record instructions,
  operand/reference forms, hooks, native calls, and unsupported actor paths.
- [x] Separate declared, statically present, exercised, and original-verified
  operations. Map each used operation to its current host, missing dependency,
  evidence, and acceptance scenario. Keep generated installed content in `magic/`.
  The census covers all 29 installed entries. The local support matrix marks
  exercised/original-verified status unknown where only static evidence exists.
- [x] Select one installed interaction that combines a wait, flag mutation, and
  visible response. PI_LVRA has an installed supplied-stage test covering its
  prerequisites and logical outcomes; playable acceptance remains below.

Exit: every encountered form has a status and owner; the first scenario has a
reproducible setup. A static inventory makes no reachability or parity claim.
Owners: asset export/tests and preservation parsers for missing records.

### 2. Connect one authoritative actor state to the game host

- [x] Reuse the existing registry and VM. Connect animation and immediate
  attributes/flags/waits through one game-owned host without introducing another
  interpreter or duplicate mutable actor state. The installed PI_LVRA test runs
  player animation, waits, activation, sound intents, rotation and a flag change
  through supplied stages. Automatic gameplay acceptance remains separate.
- [ ] Establish shared ownership of model, default animation group, attributes,
  player state, and task tokens. Resolve model-coupled attribute writes against
  preservation before supporting them.
- [ ] Complete needed property and variable-reference access, scriptless
  receivers, special map object paths, player switching, allocation/recycling,
  and reentrant dispatch in evidence-backed increments.
- [ ] Preserve synchronous read-after-write effects, receiver task ownership,
  physical operand identity, and contextual sticky errors. Test removal and
  replacement while a foreign task is pending.

Exit: an explicit stage can run the selected installed mixed-command sequence
and update the same authoritative actor observed by animation and diagnostics.
All receiver forms required by the inventory have tests or explicit blockers.
Owners: `cyrus-game/src/script`, world animation/lifecycle, and domain state.

### 3. Complete scheduling, interrupts, hooks, and synchronization

- [ ] Recover unresolved timing and eligibility rules in preservation. Define
  ordinary actor order, selected-player stages, task/body order, pause behavior,
  interrupt suppression, hook dispatch, and synchronization release.
- [ ] Keep explicit stepping as the deterministic test entry point. Add the
  automatic scheduler only for established rules, with shared native/browser
  execution and no actor allowlist or arbitrary render-frame timer.
- [ ] Cancel old-world tasks and effects before replacement. Reject stale
  callbacks, including same-world reloads and shared-map variants. Preserve the
  current world when destination preparation fails.
- [ ] Compare zero-tick, multiple-invocation, pause/resume, interruption, and
  same-stage completion sequences with the original. Calibrate the diagnostic
  instruction budget against executing content without turning it into a yield.

Exit: eligible actors run automatically in the correct order; a deliberately
wrong ordering or timing rule fails the comparison. Full clock fidelity is
**Unknown** today and is a blocker, not a constant to choose during coding.
Owners: game scheduler, animation/player timing, lifecycle, preservation research.

### 4. Finish one playable script scenario across hosts

- [ ] Run the scenario selected in item 1 automatically, with its real installed
  scripts, objects, state changes, and visible/audio effects.
- [ ] Compare the original with native and browser behavior, plus headless state.
  Add a replay fixture containing inputs rather than captured game data.
- [ ] Exercise interruption, world exit, and re-entry. Preserve FLAG2 supplied-tick
  animation contracts as regression coverage, not proof of general gameplay.

Exit: the scenario works without debug stepping or replacement content. If it
requires the parked player controller, retain explicit verification and mark
the playable acceptance blocked until that dependency is resumed.

### 5. Complete native services by shipped-content dependency

- [ ] Audio and presentation: sound ownership/cancellation, ambient tasks,
  speech/subtitles, HUD prompts, bitmap slots, and logbook events. Reuse existing
  sound and HUD paths and verify task completion/cancellation against them.
- [ ] World behavior: lifecycle/visibility, groups, attachments, movement,
  rotation, platforms, collision/spatial queries, lights, and effects.
- [ ] Interaction: player lockout/restoration, inventory and item use, dialogue
  choices, and scripted camera ownership. Feed the same state to HUD and saves.
- [ ] Gameplay: navigation/perception, combat/projectiles, health/death,
  puzzles, cinematics, and script-requested world transitions.
- [ ] Give every used handler a verified contract, success scenario, and relevant
  failure/interruption tests. Keep unused or unresolved declarations explicitly
  classified rather than silently returning zero or immediate completion.

Exit: no operation required by shipped progression lacks an implemented,
verified service. VM opcode coverage alone cannot close this item.
Owners: game-domain handlers, existing rendering/audio consumers, and preservation.

### 6. Implement fresh games, revisits, and persistence

- [ ] Establish reset and retention rules for globals, actors, locals, tasks,
  inventory, dialogue/quests, and world changes from original evidence.
- [ ] Implement real New Game initialization and revisits before treating menu
  Load as a restored game. Define allowed save points from the original.
- [ ] Restore versioned logical identities, continuations, tasks, and host state
  with content compatibility checks. Recreate presentation resources afterward.
- [ ] Use atomic native writes outside the install and transactional browser
  storage. Validate before replacing live state or overwriting durable saves.
- [ ] Compare save/restart/resume with uninterrupted execution, including waits,
  foreign tasks, transitions, corrupt saves, and failed writes.

Exit: supported save points and revisits preserve progression on both platforms.
Original save-file import remains a separate format requirement.

### 7. Close full-game coverage

- [ ] Exercise all worlds, alternate quest order, deaths, interrupted dialogue,
  repeated transitions, and the ending. Close encountered unsupported paths.
- [ ] Compare native/browser outcomes and timing with reproducible original
  scenarios. Prove selected comparisons reject a deliberately wrong behavior.
- [ ] Update roadmap support only after acceptance. Retain distinct milestones:
  complete VM semantics, complete automatic engine integration, and complete
  shipped-game progression with persistence.

Exit: a verified new-game-to-ending playthrough and required alternate/failure
paths, with no silent exclusions. Do not report a completion percentage from
function counts or cache preparation.

## Verification for implementation work

Use the narrowest existing public-interface tests for each change. Preserve
asset validation, VM, registry, animation, sound, and render contracts as their
owners change. Add tests for behavior rather than internal data arrangements.
Run install-dependent checks with the install present and report any skips.
Execute shared fixtures on native and WebAssembly; compilation alone does not
prove browser runtime parity.

Each runtime increment requires its relevant original comparison or headless
replay, followed by one final `just gates` for code changes. Check portable
boundaries and browser builds when their inputs change. Keep per-worktree build
outputs separate and all game data/research under ignored local directories.
Publish newly recovered engine facts in preservation before depending on them.

Documentation-only updates need `dev/scripts/publish-check.sh` and
`git diff --check`, without Cargo. No compiler, editor, JIT, mod API, or new
generic scripting framework is needed for this completion plan.

## Current research dependencies

**Verified:** [preservation PR54](https://github.com/michidk/redguard-preservation/pull/54)
publishes animation operand widths, walk-node reservation cleanup, and the
distinction between model default and selected playback default. The
ordinary map allocation state and the separate combat death default are now
owned by the registry. Live map attribute-44 writes are supported; dead-actor
reservation cleanup and standalone role writes remain unsupported. Corrected
write guards and allocation defaults are published in
[preservation's rules](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/animation.md#living-actor-default-selection).
The installed TS_RP sequence still requires automatic default selection and scheduling.

**Unknown:** faithful automatic host pacing, complete actor traversal/admission,
state-dependent defaults, hook/synchronization release, and progression retention.
These block the automatic gameplay and persistence milestones. Explicit stages
and synthetic mixed-command tests do not close them.


**Verified:** the installed PI_LVRA lever is a small mixed-command acceptance
candidate. Its prerequisites include player distance/facing queries, activation,
player animation, positional sound, and object rotation.
Spatial queries, attached transforms and rotation are connected through explicit
supplied stages and verified by a native headless PALACE replay. The installed
mixed-command test also covers activation with packed RTX keys, shared player
animation, FlatSound intents and the authored flag change. Automatic actor inputs,
sound playback comparison and production scheduling remain open; supplied stages
do not establish playable acceptance. **Inferred next implementation order:**

1. Connect the player-query and rotation-task contracts published in
   [preservation PR55](https://github.com/michidk/redguard-preservation/pull/55).
   Complete automatic activation inputs and sound playback comparison before
   playable scenario acceptance.
2. Establish attached transform ownership and refresh, then connect local
   rotation, effective query coordinates, and player animation to authoritative
   logical state, including cancellation and the established playback default.
   **Verified:** this lever uses attached coordinates. Placement-only distance
   and facing handlers cannot satisfy its authored sequence.
3. Run the authored lever sequence under explicit supplied stages, then compare
   its flag, transform, animation and sound outcomes with the original.
4. Enable automatic acceptance only after production invocation pacing and
   eligibility are verified; repeat in the browser and across world replacement.

**Verified:** periodic sound-maintenance inhibition is sampled by the player
controller, rather than every ordinary object update, as documented in
[preservation](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/sound.md).
Automatic reset integration must preserve that admission point and the ordered
RTX-cache, dialogue, sample-channel and walk-node cleanup. The explicit
`SoundMaintenance` timer does not establish this runtime ownership.

## Unfinished session checkpoint

The shared session implementation is committed as work in progress. It includes
current/visited-world documents, source validation, replacement candidates,
render/audio staging, boundary capture/commit and asynchronous load preparation.
Native and browser diagnostic capture hooks are present. This does not enable
accepted end-to-end save/load.

Focused public tests cover delayed-asset waiting, commit, malformed-candidate
rejection without live replacement, and direct-candidate/audio continuation.
The full gate for runtime checkpoint `66340ca` passed 947 Rust test executions,
with zero failures, ignored tests or installed-data skips.

Native diagnostic load reached ready ISLAND gameplay from START and recaptured
both worlds. Retained START matches after actor-generation rebinding. An altered
retained-world source is rejected while the accepted world and generation remain
live. These checks do not establish complete rendered continuation. Ordinary map
actors could select another animation model without replacing their original scene
geometry. The current draft adds that replacement owner and its session staging;
the checkpoint passed full gates with zero installed-data skips. Installed native
load and recapture now retain all 173 ordinary model replacements with matching
displayed poses and unique roots, plus unchanged dormant START state after fresh
actor-generation binding. Pixel comparison and complete rendered/audio
continuation remain unverified.

The diagnostic browser caller is integrated. Remaining work includes complete
rendered/audio restoration, restart/reload and visited-world continuation. The revisit draft now initializes entry roles, validates retained actors, clears
shared sound services and restores saved execution state before traversal.
Cancellation preserves the retained world. Final workspace verification passed
955 Rust test executions with zero failures, ignored tests or installed-data
skips. Installed ISLAND-to-START revisit preserves 34 retained actors, task
identities and player records; seven invocation counters continue and departed
ISLAND remains unchanged. Eighteen pre-existing controller/combat update errors
remain. Returning to ISLAND preserves all 975 actor records exactly and leaves
dormant START unchanged; its 70 pre-existing failures persist. Complete gameplay
revisit acceptance remains open.
Audio snapshots cover source-decoder continuation, not downstream converter/device
queues. Complete gameplay and controller acceptance remain open.

### Authored light lifecycle draft

The current implementation retains authored type-six lights, applies `LightOff`,
and preserves disabled state through logical replacement and material staging.
Four public registry contracts, installed-map light projection and the renderer
texture replacement test pass. Clippy and the WASM session check pass; full
workspace gates pass with zero installed skips. Installed rendered light comparison
remains pending.
The rules follow the [published light lifecycle](https://github.com/michidk/redguard-preservation/compare/main...docs/soup-light-lifecycle).
Dynamic light creation, modulation, type-four light controls and separate-record
movement remain unsupported. This does not complete the light/native-service row.

### Player-looking query draft

`PlayerLooking` now routes through the existing spatial query owner. Its signed-width
bearing helper matches 272 retained-instruction vectors, including signed-minimum
differences. Three public argument, position-priority and script-dispatch tests pass.
Final workspace gates and the WASM check pass. Installed automatic traversal remains pending.
This does not establish perception, line of sight or complete installed traversal.

### Portable current and visited fixture

**Verified:** a fresh native capture using portable placement exports retains
current ISLAND and visited START, including 173 ordinary rendered model replacements
and the player model. The copied native cache matches the capture runtime byte-for-byte.
Source, executable, document and cache manifests accompany the local fixture.
Browser acceptance of this corrected-export fixture, rendered/audio continuation
and complete gameplay revisits remain open.

### Native corrected-export restoration

**Verified:** fresh-process native load and recapture of the portable current ISLAND /
visited START fixture preserve all retained world state except validated fresh actor
generation references. All 173 ordinary rendered model replacements and the player
retain model, frame, source, transform and visibility. HUD and scene identity match.
Integrity-valid current-world and dormant-world source mismatches both reject.
Recapture after those failures preserves registry, player, HUD, random/toggle state,
entity bindings and model replacements exactly. Pixel/audio continuation and complete
successful gameplay traversal remain open.

### Retained supporting-actor query

`PlayerStand` reads the existing selected-player model-support owner. Three public
identity, routing and rejection contracts pass. The installed non-standing
KOTAROTB branch runs without changing flags or support. The published original
identity comparison and its altered-branch control pass against retained instructions.
WASM verification and final workspace gates pass with zero installed skips. This does not establish
collision assignment, the standing dialogue route or automatic gameplay acceptance.

### Receiver-relative player arc query

The [published PlayerArc contract](https://github.com/michidk/redguard-preservation/compare/main...docs/player-arc-contract)
establishes receiver heading, attached/linked/local position priority, signed offset
and inclusive angular tolerance. Complete retained helper execution and altered
controls verify that bounded query, including signed-offset extremes. The registry
now routes it through the existing spatial query owner. Public and installed
CAMPDIRG distant-player contracts pass. A double-negation error was caught by
the cardinal-direction contract and corrected by using the existing bearing
helper directly. WASM verification and combined-tree gates pass with zero
installed skips. Complete dialogue and automatic gameplay
acceptance remain open.

### Local heading property and full-width references

The preservation decoder and portable runtime retain complete 16-bit property
references. Program schema 3 rejects older exports that may have truncated them.
The registry resolves installed reference names and supports local `angle.vy`
reads through its existing spatial owner. Other properties and writes remain
explicitly unsupported. Public contracts and the installed shrine non-standing
branch pass with explicit player pose and timing inputs. The WASM check passes.
Final workspace verification remains pending after correcting stale schema
versions in native/render test fixtures. Complete shrine interaction remains open.

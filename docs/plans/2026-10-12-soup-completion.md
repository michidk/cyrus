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

**Verified:** the implementation is a portable execution core with explicit game
hosts and inspection tools. Normal world loading prepares scripts but does not
automatically execute all eligible actors. `ScriptPlugin` prepares the registry,
updates diagnostics, and advances explicitly attached animation playback.

- `cyrus-assets/src/script.rs` and `script_export.rs` retain installed definitions,
  initial flags, locals, strings, initialized attributes, hooks, placement model
  metadata, and decoded programs. The script cache is version 6, programs are
  version 2, and content
  identity is validated. Attribute-load defaults use installed SYSTEM.INI through
  preservation [PR59](https://github.com/michidk/redguard-preservation/pull/59).
  Model-path message-group assignment and complete initialization remain open.
  Unsupported decoding remains visible.
- `cyrus-script` implements values, control flow, main and interrupt continuations,
  the shared actor return continuation, six task slots, operand reevaluation,
  pause/resume/stop, receiver dispatch, and sticky failures. Callers supply stages
  and timing. Serializable state is not a working save implementation.
- `cyrus-game/src/script/registry.rs` prepares logical map actors and standalone
  player, gremlin, and camera roles. It guards world generations and supports
  Me, Player, Camera, and named receivers. Inspection handlers support attribute
  reads/writes, flag helpers, Wait, WaitOnTasks, and WaitNonMulti.
- The registry host combines PushAnimation, waits, attributes, and flags on one
  logical actor. Native task tokens are shared, including routed receiver tasks.
  The standalone animation adapter reuses that task handler. Explicit animation,
  ordinary entry, task, and body stages still require caller-established timing
  and eligibility. Explicit spatial preparation adds local/composed poses,
  RotateByAxis tasks, and player queries with supplied global inputs. Linked
  position ownership and dynamic attachment changes remain unsupported.
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

**Verified:** the roadmap records unresolved production invocation pacing and parks the
player controller at the user's request. This plan does not resume that work.
Script research, explicit host integration, and world inspection can proceed;
player-dependent acceptance remains a separate dependency.

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
distinction between model default and selected playback default. Both
attribute-44 write paths remain unsupported here until those state owners exist.
The installed TS_RP wait/animation sequence requires that missing integration.

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

# SOUP runtime and game integration plan

For the current implementation inventory and remaining ordered checklist, use
the [SOUP completion plan](2026-10-12-soup-completion.md). The early baseline
below predates the portable VM and inspection hosts now implemented.

## 1. Objective

Implement the installed game's SOUP scripts across native, headless, and browser builds, including the engine services those scripts require. Deliver this in tested increments, ending with playable scripted progression and resumable saves.

This is an implementation proposal, not a claim that SOUP or gameplay is complete. **Verified** means current source or published preservation documentation was inspected. **Inferred** means a design recommendation based on that evidence. **Unknown** marks behavior requiring research before implementation. All current-code findings below are source-derived.

Evidence baseline: cyrus `1c5f643134e9768eaff6fd30dd8a72ca20fcbba0`; preservation `e604d9c6924116706d37c5fb66fa1aa43269cb4d`; RG Unity `678905846bec4942bf1492c54fdd6f479b1482fe`. The preservation and RG Unity revisions matched their remote HEADs when checked. The cyrus dependency is pinned to an older preservation revision; research availability does not imply API availability in the build.

Evidence provenance uses schema 2. Global dirty digest: `0a9c85780067d9afcd0764f307b60891e3cee927ee11eaeb5ec7826d10fd82cd`. The context pack contains 20 sorted cited paths; only this generated plan is excluded.

## 2. Current behavior

**Verified:** `cyrus-assets` retains original script inputs and exports a versioned `Program` containing literal tasks, `End`, or an explicit unsupported operation. Its exporter uses preservation's `decode_literal_script_step`, not the textual disassembly as executable input.

**Verified:** `AnimationScript` in `cyrus-world` executes literal `PushAnimation` and `End` only. It rejects interrupt entries and synchronization channels, preserves request/start/wait phases, and retains failures so a failed sound cue is not retried. `AnimationPlayback::new_scripted` combines it with `ActorAnimation` and supplied `AnimationTick` messages. There is no production clock or automatic actor script activation.

**Verified:** `PreparedWorld` supplies shared cache references. Native scene preparation exports installed resources; the browser loads exported caches. Metadata readiness alone does not mean player, graphics, or future script dependencies are ready. `GamePlugin` currently registers player rules, preferences, fidelity, and app state.

**Verified:** the roadmap already assigns scripts and world activation to stage 1, player integration to stage 2, and dialogue, inventory, transitions, and persistence to stage 3. This plan expands those commitments rather than creating a parallel roadmap.

## 3. Ownership and architecture

**Inferred recommendation:** add one workspace crate, `cyrus-script`, for a portable SOUP execution core. Keep its execution API independent of Bevy, graphics, audio, files, networking, and the native preservation crate. Its separate boundary is justified by shared native/browser execution, deterministic tests, and serializable continuations.

Use the following dependency direction:

- `rgpre` owns installed format parsing, typed bytecode decoding, definition metadata, and documented engine semantics.
- `cyrus-assets` owns portable program records and cache validation, plus native export through `rgpre`.
- `cyrus-script` depends on portable records from `cyrus-assets`, and owns expressions, continuations, calls, tasks, and VM errors. This retains the existing record ownership. Because assets currently uses Bevy asset types, the initial core has a transitive Bevy dependency; it must expose no Bevy types and import no Bevy APIs. A separate data-only crate is unnecessary for this feature.
- `cyrus-game` depends on the VM and `cyrus-world`. It owns actor scheduling, script-visible gameplay state, native command handlers, and authoritative completion results.
- `cyrus-world` continues to own animation, collision, world identity, and sound intents. It must not depend on `cyrus-game` or implement a second SOUP runtime.
- `cyrus-render` presents game state and consumes ordered effects. Native and browser hosts prepare resources and persist local state.

Do not put all 367 declared handlers in the VM. Do not create one crate per command family. Group handlers by the game subsystem that owns their state. Confirm this ownership with the current animation/player work before moving shared code.

## 4. Research findings

GitNexus `list_repos` returned no cyrus index. No LSP navigation tool was exposed. No index refresh or local agent-state inspection was performed. The plan uses targeted source reads, with no graph-derived completeness claims.

**Verified, preservation:** [SOUP](https://github.com/michidk/redguard-preservation/blob/e604d9c6924116706d37c5fb66fa1aa43269cb4d/docs/engine/SOUP.md) describes a register-free interpreter without an operand stack, mode-dependent operands, local variables, global flags, separate main/interrupt continuations and call stacks, cooperative tasks, and native function dispatch. `End` yields with a stored continuation; it does not permanently stop the script. Nonblocking instructions run until a blocking operation or yield, rather than one instruction per render update.

**Verified, preservation:** [SOUP386.DEF](https://github.com/michidk/redguard-preservation/blob/e604d9c6924116706d37c5fb66fa1aa43269cb4d/docs/engine/SOUPDEF.md) supplies names and declaration metadata. Declaration does not establish that a function is called by shipped content. No standalone `.AI` files ship with the documented releases, so a development-file loader is not a prerequisite.

**Verified, preservation source:** `src/import/soup_def.rs` currently retains function names and argument counts but not declaration kinds. It retains flag names/types but not authored initial values. The parser also substitutes zero for an invalid argument count. These are upstream prerequisites for strict runtime loading, not reasons to hardcode tables in cyrus. Resolve the documentation/parser differences in section syntax and null-call encoding against installed data before expanding the decoder.

**Verified, preservation source:** `src/import/rgm/script_step.rs` provides typed decoding only for literal Task/End. `src/import/rgm/script.rs` has broader textual disassembly, but formatted strings are not a runtime instruction representation. Extend a typed decoder upstream and share decoding rules where practical.

**Verified, reference implementation only:** RG Unity's [RGRGMScriptStore.cs](https://github.com/RGUnity/redguard-unity/blob/678905846bec4942bf1492c54fdd6f479b1482fe/Assets/Scripts/RGFileImport/RGMData/RGRGMScriptStore.cs) contains operand decoding, separate thread readers, and call stacks. Its `tickScript` reads one instruction per thread, while `TaskPause` and `ScriptRV` have unfinished behavior. [RGScriptedObject.cs](https://github.com/RGUnity/redguard-unity/blob/678905846bec4942bf1492c54fdd6f479b1482fe/Assets/Scripts/RGFileImport/RGMData/RGScriptedObject.cs) demonstrates main-task and multitask integration. Use these as a research checklist, not copied code or proof of original timing.

## 5. Ordering constraints

No statement-level PDG is available. The following constraints come from current code and preservation's [animation rules](https://github.com/michidk/redguard-preservation/blob/e604d9c6924116706d37c5fb66fa1aa43269cb4d/docs/engine/animation.md).

- **Verified:** existing playback advances animation, requests the selected default, then advances the script. A script's immediate animation request can reuse the same supplied tick input. Migration must preserve separate invocations within one update.
- **Verified:** the original ordinary actor stage performs eligibility checks, animation/default work, intervening movement, then enabled script entry/task/main work. Script-control flags and the global task switch gate stages separately. Interrupt invocation has separate suppression rules.
- **Verified:** current script errors are sticky. A runtime replacement must not retry an already-issued effect when execution is queried again.
- **Unknown:** complete host timing, interrupt/task interaction, synchronization, and all native handler semantics are not established merely by the bytecode specification. Resolve each needed contract before automatic execution.

**Inferred:** make ordered execution synchronous where scripts observe the result immediately. Deferring every command to the next Bevy frame would change read-after-write behavior. Queue presentation effects only after authoritative state changes, and preserve command order and task identity in completion responses.

## 6. Proposed changes

### Programs and installed data

Extend the typed decoder in preservation for all documented instruction forms and operand modes. Return structured expressions, calls, branch targets, and source-relative instruction identities. Preserve unsupported operations explicitly, with a distinction between malformed input and valid but unimplemented behavior. Validate reachable instruction boundaries without assuming a linear scan describes every entry point.

Expand `cyrus-assets::script::Program` into a complete portable record. Retain main and interrupt entry information, hook metadata, locals, strings, attributes, installed references/functions, and global flag defaults. Keep names and numeric identifiers tied to the installed definition rather than maintaining a copied function table. Retain numeric bit patterns until a verified handler interprets them as integers, text keys, item IDs, or world IDs.

Add a versioned `.scripts` sidecar and retain its path/handle with `PreparedWorld`. Script lifetime then does not depend on animation geometry being present. Include a stable content identity covering the program and definition metadata for cache/save compatibility. Export all WORLD.INI entries, including shared-map variants. Stage a complete cache generation before making it available; reject mixed generations and old schemas with a re-export instruction.

Migrate existing literal descriptors and their callers to this single representation, then remove duplicate script execution/export paths. Raw installed payloads remain local cache data, never tracked fixtures.

### Execution core

Proposed responsibilities inside `crates/cyrus-script/src/`, with filenames finalized during implementation:

- Program validation and lookup, using asset records rather than decoding installed bytes again.
- Integer/expression evaluation, comparisons, local/global access, and object references.
- Per-instance main/interrupt cursors, a shared actor return continuation, return state, and explicit running/waiting/yielded/failed states.
- Blocking tasks, multitasks, completion identity, cancellation, and synchronization according to researched rules.
- Structured failures containing world, actor, thread, instruction, operation, and relevant argument.

Use stable script actor identifiers, distinct from Bevy entities and display names. The host resolves them against the current world generation. Reject stale task completions after actor removal or world replacement. Research duplicate-name resolution and cross-object dispatch before selecting lookup behavior.

Expose a small host interface for immediate queries/mutations, starting tasks, polling/completing tasks, and cancellation. Keep runtime task payloads serializable rather than storing Rust closures. The exact method signatures follow the first supported animation and flag contracts; do not prebuild a generic scripting framework.

Run nonblocking instructions through the documented yield boundary. Add a diagnostic execution budget to detect runaway scripts; exhaustion produces an explicit failure, never an invented normal frame yield. Calibrate it against the shipped corpus before enabling ordinary gameplay. Integer overflow, division, shifts, formula order, boolean evaluation, invalid references, and stack failures require explicit semantics and edge-case tests.

### Scheduler and world lifetime

Add the game-owned scheduler after resource preparation and actor identity resolution. Instantiate scripts for logical actors even when they have no visible mesh. Preserve authored actor traversal where observable. Separate selected-player updates, ordinary actor eligibility, main/interrupt stages, existing-task updates, and presentation application.

Initially expose deterministic explicit invocation for tests. Enable automatic invocation only after host timing and eligibility comparisons pass. Do not substitute a fixed twelve-Hertz timer or render-frame count for the original behavior.

On replacement, quiesce old invocations and cancel old-world tasks/effects. Commit the prepared destination only when required resources validate. Prevent late old-world callbacks from mutating new actors. Handle same-world reloads and shared-map world variants by world identity and generation, not only a scene-name equality check.

### Native game commands

Build a registry resolved from installed definitions. Every declaration has a visible status: implemented with evidence, unsupported with reason, or shown by research to be irrelevant to shipped gameplay. An unimplemented command must not return a fake success, zero, or an immediately completed task.

Deliver these command families in dependency order:

1. Locals, global flags/defaults, references, attributes, object lookup, calls, control flow, and immediate queries.
2. Animation requests/waits, delays, lifecycle, visibility, activation, task cancellation, interrupts, hooks, and synchronization.
3. Sound and ambient tasks, speech/subtitles, logbook and HUD requests. Reuse existing sound ownership and preference paths.
4. Movement/rotation, attachments, groups, moving platforms, collision/contact queries, spatial tests, lights and effects. Preserve original coordinate units at the game boundary.
5. Player lockout, interaction, inventory mutations and item use, dialogue choices, scripted cameras, and restoration of control.
6. NPC navigation/perception, combat, projectiles, health/death, puzzle-specific commands, cinematic requests, and world transitions.

This is delivery order, not permission to omit a family used by an earlier reference sequence. Move a prerequisite forward when the installed script census demonstrates it is needed. Do not enable New Game merely because its first script starts.

### Persistence and diagnostics

Keep global game state, per-world persistent changes, per-actor VM state, task state, inventory, dialogue/quest state, and pending transitions distinct. Establish original reset/revisit rules before choosing which state survives each boundary. Save logical identities and continuations, never entity IDs, pointers, or audio handles.

Use a versioned snapshot with program identity, atomic native writes under `cyrus-config/`, and an appropriate transactional browser store for the same logical schema. Validate the full snapshot before replacing live state. Recreate presentation resources after restoration and reject unsupported versions/content without overwriting saves. Original save-file import is a separate format-research requirement; it is not assumed by implementing cyrus saves.

Extend control `State` with script readiness, active/waiting/failed counts, and bounded per-actor diagnostics. Provide deterministic stage stepping for tests through the existing control/MCP adapters when needed. Keep detailed execution logs local and disabled by default. Do not expose raw installed scripts as a public debug download.

## 7. Implementation sequence and exit gates

1. **Research and census.** Enumerate installed worlds, actors, entry points, opcodes, native calls, attributes, references, and hooks through `rgpre`. Keep generated content under `magic/`. Build a status matrix separating declared, statically observed, exercised, and verified support. Select reproducible original-game sequences for each family. Exit when every encountered operation has an owner, evidence link, and supported/unknown status. Static presence does not prove reachability or complete behavior.
2. **Preservation prerequisites.** Add strict, lossless definition metadata and typed instruction decoding, with synthetic tests and installed-corpus validation. Research task/interrupt/return/synchronization ambiguities and publish durable facts upstream before use. Under the user's GitHub policy, push contribution branches and provide compare links for the user to open PRs. Pin cyrus only to the reviewed available revision. Exit when required records round-trip without silently lost defaults or declarations.
3. **Portable cache.** Export/load `.scripts`, definition identity, actors, entries, and initial state on native and browser paths. Add contextual malformed-data errors and re-export handling. Exit when all installed worlds can be inventoried without silently discarded scripts, and unsupported entries remain visible.
4. **Pure VM.** Add `cyrus-script` and a synthetic host. Implement expressions, globals/locals, branches, calls/returns, object resolution, and explicit yield/failure. Exit when public-interface tests prove state mutations and exact invocation boundaries without a renderer.
5. **Tasks and animation migration.** Add blocking/multitask state, explicit stages, researched interrupt/synchronization behavior, and the existing animation host. Migrate literal-task callers/tests and delete the old interpreter. Exit when existing request/count/wait/End, sound-failure, and mesh-restoration contracts still pass through the new core.
6. **Live scheduler.** Finish the actor registry, initialization, eligibility, production time input, pause/resume, and world teardown. Exit only after matched original sequences prove actor order and same-update effects, including zero/multiple tick inputs. This phase depends on the existing player/animation timing work.
7. **First game slice.** Use the installed FLAG2 animation as a regression seed, then select an interaction from the census that requires a flag mutation and visible world response. Add its actual missing handlers. Verify native, headless state, and browser behavior. Exit when the interaction works without a special actor allowlist or scripted replacement content.
8. **Complete command families.** Deliver movement, audio/dialogue, inventory/HUD, camera/control, AI/combat, and transitions in small dependency-driven slices. Each slice needs original evidence and a replayable game scenario. Add failure and interruption paths alongside the success path.
9. **Persistence.** Implement snapshot/restore, world revisits, fresh-game initialization, and safe failed-load behavior. Exit when saving during supported waits/tasks and continuing after restart matches uninterrupted execution. Resolve permitted save points against the original before exposing menus.
10. **Content completion.** Exercise all worlds and progression paths, including alternate quest order, deaths, interrupted dialogue, repeated transitions, and endgame. Close remaining used commands and uncovered branches. Update roadmap coverage only for behavior verified in the game.

Phases 4 and later can build against synthetic records while upstream research continues, but no guessed semantics may ship. Each phase may contain several small commits. Do not estimate calendar completion from opcode or function counts.

## 8. Verification strategy

Add public-interface synthetic tests to the new crate for expression modes, arithmetic edges, chained conditions, subroutine continuation overwrite, bad branch targets, local/global initialization, and object-target calls. Include main/interrupt shared-local behavior, overwritten actor return continuations, task pause/completion, multitask cancellation, End restart, synchronization release, and runaway failure after their semantics are established.

Preserve and migrate these existing contracts:

- `crates/cyrus-world/tests/animation_script.rs`: request/start/wait separation, same-group waits, End restart, invalid continuations, and unsupported operations.
- `crates/cyrus-world/tests/animation_script_sounds.rs`: cue ordering and sticky callback failures.
- `crates/cyrus-render/tests/animation_script.rs`: actor-local mesh changes and restoration after world replacement.
- `crates/cyrus-render/tests/animation_script_sounds.rs`: sound start/stop in the pose update.

Add integration cases for invisible scripted actors, missing resources, duplicate/stale object references, cancellation while waiting, late completion after world replacement, same-world reload, global-state retention, failed destination preparation, corrupted caches, and failed save writes. A failed load must leave prior durable data intact.

Run the same portable program/state fixtures on native and WebAssembly. Run installed export/census tests with an install; report skips without one. Keep game bytes and captured outputs outside git. Synthetic fixtures must be authored independently.

For each runtime slice, compare a known original scene, starting state, inputs, and simulation invocations. Record PC/task/state outcomes as well as visible/audio effects. Deliberately change one ordering or semantic rule and prove the comparison fails. A mock-host unit test alone does not establish game fidelity.

Existing commands to use at the relevant implementation stage:

```sh
cargo test -p cyrus-assets --features native -- --nocapture
cargo test -p cyrus-world
cargo test -p cyrus-game
cargo test -p cyrus-render --test animation_script
cargo test -p cyrus-render --test animation_script_sounds
dev/scripts/check-core-boundaries.sh
just web-build
just gates
just run-headless --scene START
just smoke-mcp
```

After creating the crate, add `cargo test -p cyrus-script`. Use `just replay <fixture> <label>` for newly authored sequence fixtures and the project playtest skill for original-game comparisons. Run the narrowest tests during development, then one `just gates` after the final code edit in each delivery. Keep the worktree's target directory separate. Documentation-only changes need publish-check and whitespace checks, not Cargo.

## 9. Risks and compatibility

**Inferred:** scheduling and handler fidelity are the largest risks. A correct decoder can still produce the wrong game if a task resumes one stage late, an interrupt shares the wrong continuation, or a sound callback runs twice.

Cache records, animation callers, render integration tests, both hosts, control diagnostics, and eventual saves are affected. Migrate all internal callers in the same delivery as API replacement. Keep direct animation-group playback available independently of SOUP.

Budget failures must remain diagnostic failures, not a hidden performance-dependent gameplay rule. Avoid allocations for every instruction and decode once during preparation. Measure installed scenes only after proving that all expected actors actually execute.

Do not serialize platform resource handles. Associate asynchronous work with world generations. Do not use unordered entity iteration as a substitute for researched actor traversal. Complete core-boundary and browser checks before accepting the crate split.

## 10. Expected files and ownership

Existing locations to modify, with new paths explicitly marked proposed:

- `Cargo.toml` and `Cargo.lock`: register the crate and pin accepted preservation changes.
- `crates/cyrus-assets/src/script.rs`, `src/animation_export/script.rs`, `src/scene.rs`: replace literal-only export with portable full programs; move export into a script-owned module when animation no longer owns it.
- `crates/cyrus-script/` **proposed**: execution core, public tests, README, and manifest.
- `crates/cyrus-world/src/animation/script.rs`, `src/animation/playback.rs`, `src/animation.rs`: remove duplicate interpreter ownership and migrate scripted playback callers while keeping animation execution.
- `crates/cyrus-world/src/lifecycle.rs`: retain script resources and world-generation identity.
- `crates/cyrus-game/src/lib.rs` and a `src/script/` module **proposed**: register the scheduler and domain handlers. Other domain modules change only when implementing their command family.
- `crates/cyrus/src/scene.rs` and `crates/cyrus-web/src/main.rs`: prepare and load script resources through the shared lifecycle.
- `crates/cyrus-control/src/lib.rs`, native control and MCP adapters: bounded diagnostics and explicit test driving. Inspect their complete dispatch paths before editing protocol signatures.
- Existing animation tests above, new asset/VM/game tests, and new `dev/fixtures/` input sequences: compatibility and behavior verification.
- `dev/scripts/check-core-boundaries.sh`: include the new crate in the isolated consumer check.
- Crate READMEs, `docs/SETUP.md`, `docs/ROADMAP.md`, and generated `docs/MAP.md`: describe accepted support and commands as implementation lands.

Upstream preservation changes belong in its `src/import/soup_def.rs`, typed RGM script decoding, related tests, and relevant format/engine documentation. Keep investigation notes and executable traces out of both tracked trees.

## 11. Reusable implementation context

The context pack below pins the current source. Proposed paths and new APIs are design choices, not existing symbols. Re-anchor changed files before implementation; do not repeat the whole investigation.

```json
{
  "task_summary": "Implement faithful SOUP execution and all required game integration in staged deliveries.",
  "acceptance_criteria": ["One portable interpreter", "Installed definitions remain authoritative", "Original-game ordering and task behavior verified", "Native/browser behavior agrees", "Shipped progression and resumable state verified"],
  "evidence_provenance": {"schema_version": 2, "head_commit": "1c5f643134e9768eaff6fd30dd8a72ca20fcbba0", "generated_plan_path": "docs/plans/2026-10-11-gitnexus-plan-soup-runtime-game-integration.md", "global_dirty_digest": {"algorithm": "sha256", "canonicalization": "gitnexus-evidence-provenance-v2 NUL-framed UTF-8 records", "value": "0a9c85780067d9afcd0764f307b60891e3cee927ee11eaeb5ec7826d10fd82cd"}, "cited_path_manifest": [{"path": "Cargo.toml", "object_kind": {"head": "regular", "index": "regular", "worktree": "regular", "untracked": "absent"}, "state": "clean", "rename_from": null, "rename_to": null, "head_digest": "sha256:878ecf099bf838899b43b12556b61f05cf91bf36d311960eade2d130bd0131f1", "index_digest": "sha256:878ecf099bf838899b43b12556b61f05cf91bf36d311960eade2d130bd0131f1", "worktree_digest": "sha256:878ecf099bf838899b43b12556b61f05cf91bf36d311960eade2d130bd0131f1", "untracked_digest": "absent"}, {"path": "crates/cyrus-assets/src/animation_export/script.rs", "object_kind": {"head": "regular", "index": "regular", "worktree": "regular", "untracked": "absent"}, "state": "clean", "rename_from": null, "rename_to": null, "head_digest": "sha256:ca326969949a697d3d01a4d981c542ec55cd360d64ac908f401d79785b78923f", "index_digest": "sha256:ca326969949a697d3d01a4d981c542ec55cd360d64ac908f401d79785b78923f", "worktree_digest": "sha256:ca326969949a697d3d01a4d981c542ec55cd360d64ac908f401d79785b78923f", "untracked_digest": "absent"}, {"path": "crates/cyrus-assets/src/scene.rs", "object_kind": {"head": "regular", "index": "regular", "worktree": "regular", "untracked": "absent"}, "state": "clean", "rename_from": null, "rename_to": null, "head_digest": "sha256:ac5754f0df1bd82a1547767389b0b8a62998fc6f6764f671293c311c10dced8e", "index_digest": "sha256:ac5754f0df1bd82a1547767389b0b8a62998fc6f6764f671293c311c10dced8e", "worktree_digest": "sha256:ac5754f0df1bd82a1547767389b0b8a62998fc6f6764f671293c311c10dced8e", "untracked_digest": "absent"}, {"path": "crates/cyrus-assets/src/script.rs", "object_kind": {"head": "regular", "index": "regular", "worktree": "regular", "untracked": "absent"}, "state": "clean", "rename_from": null, "rename_to": null, "head_digest": "sha256:649db34d3ad801f85613a78fde1d19348efc1c42ae797634e1113439a40484c6", "index_digest": "sha256:649db34d3ad801f85613a78fde1d19348efc1c42ae797634e1113439a40484c6", "worktree_digest": "sha256:649db34d3ad801f85613a78fde1d19348efc1c42ae797634e1113439a40484c6", "untracked_digest": "absent"}, {"path": "crates/cyrus-control/src/lib.rs", "object_kind": {"head": "regular", "index": "regular", "worktree": "regular", "untracked": "absent"}, "state": "clean", "rename_from": null, "rename_to": null, "head_digest": "sha256:b0c6c09e17b621c9667daffb3fcb7ebff1c9c27ce07e56388e9e1c1c263d6079", "index_digest": "sha256:b0c6c09e17b621c9667daffb3fcb7ebff1c9c27ce07e56388e9e1c1c263d6079", "worktree_digest": "sha256:b0c6c09e17b621c9667daffb3fcb7ebff1c9c27ce07e56388e9e1c1c263d6079", "untracked_digest": "absent"}, {"path": "crates/cyrus-game/src/lib.rs", "object_kind": {"head": "regular", "index": "regular", "worktree": "regular", "untracked": "absent"}, "state": "clean", "rename_from": null, "rename_to": null, "head_digest": "sha256:0a9b4d0a8cc5c4039501a0875521d39b1392f843aeafd7c6289187664f5f1a86", "index_digest": "sha256:0a9b4d0a8cc5c4039501a0875521d39b1392f843aeafd7c6289187664f5f1a86", "worktree_digest": "sha256:0a9b4d0a8cc5c4039501a0875521d39b1392f843aeafd7c6289187664f5f1a86", "untracked_digest": "absent"}, {"path": "crates/cyrus-render/tests/animation_script.rs", "object_kind": {"head": "regular", "index": "regular", "worktree": "regular", "untracked": "absent"}, "state": "clean", "rename_from": null, "rename_to": null, "head_digest": "sha256:6582f193592b74190c6819fa9e07be7d907f98b612bc5595d92d5f27a139eb73", "index_digest": "sha256:6582f193592b74190c6819fa9e07be7d907f98b612bc5595d92d5f27a139eb73", "worktree_digest": "sha256:6582f193592b74190c6819fa9e07be7d907f98b612bc5595d92d5f27a139eb73", "untracked_digest": "absent"}, {"path": "crates/cyrus-render/tests/animation_script_sounds.rs", "object_kind": {"head": "regular", "index": "regular", "worktree": "regular", "untracked": "absent"}, "state": "clean", "rename_from": null, "rename_to": null, "head_digest": "sha256:0f527ae55d83ab5d98c503152c04d8686d5928de112a00648fd5363456ae8d58", "index_digest": "sha256:0f527ae55d83ab5d98c503152c04d8686d5928de112a00648fd5363456ae8d58", "worktree_digest": "sha256:0f527ae55d83ab5d98c503152c04d8686d5928de112a00648fd5363456ae8d58", "untracked_digest": "absent"}, {"path": "crates/cyrus-web/src/main.rs", "object_kind": {"head": "regular", "index": "regular", "worktree": "regular", "untracked": "absent"}, "state": "clean", "rename_from": null, "rename_to": null, "head_digest": "sha256:1a286ca9d7a02afd5b20fcb575e508dd9ac0c05d3d2c26eb2fdfa858c4477921", "index_digest": "sha256:1a286ca9d7a02afd5b20fcb575e508dd9ac0c05d3d2c26eb2fdfa858c4477921", "worktree_digest": "sha256:1a286ca9d7a02afd5b20fcb575e508dd9ac0c05d3d2c26eb2fdfa858c4477921", "untracked_digest": "absent"}, {"path": "crates/cyrus-world/src/animation.rs", "object_kind": {"head": "regular", "index": "regular", "worktree": "regular", "untracked": "absent"}, "state": "clean", "rename_from": null, "rename_to": null, "head_digest": "sha256:964f669a8f3756b562114d7cbacad558d54fd19fc11690c930ed9eeb61e88aef", "index_digest": "sha256:964f669a8f3756b562114d7cbacad558d54fd19fc11690c930ed9eeb61e88aef", "worktree_digest": "sha256:964f669a8f3756b562114d7cbacad558d54fd19fc11690c930ed9eeb61e88aef", "untracked_digest": "absent"}, {"path": "crates/cyrus-world/src/animation/playback.rs", "object_kind": {"head": "regular", "index": "regular", "worktree": "regular", "untracked": "absent"}, "state": "clean", "rename_from": null, "rename_to": null, "head_digest": "sha256:d18f06fc2ab92206941a744a2b60272f3dde14d49b30782207eb54a0e7fdbacd", "index_digest": "sha256:d18f06fc2ab92206941a744a2b60272f3dde14d49b30782207eb54a0e7fdbacd", "worktree_digest": "sha256:d18f06fc2ab92206941a744a2b60272f3dde14d49b30782207eb54a0e7fdbacd", "untracked_digest": "absent"}, {"path": "crates/cyrus-world/src/animation/script.rs", "object_kind": {"head": "regular", "index": "regular", "worktree": "regular", "untracked": "absent"}, "state": "clean", "rename_from": null, "rename_to": null, "head_digest": "sha256:a3310d879fa200deaaf1f923506911c9a1f7cd949f68d236233c07535094585b", "index_digest": "sha256:a3310d879fa200deaaf1f923506911c9a1f7cd949f68d236233c07535094585b", "worktree_digest": "sha256:a3310d879fa200deaaf1f923506911c9a1f7cd949f68d236233c07535094585b", "untracked_digest": "absent"}, {"path": "crates/cyrus-world/src/lifecycle.rs", "object_kind": {"head": "regular", "index": "regular", "worktree": "regular", "untracked": "absent"}, "state": "clean", "rename_from": null, "rename_to": null, "head_digest": "sha256:f76ecc157663cdcd1ec1fea37a62d1bffddd0153a144e24243a8bc8d93e89da9", "index_digest": "sha256:f76ecc157663cdcd1ec1fea37a62d1bffddd0153a144e24243a8bc8d93e89da9", "worktree_digest": "sha256:f76ecc157663cdcd1ec1fea37a62d1bffddd0153a144e24243a8bc8d93e89da9", "untracked_digest": "absent"}, {"path": "crates/cyrus-world/tests/animation_script.rs", "object_kind": {"head": "regular", "index": "regular", "worktree": "regular", "untracked": "absent"}, "state": "clean", "rename_from": null, "rename_to": null, "head_digest": "sha256:b1bb73a8495b56b227a83ac9647fdd72b93a1ae0e4a044828c6da01e91e74279", "index_digest": "sha256:b1bb73a8495b56b227a83ac9647fdd72b93a1ae0e4a044828c6da01e91e74279", "worktree_digest": "sha256:b1bb73a8495b56b227a83ac9647fdd72b93a1ae0e4a044828c6da01e91e74279", "untracked_digest": "absent"}, {"path": "crates/cyrus-world/tests/animation_script_sounds.rs", "object_kind": {"head": "regular", "index": "regular", "worktree": "regular", "untracked": "absent"}, "state": "clean", "rename_from": null, "rename_to": null, "head_digest": "sha256:892def68f85b5392b52fd2ed318b1ca01de405caf9ce36b017e0290737a47fd7", "index_digest": "sha256:892def68f85b5392b52fd2ed318b1ca01de405caf9ce36b017e0290737a47fd7", "worktree_digest": "sha256:892def68f85b5392b52fd2ed318b1ca01de405caf9ce36b017e0290737a47fd7", "untracked_digest": "absent"}, {"path": "crates/cyrus/src/scene.rs", "object_kind": {"head": "regular", "index": "regular", "worktree": "regular", "untracked": "absent"}, "state": "clean", "rename_from": null, "rename_to": null, "head_digest": "sha256:19bde27c00a791c70017ddb5d81946322504bce44f881ec7008f78b6cd0746a9", "index_digest": "sha256:19bde27c00a791c70017ddb5d81946322504bce44f881ec7008f78b6cd0746a9", "worktree_digest": "sha256:19bde27c00a791c70017ddb5d81946322504bce44f881ec7008f78b6cd0746a9", "untracked_digest": "absent"}, {"path": "dev/scripts/check-core-boundaries.sh", "object_kind": {"head": "regular", "index": "regular", "worktree": "regular", "untracked": "absent"}, "state": "clean", "rename_from": null, "rename_to": null, "head_digest": "sha256:f62048b4070fe579fd2159a0d7d13d4dd21af9ad01fb1add975879e6fb7f4608", "index_digest": "sha256:f62048b4070fe579fd2159a0d7d13d4dd21af9ad01fb1add975879e6fb7f4608", "worktree_digest": "sha256:f62048b4070fe579fd2159a0d7d13d4dd21af9ad01fb1add975879e6fb7f4608", "untracked_digest": "absent"}, {"path": "dev/scripts/gates.sh", "object_kind": {"head": "regular", "index": "regular", "worktree": "regular", "untracked": "absent"}, "state": "clean", "rename_from": null, "rename_to": null, "head_digest": "sha256:d3ec2903d462e70431e80561dc052bbfc276f1cd94ae757166c32d3a84eaf75e", "index_digest": "sha256:d3ec2903d462e70431e80561dc052bbfc276f1cd94ae757166c32d3a84eaf75e", "worktree_digest": "sha256:d3ec2903d462e70431e80561dc052bbfc276f1cd94ae757166c32d3a84eaf75e", "untracked_digest": "absent"}, {"path": "docs/ROADMAP.md", "object_kind": {"head": "regular", "index": "regular", "worktree": "regular", "untracked": "absent"}, "state": "clean", "rename_from": null, "rename_to": null, "head_digest": "sha256:f2374211837c7803f8f3082fa16a2164ecfa6b24066786d5cfb8d53eb3c64af3", "index_digest": "sha256:f2374211837c7803f8f3082fa16a2164ecfa6b24066786d5cfb8d53eb3c64af3", "worktree_digest": "sha256:f2374211837c7803f8f3082fa16a2164ecfa6b24066786d5cfb8d53eb3c64af3", "untracked_digest": "absent"}, {"path": "justfile", "object_kind": {"head": "regular", "index": "regular", "worktree": "regular", "untracked": "absent"}, "state": "clean", "rename_from": null, "rename_to": null, "head_digest": "sha256:561e9c92bb384b44be56868d0d15601c2cce3284f4b6cedb8e58d0481b38cfd6", "index_digest": "sha256:561e9c92bb384b44be56868d0d15601c2cce3284f4b6cedb8e58d0481b38cfd6", "worktree_digest": "sha256:561e9c92bb384b44be56868d0d15601c2cce3284f4b6cedb8e58d0481b38cfd6", "untracked_digest": "absent"}]},
  "primary_symbols": [{"symbol": "Program", "file": "crates/cyrus-assets/src/script.rs", "role": "Portable instructions to extend"}, {"symbol": "AnimationScript", "file": "crates/cyrus-world/src/animation/script.rs", "role": "Literal executor to migrate"}, {"symbol": "AnimationPlayback", "file": "crates/cyrus-world/src/animation/playback.rs", "role": "Existing animation integration"}, {"symbol": "PreparedWorld", "file": "crates/cyrus-world/src/lifecycle.rs", "role": "Shared resource lifetime"}, {"symbol": "GamePlugin", "file": "crates/cyrus-game/src/lib.rs", "role": "Game scheduler registration"}],
  "related_symbols": [{"symbol": "ActorAnimation", "relationship": "host animation service", "relevance": "Preserve request/start/wait and synchronous cues"}, {"symbol": "Command", "relationship": "control protocol", "relevance": "Expose bounded script status"}],
  "execution_path": ["rgpre parses installed data", "assets exports portable records", "hosts prepare world resources", "game scheduler admits actor stage", "VM executes through yield using domain handlers", "presentation consumes ordered effects"],
  "pdg_constraints": [],
  "architectural_patterns": [{"pattern": "Native exporter and portable cache consumer", "example_location": "crates/cyrus-assets/src/animation_export/script.rs", "usage_guidance": "Do not link native rgpre into runtime core"}],
  "files_to_modify": [{"file": "crates/cyrus-assets/src/script.rs", "symbols": [], "intended_change": "See sections 6 and 10"}, {"file": "crates/cyrus-assets/src/animation_export/script.rs", "symbols": [], "intended_change": "See sections 6 and 10"}, {"file": "crates/cyrus-assets/src/scene.rs", "symbols": [], "intended_change": "See sections 6 and 10"}, {"file": "crates/cyrus-world/src/animation/script.rs", "symbols": [], "intended_change": "See sections 6 and 10"}, {"file": "crates/cyrus-world/src/animation/playback.rs", "symbols": [], "intended_change": "See sections 6 and 10"}, {"file": "crates/cyrus-world/src/animation.rs", "symbols": [], "intended_change": "See sections 6 and 10"}, {"file": "crates/cyrus-world/src/lifecycle.rs", "symbols": [], "intended_change": "See sections 6 and 10"}, {"file": "crates/cyrus-game/src/lib.rs", "symbols": [], "intended_change": "See sections 6 and 10"}, {"file": "crates/cyrus/src/scene.rs", "symbols": [], "intended_change": "See sections 6 and 10"}, {"file": "crates/cyrus-web/src/main.rs", "symbols": [], "intended_change": "See sections 6 and 10"}, {"file": "crates/cyrus-control/src/lib.rs", "symbols": [], "intended_change": "See sections 6 and 10"}, {"file": "crates/cyrus-script/", "symbols": [], "intended_change": "Proposed new VM crate"}],
  "tests": [{"file": "crates/cyrus-script/tests/runtime.rs", "scenarios": ["Proposed: immediate statements run through yield", "Independent continuations share actor locals", "Task completion resumes exactly once", "Invalid targets and unsupported commands retain context"]}, {"file": "crates/cyrus-world/tests/animation_script.rs", "scenarios": ["Migrate existing request/count/wait/End contracts"]}, {"file": "crates/cyrus-render/tests/animation_script_sounds.rs", "scenarios": ["Preserve same-update cue and pose ordering"]}],
  "verification_commands": ["cargo test -p cyrus-world", "cargo test -p cyrus-game", "dev/scripts/check-core-boundaries.sh", "just web-build", "just gates"],
  "risks": ["Unresolved production clock", "Incomplete native handler evidence", "World-lifetime races", "Cache/save schema compatibility"],
  "assumptions": ["Validate separate script-cache readiness on both hosts", "Inspect all caller references before migration"],
  "open_questions": ["TaskPause and ScriptRV semantics", "Interrupt and synchronization lifecycle", "Default/reset/save behavior", "Complete shipped command census"],
  "avoid": ["No copied RG Unity code", "No operand-stack VM assumption", "No one-instruction-per-frame scheduler", "No fake-success handlers", "No hardcoded installed definition table", "No duplicate literal interpreter", "No tracked game data or research traces"],
  "index_refresh": "Not performed: no cyrus index; user-local agent state left untouched",
  "external_evidence": {"preservation": "e604d9c6924116706d37c5fb66fa1aa43269cb4d", "rgunity": "678905846bec4942bf1492c54fdd6f479b1482fe"}
}
```

## 12. Unknowns, assumptions, and deferred work

- **Unknown:** full task return/status conventions, TaskPause labels, ScriptRV branching, interrupt preemption/reentrancy, hooks, group/user synchronization, and cross-object task ownership. Research original executions before defining these transitions.
- **Unknown:** all native function semantics and their shipped reachability. The census is the first implementation deliverable, not something inferred from 367 declarations.
- **Unknown:** exact initialization/reset/revisit/save rules, unresolved numeric edge cases, duplicate actor-name lookup, and definition compatibility checks. Require specific upstream facts and fixtures.
- **Unknown:** production timing remains blocked by the existing clock/eligibility research. Explicit deterministic VM support may land earlier; automatic gameplay may not claim parity.
- **Assumption, to verify:** separate `.scripts` preparation can reuse the existing cache source on both hosts. Validate readiness and cache-generation behavior before changing loading.
- **Assumption, to verify:** current tests cover the literal animation contract sufficiently to migrate it. Read all caller references and test bodies before deletion; the plan inspected representative tests and source, not a complete runtime audit.
- Defer standalone development `.AI` loading, a SOUP source compiler/editor, mod APIs, JIT compilation, and a general-purpose scripting framework. None is needed to run installed shipped content.
- Original save interoperability requires separate preservation research. Keep it an explicit compatibility decision; cyrus-native save support must not imply it.

## 13. Definition of done

Track three distinct milestones:

1. **VM complete:** documented instruction and task semantics are implemented and tested, unsupported behavior is explicit, and native/browser execution agrees on shared fixtures.
2. **Engine integration complete:** eligible actors execute automatically in the correct order; animation, movement, sound, interrupts, tasks, and transitions match original reference sequences without allowlists or invented timing.
3. **Game support complete:** every operation needed by shipped progression works, dialogue/inventory/camera/combat/puzzles use the same authoritative state, world revisits and supported save points restore correctly, and full-game paths are verified.

All milestones require contextual errors, unchanged installed files, portable dependency checks, no game data in tracked files, and honest resource-coverage documentation. A parsed script or successful opcode dispatch is not evidence that the game behavior works.

## Implementation corrections

**Verified:** subsequent original-interpreter research corrected the initial
preservation and RG Unity assumptions. SOUP has a single shared actor return
continuation, not general nested call stacks. Gosub overwrites it; Return
restores a saved script-return byte. Active tasks reevaluate their encoded
operands and use six actor-owned slots. Opcode 27 stops matching tasks; opcodes
28 and 29 pause and resume them. Conditional operands can update their source
before comparison. Native arguments evaluate from last to first, while handlers
receive authored argument order. Global flag helpers maintain typed numeric values
and separate latches. Object-targeted tasks belong to the receiver and allow the
caller to continue. These contracts are published on the preservation
[contribution branch](https://github.com/michidk/redguard-preservation/compare/main...feat/soup-runtime-records).

These corrections supersede the corresponding proposed VM and test behavior
above. Implementation remains incremental; portable execution and inspection
do not satisfy the live scheduler, command-family, persistence or content exit
gates.

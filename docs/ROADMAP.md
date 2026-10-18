# Roadmap

Rebuild the original single-player Redguard. The original game is the
specification. No multiplayer or new content.

## At a glance

| Stage | Status | Goal |
| --- | --- | --- |
| Foundations | Done | Install loading, world exports, browser viewer and development tools |
| 0. Development tooling | Done | Build, run, drive and compare both games |
| 1. Complete world playback | In progress | Render, animate, sound and execute every required world script |
| 2. Player controller | Bounded walking integration | Original movement, camera and collision |
| 3. Interaction and persistence | In progress; complete restoration unverified | Dialogue, inventory, scripted cameras, transitions and saves |
| 4. Complete gameplay | Later | Quests, combat and remaining gameplay services |
| 5. Full-game fidelity | Later | A verified new-game-to-ending playthrough |
| 6. Release | Later; packaging exists | A tested distribution and clean-machine setup |
| Console and cheats | Lowest priority | Remaining cheat effects, then developer commands |

Checked items exist in committed code; they do not establish complete fidelity.
**Verified** means recorded evidence, **Inferred** means a conclusion from evidence,
and **Unknown** means unresolved behavior. Detailed verification belongs in
commits and implementation docs.

## Next work

Next rendering work after the Windows validation batch:

- [x] Disable game-camera MSAA. **Verified:** the camera regression and fresh native
  production-library run report `Off`; an isolated four-sample control changes the
  roof-edge pixels without changing the sampled sky. Full installed gates pass 993
  Rust executions with zero skips. [AA/sky evidence](https://github.com/michidk/redguard-decomp/tree/e8e9944464699d128640ee30c5869e1cbf3e0fa1/research/supplements/2026-10-16-next-five-validations).
- [x] Remove the injected sky grid by separating sky shading from software framebuffer
  reconstruction ([preservation scope](https://github.com/michidk/redguard-preservation/pull/93)).
  **Verified:** isolated native controls reduce repeating four-pixel channel variation
  from 2.3–2.7 to 0.02–0.07 RGB levels, versus 0.02–0.10 in the original capture.
  Sampled wall/actor interiors are unchanged; fresh production sky pixels match the
  accepted control. Full sky/sun color parity and browser runtime repetition remain
  **Unknown**. [Comparison evidence](https://github.com/michidk/redguard-decomp/tree/e8e9944464699d128640ee30c5869e1cbf3e0fa1/research/supplements/2026-10-16-next-five-validations).

1. Compare ordinary native/browser actors with the original at matched poses and frames.
   Authored animated normals and installed actor skins are implemented ([evidence](https://github.com/michidk/redguard-decomp/tree/0ba74a93f72d81dd77343eed395987155ab09ab4/research/supplements/2026-10-16-actor-fixes)).
   **Verified:** held Cyrus and START Brennan controls favor authored vertex normals; production
   now selects them. Native and browser START show Brennan and the VANDA pirates with their
   installed skins. **Unknown:** production actor pixel parity at matched pose and viewport; the
   improvement figures below were measured on the diagnostic control, not on production pixels.
2. Compare follow-camera placement and movement with the original, including the
   confirmed player/model intersection. Camera implementation stays with its owner.
3. Compare original candle timing under representative runtime conditions; bounded native/browser cadence
   is measured below, without tuning to the slower emulator.
4. Compare ordinary gameplay damage feedback and complete original/native snuff pixel
   parity; every original snuff state is now captured, matching the native fixture's order.
5. Correct the confirmed logbook scroll shading (lettering colour is done), then repeat
   matched captures. Complete remaining HUD layout and sampling comparisons.

Use background window input and game-only capture on the shared Windows desktop.
Record failed comparisons as defects; a completed investigation does not imply a fix.

- [x] Investigate the missing START boat in the isolated actor preview. **Verified:**
  ordinary native and browser START entry show the deck and ship, and the isolated fixture
  clears ordinary actor traversal; no production defect was found. One native capture taken
  right after a window resize showed only sky and fog and was not reproduced. [Evidence](https://github.com/michidk/redguard-decomp/tree/0ba74a93f72d81dd77343eed395987155ab09ab4/research/supplements/2026-10-16-actor-fixes).

**Verified:** matched Windows original,
native and browser START/ISLAND static regions favor the current world sampling
over forced point sampling. Remaining filtering and full pixel parity are **Unknown**.
Production activation prompt/item acceptance depends on
complete player-lineup execution and installed SOUP traversal acceptance.

- Complete START actor movement, collision and ambient maintenance so New Game
  sustains authored scripts and animation. Repeat acceptance across installed worlds.
- Resolve remaining terrain/model shading differences, actor prelighting and
  synchronized texture playback. Verify sky, sun and water across menu transitions.
- Complete automatic sound triggers and moving emitters. Surface remapping exists.
- Connect remaining gameplay state to the HUD and verify its timing and presentation.
- Compare original, native and browser behavior and audible output. Verify the
  selected-player idle/walk/release path before extending the controller.

The SOUP completion checklist (kept as local working memory) owns detailed
script dependencies. [MAP.md](MAP.md) owns module boundaries;
[SETUP.md](SETUP.md) owns launch, cache and comparison instructions.

### Active assignments

| Work | Status | Session |
| --- | --- | --- |
| SOUP integration: complete session candidate/commit, camera/terrain ownership, automatic combat, dialogue, lifecycle, remaining script services and surface-sound acceptance | Unclaimed, resume here; **Verified (installed START scripts, 2026-10-17):** START is the opening duel. The `START` actor sets player attribute 89 and removes itself; each pirate (`SPIRATE1/2`) waits 12 ticks, enters combat and, when dead, adds one to flag `DEADPIRATES` (362); when it reaches two, `SBRENNAN` appears and `SCENEST` runs the cutscene (sheathe, camera shots, dialogue, then `LoadWorld`). A playable opening therefore needs player fighting, the pirate combat decision stage, health and damage, and the camera natives. **Verified (START live run, 2026-10-17):** forward type-two animation groups (player groups 1-4 and 7, every pirate's group 1) now play, following [preservation PR 104](https://github.com/michidk/redguard-preservation/pull/104); `CYRUS` no longer fails every update. Still failing there: `SPIRATE1/2` with `ordinary combat decision stage unsupported` (the original's decision routine is about 200 instructions calling 20 helpers, five of them 200 to 250 instructions each; target validity, selection, distance and engagement are published in [preservation PR 109](https://github.com/michidk/redguard-preservation/pull/109), and the taunt requests the stage makes (the selector at the start of the stage chain is the taunt selector, not an attack-move chooser) are published in preservation SOUP.md, leaving the fight spacing and its nonplayer locomotion (the stage issues `MoveForward`, `MoveBackward`, `MoveLeft`, `MoveRight` and `WalkForward` requests, so those natives and their shared stepper come first; The blocked-ahead probe and the animation group classes are published in preservation (`MoveBackward` is implemented as a native task (preservation SOUP.md contract; tests cover the idle walk and a missing group). Combat stance needs `ScriptInputs::blocked_ahead_30`, which no update supplies yet, so it fails explicitly there; the motion step counter bits and the frame-wide flag the original sets before moving are not modelled. `DialogBegin`/`DialogEnd` and `MenuBegin`/`MenuEnd` write the dialogue-mode and topic-menu words (**Verified**: handler bodies and preservation sound.md; tested); `DialogBegin`'s extra clear of one retained byte with an unidentified owner is not modelled. `MyId` returns the actor's signed 16-bit runtime identifier (**Verified**: the handler reads the word the actor allocator fills with pool index plus three; tested against the actor's own identifier; an actor without one reports it unavailable). **Decision stage decode status (2026-10-18; details in the private research archive):** the stage root, target and engagement, the attack and defence selection routines, the attack and defence executors (small state machines), the move-table lookups, the COMBAT.INI record layouts, the combat flag byte and the combat maintenance are decoded; still undecoded are about ten predicate and starter helpers behind the defence selection, the 198-instruction helper after the selections and the root's message sends, and the strafing steppers depend on the ledge system. The executors alone would not change START, because the pirates begin idle. **Verified (START on the virtual display, 2026-10-18):** the real automatic update leaves exactly two actors failing, `SPIRATE1` and `SPIRATE2`, both with `ordinary combat decision stage unsupported`; `CYRUS`, `SBRENNAN`, `SCENEST`, `BOAT1`, the other pirates and the sharks run without failure, so the combat decision stage is the single blocker for the opening duel (the Docker daemon cannot mount a worktree under `~/.config`; copy the built binary under `/home/vibepod/workspaces` and mount that). **Observed (headless START, 2026-10-18):** after thousands of frames every actor is still untouched (`SPIRATE1` has no animation or combat record, living state 0, no failure), so the automatic ordinary update does not run in the headless native; live START checks need the windowed or virtual-display path, and the control `step_script` tool runs actors outside the real update (its "spatial state not prepared" errors are not game failures). The camera show natives (`ShowCyrus`, `ShowObj` and the `Loc`, `Pan` and `Look` variants) are thin wrappers over camera state and one pose stage; the stage itself already exists in `cyrus-game` (`CameraState::prepare_pose`: candidate position, segment obstruction, movement limiting, terrain height; `CameraRecord::candidate` matches the decoded desired-position math), so the missing piece is the native wiring: `CameraState` is used only by its own tests (no `ScriptWorld`, session or app owner holds one), so the first step is an owner created at world load from `CameraState::initial_records`, then a `GameHost` camera service, and the show natives need one that supplies the selected record, the object's attributes and the geometry the pose stage reads. `ShowCyrus` and `ShowCyrusLoc` do nothing while the selected player is armed. `IsWithinMap` is constant true and `Animal` constant zero; `LoadWorld`, `WonGame`, `PushPlayer` and `PopPlayer` only raise a request (`GameplayGlobals::take_host_requests`) exactly as the original stores them for its main loop (**Verified**: handler bodies; tested). No host consumes these requests yet: the world switch, game-won flow and player stack are unimplemented. `PlayerMoved` compares the selected player's position with its update-boundary snapshot (**Inferred** that the retained copy is cyrus's `previous_local_position`; only the missing-player and missing-spatial errors are tested). `PlayerTurned` waits on the angle representation. `SetAIType`, `SetAIMode`, `IsMaster`, `IsSlave` and `TrueFunction` follow their handler bodies (**Verified**; tested). `DisableTasks`/`EnableTasks` (0x04), `DisableObject`/`EnableObject` (0x08) and `DisableCombat`/`EnableCombat` (0x10) set and clear bits of the script-control byte beside the existing script (0x01) and hook (0x02) bits (**Verified**: handler bodies; tested). The task and object bits already gate the update stages; no reader of the combat bit has been found, so its effect is **Unknown**. `AdjustStrength` and `AdjustArmor` add to the combat record values (**Verified**: handler bodies; only the missing-record error is tested). `SetHealth` and `AdjustHealth` apply the clamped add of the original's two health routines (**Verified**: handler bodies and clamping; tested) for non-selected actors without the attribute-97 gate; attribute 120 ignores changes, and the death sequence, the attribute-97 combat-bit gate and selected-player HUD health are explicitly unavailable. `Health`, `PlayerHealth`, `Strength`, `Armor` and `ReplenishHealth` read or restore attribute 3 and the combat record (**Verified**: handler bodies; selected-player `ReplenishHealth` is explicitly unavailable until the HUD health timer has an owner; the strength/armor success path is untested). `IsDrawingOrSheathing` is the drawing or sheathing predicate (**Verified**: handler composition; tested with groups 91, 129 and 1). `IsCombatCapable` (holding attribute, else animation and combat record present) and `PlayerArmed` (selected player capable, weapon drawn, not sheathing) follow the original's handler bodies; the drawn-weapon byte and the transition-lock/world-active globals are **Inferred**, and only the fresh-actor `IsCombatCapable` case is tested. `IsInAir` (sliding, falling or queued-motion state nonzero, or living state 27-29; the transition-lock early exit is **Inferred** clear) is added likewise. `MyAIType` (lifecycle selector), `MyAIMode` (death notification byte) and `IsOnEdge` (living states 10-19) read the receiver's lifecycle bytes (**Verified**: handler bodies; fresh-state and argument tests only). `IsInWater` (surface classes 1-4) and `IsInLava` (class 5) read the receiver's stored surface like `IsInDeepWater` (**Verified**: handler bodies compared against the surface-class table; tested over all 256 values). `ScriptWorld::step_hazard` implements the forward-step hazard test (steep destination surface, with a terrain-height fallback and a missing-record case that are both **Inferred**; the slope case and the unrelated cases are tested). `TerrainGround::edge_blocked` implements the terrain half of the nonplayer deep-water and edge probe (**Verified**: handler bodies compared with the existing terrain material rules, which already reproduce the original's tile table; [preservation SOUP.md](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md) records it; tested over covered, uncovered-rock, flat and missing-grid cases); `MoveLeft` and `MoveRight` are the strafe twins of `MoveBackward` with their own step-vector and side-effect helpers still undecoded. `MoveForward` is decoded except two safety helpers (a forward-step safety check on terrain edges and candidate models, and a deep-water destination probe), which need terrain and water edge machinery that cyrus has only for the player; `MoveLeft`, `MoveRight` and `WalkForward` are not decoded yet. `ScriptWorld::blocked_ahead` implements it on the existing residual body query with a zero-radius sphere, and `cyrus_world::animation::group_class` carries the class table; only the early-exit paths have a fixture, the geometry path still needs a live combat actor; both flag meanings are now identified: queued-motion 24 excludes actor-owned candidate type `0x80`, as cyrus's `excluded_candidate_type` already does; implementing the probe needs the positive Y extent source and a zero-radius body probe over static and actor candidates). `ScriptWorld::combat_engaged` composes the published target validity, selected-player fallback and engagement rule (its pure parts are unit-tested; the composed call has no live fixture yet). `ScriptWorld::actor_in_sight` now implements the in-sight test on the prepared ordinary candidate list (distance gate, then a segment test where only the target's own model does not block; whether both faces block is **Inferred**, not yet compared with the original); the stepper's blocked-ahead probe and the in-sight test both read the candidate model list that ordinary traversal collects for each actor just before its update (published in preservation SOUP.md; it is a sphere around the actor, twice its model radius, so it can reuse cyrus's ordinary collision candidates)), the attack moves, pursuit and messages; the in-sight test reuses the candidate model list left by the most recent candidate collection, so it needs the frame order of those collections), and `SCENEST` is idle because its natives are unhandled. Reverse type-two playback is unsupported. `PlayAnimation` (the non-blocking animation request, groups 0 to 32767), `DisableHook`, `EnableHook`, `GetFramePassed`, `DropAllItems`, `InRectangle` and the script light controls (`Light`, `LightRadius`, `LightIntensity`, `LightOffset`, `LightFlicker`, `FlickerLight`, `LightFlickerOff`, retained state only: dynamic light position, colour and rendering remain unsupported) landed. Combat construction now copies attributes 2 and 4 into the strength and armor values (**Verified**: the original's constructor; previously both started at zero; [preservation SOUP.md](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md) records the other constructor values). `SetPotion`, `GetPotion`, `setStrength` and `setArmor` landed ([preservation PR 107](https://github.com/michidk/redguard-preservation/pull/107); potion indices beyond 3 are rejected). `FacePlayer` landed ([preservation PR 106](https://github.com/michidk/redguard-preservation/pull/106)). `SheathSword` landed for non-selected actors ([preservation PR 105](https://github.com/michidk/redguard-preservation/pull/105); the selected player's HUD timer and camera selection stay unsupported). `KillMe` landed (killed flag, death selector, hidden model, owned sounds; walk-node release waits for actor navigation state). **Verified** by an installed-content census: 94 of the 151 distinct native calls in shipped scripts still have no handler, most used first: `showPlayerPan`, `showCyrusPanLoc`, `setHealth`, `adjustHealth`, `SetPotion`, `GetPotion`, `FlatAnimate`, `showObjLoc`, `showCyrusLoc`, `LoadWorld`. Most need preservation contracts first; camera record initialization (`CameraState::initial_records`) and location-offset conversion landed; the `[camera]` settings are exported through `cyrus-assets` (an absent per-mode `*_max_vel` reads as zero, which keeps the copied rate, as the installed SYSTEM.INI requires) (script cache schema 16: re-export older worlds) and preservation PRs 100, 104, 105, 106, 107, 108 and 109 are merged and pinned; the camera initial state (both camera actors are `CAMERA`, selection zero, a player-type-one settings variant) is published in preservation, but the session state still waits on the full pose stage that setup runs twice, which needs the live geometry owners; next: camera-specific model admission, actor rebinding, live camera scheduling driven by SOUP requests | Released by Codex, Integrate SOUP Runtime Services. Ordered next steps are in this row (the plan file is no longer tracked); branch `t3/soup-implementation-roadmap`; research archived in `redguard-decomp` under `research/supplements/2026-10-17-soup-terrain-camera-handoff` |
| Player controller: movement, input, placement/runtime, collision and installed walking acceptance | In progress | Codex, controller completion; `magic/artifacts/2026-10-16-controller-completion/` |
| Logbook reader scroll lighting: recover the shading rule from matched original/native captures | In progress | Claude, cyrus-84, `startup-comparison` worktree; `magic/artifacts/2026-10-17-book-lighting/` |
| Actor/HUD follow-ups from the START comparison: compass heading and anchor in ISLAND, matched-pose Cyrus/Brennan actor pixel comparison, solid-colour faces under actor texture sets (scroll lighting stays with the row above) | In progress | Claude, feeble-panther-1c; `magic/artifacts/2026-10-16-actor-fixes/` (`GOAL.md`) |

Other rendering follow-ups, HUD integration and the save-slot menu are unassigned.
Player-controller implementation and walking acceptance belong to the controller owner.
SOUP consumes that owner through the existing ordinary-stage and snapshot contracts.
Shared load preparation and the native caller belong to the SOUP owner.

## Resource coverage

**Verified:** all 29 installed WORLD.INI entries export geometry and supported
animation references. Parsing and export coverage do not establish working gameplay
or fidelity across every world and palette.

| Resources | Implemented | Still required |
| --- | --- | --- |
| WORLD.INI, RGM, WLD | World selection, placements, terrain and authored lighting | Scripted light changes, actor prelighting and complete actor behavior |
| 3D, 3DC, ROB | Static models, retained frames, supplied-tick poses and book motion | Complete production animation, encodings, group types and model replacement |
| BSI, COL, Glide sky | Textures, palettes, sky, fog, waves and supported texture playback | Remaining animation and matched filtering, blending and lighting |
| GXA, splash DLL, FNT | Splash, title, menu, HUD/logbook artwork and shared text rendering | Remaining gameplay screens, software sky and splash fidelity |
| SFX, RTX, CD audio | Effects, speech, subtitles, channels, music, preferences and surface remaps | Complete automatic triggers, moving emitters, nonplayer ground updates and audible parity |
| SOUP | Portable VM, logical actors, supported ordinary updates and explicit hosts | Remaining actor stages, native services, lifecycle, synchronization and persistence |
| MENU.INI, KEYS.INI | Data-driven menu, keyboard navigation, settings, rebinding and Movies | Gameplay handlers, device capture, Continue/pause, save slots and movie unlocks |
| ITEM.INI, COMBAT.INI | Shared script/HUD ownership, recursive acquisition/removal, startup counts and compass use | Pickups, remaining item use, automatic weapon validation, resets, saves and combat |
| SMK | Streamed native/browser video, first-track audio and skip/subtitle rules | Original comparison, A/V sync and hardware seek/audio checks |
| BMP, PCX, LBM, ANM | Image inspection and authored-rate ANM playback | Game cursor and palette cycling |
| CHT, CPF | Cheat and console parsing, local persistence and supported commands | Remaining gameplay effects, commands and console fidelity |
| PVO | Parsing and viewer inspection | Runtime culling excluded at user request |
| Save data | Component replacements and complete-document storage checks | Live whole-session restoration, revisits and original-save compatibility |

## 0. Development tooling

- [x] Install discovery, preload reports, world exports and read-only install access.
- [x] Original-game runner, captures, TCP/MCP control, headless mode and replay fixtures.
- [x] Native/browser inspection controls, independent flycam and world picker.
- [x] Separate asset viewer with model/world inspection, images, fonts, animations and media controls.
- [x] Shared asset preparation, browser workers, caching and byte-range serving.
- [x] Local browser folder access without uploads and native install selection in the application window.
- [x] CI, native/browser packaging and preservation integration.
- [x] Installed world audit and repeatable original/native/browser captures for ISLAND, BELLTOWR and START. **Verified:** [capture workflow closeout](https://github.com/michidk/cyrus/commit/6248d6f). Static captures do not verify synchronized gameplay or audio.

Done when a fresh clone with a purchased install runs both games, accepts agent
control and produces a repeatable comparison through SETUP commands.

## 1. Complete world playback

Static loading and supported ordinary script updates work. Complete automatic
world playback remains unfinished.

### Rendering

- [x] World geometry, terrain, textures, palettes, horizontal camera projection and independent fidelity controls. Optional Modern sky, fog and water exist; Modern lighting reuses Faithful results.
- [x] Installed model lighting, static map-light baking, authored object lights and ROB filtering attributes.
- [x] Compare held Cyrus and START Brennan shading with isolated normal and skin controls. **Verified:** sampled sleeve RGB error falls from 37.91 to 3.60 for Cyrus and 5.32 to 2.42 for Brennan when only normals change; the wrong-skin control raises Brennan shirt error from 2.94 to 44.03. Presentation resampling and unmatched boat state limit acceptance. Brightness and lighting inputs stay fixed; no production correction ships. [Bounded comparison](https://github.com/michidk/redguard-decomp/tree/1e19412b230b142643bb042804a1b6355d36d2d0/research/supplements/2026-10-16-held-actor-shading).
- [x] Preserve authored vertex-normal selection during animated mesh updates, with face fallback for invalid normals. **Verified:** unit and installed-data tests (frame-zero normals equal preservation's export; no animated frame substitutes face normals for fully authored models); a face-normal mutation fails both. Native and browser START show the actors. Matched-pose pixel comparison remains open.
- [x] Retain each actor's installed texture set when preparing and replacing animation source models. **Verified:** every START actor's RAHD/RAEX skin values agree; export writes `<MODEL>-<set>.glb` per set and the runtime selects the bound actor's copy ([preservation actor texture sets](https://github.com/michidk/redguard-preservation/pull/94)); native and browser show Brennan's green bandana and the pirates' striped shirts. **Unknown:** solid-colour face handling (open in that pull request), pixel parity. Cargo pins the merged preservation commit; the solid-colour question stays open there.
- [x] Finite terrain window, clipping, fixed-point texture sampling, fog and water-edge normal retention.
- [x] Share the corrected rotation table between world transforms and water. **Verified:** the [published initializer rule](https://github.com/michidk/redguard-preservation/commit/7fb4970af38d225f2f0182f2af0d44885aea8569) retains angle precision through sine and includes the extended cosine endpoint; retained native execution matches every table sample and rejects premature angle rounding. Complete camera and visual parity remain open.
- [x] Resizable World Inspector with a scrollable entity viewport. **Verified:** the egui pointer-drag regression checks window and viewport growth; headless browser v0.1.12 START captures at 1000×800 confirm growth and shrinkage with the entity viewport filling the resized window.
- [x] Modern render distance expands terrain and water together. **Verified:** native v0.1.12 ISLAND captures at the same elevated view show extended terrain and river water at the default Modern distance; returning Fog to Faithful restores the original 3,800-unit view. Water remains Faithful during the check. **Verified:** headless browser START checks accept typed distances of 7,600 and 5,200, select Modern Fog, and restore 3,800 with Faithful Fog through Original.
- [x] Sky phase controls, plane-crossing behavior, cloud coordinates, integer sun-alpha filtering and encoded-RGB sun blending.
- [x] Shared texture playback for terrain, ordinary archive models and MPSF flats; inspection holds for matched frames.
- [x] Source-sized camera-facing flats with authored brightness, alpha blending, generated normals and submission order.
- [x] Matched rendering comparisons in ISLAND, BELLTOWR and START, including controlled texture/sky/wave state. **Verified:** [latest START menu-entry comparison](https://github.com/michidk/cyrus/commit/bdfa55c). The native/browser deck remains darker; complete pixel parity is **Unknown**.
- [x] Consume [converted runtime face brightness](https://github.com/michidk/redguard-preservation/commit/aa9632769884108395f45f0d6de51ac98dddea57) in version-4 model exports. **Verified:** installed START deck input, rejection of old lighting metadata, and matched original/native/browser menu-entry captures with measured camera, frozen sky/waves and held water texture. Against the old-version control, mean deck RGB error falls from 18.36 to 12.96 and rail error from 8.62 to 1.82 on both backends. Generated-flat brightness remains direct. Residual deck shading and full parity remain **Unknown**.
- [x] Retain all indexed authored model normals through [version-5 exports](https://github.com/michidk/redguard-preservation/commit/a2ea54e358679cbe35c555aad3fb9518068d41dc). **Verified:** matched START original/native/browser menu-entry comparisons reduce mean deck RGB error from 12.96 to 3.18 against the prior-version control; sky is unchanged. Residual shading and full parity remain **Unknown**.
- [x] Clip ordinary model RGB before projection using the [published camera-space rule](https://github.com/michidk/redguard-preservation/commit/dd8c5f8b10a0cdaeaaa33f0aedcb5a177a98ed2a). **Verified:** matched native/browser ISLAND captures against the saved original view and prior-version control reduce shoreline mean RGB error from 3.86 to 2.05; sampled sun/cloud regions are unchanged. Original water phase, generated-flat clipping, remaining sampling precision and transition parity remain **Unknown**.
- [x] Quantize ordinary clipped-model projection using the [published sixteenth-pixel rule](https://github.com/michidk/redguard-preservation/commit/c91863c44fafaf212e7b9928a3331c8242049cc1). **Verified:** original-instruction arithmetic checks and native/browser START New Game captures at the measured viewpoint. Combined clipping/projection changes reduce deck mean RGB error from 3.18 to 1.56 against the retained pre-clipping control; sampled sky is unchanged. ISLAND shoreline error remains 2.05. Original water timing, residual shading and full parity remain **Unknown**.
- [x] Apply ordinary-model fixed color planes and integer texture modulation from the [published driver rule](https://github.com/michidk/redguard-preservation/commit/8e504c11309a9daf5cb33ac743d602ce4baf875e). **Verified:** measured native/browser START and ISLAND captures against saved originals and prior-version controls; START deck mean RGB error falls from 1.56 to 1.06, ISLAND shoreline from 2.05 to 1.48. Sampled sky/sun pixels are unchanged. Original water cadence and full pixel parity remain **Unknown**.
- [ ] Apply actor prelighting from authoritative world-entry state and recover dynamic lighting overrides. **Verified:** the published eligibility rule requires the resolved static-model identity, `attr_non_moving` and pose at the world-entry prelighting checkpoint. The SOUP host must expose that checkpoint; current presentation state does not establish it. Renderer integration and dynamic overrides remain **Unknown**.
- [x] Apply [filtered palette-key rejection and point-sampled placed flats](https://github.com/michidk/redguard-preservation/commit/7d7c0f38a7fc30dcfd93c4f045df581673e42902). **Verified:** all five held BELLTOWR native/browser flame frames improve against the saved original and previous-renderer control; frame 2 mean RGB error falls from 2.09 to 1.18. Authored solid-color faces bypass texture-key metadata. Original animation phase and cadence remain **Unknown**.
- [x] Recover [actor initialization versus static draw-flag ownership](https://github.com/michidk/redguard-preservation/commit/2292a631df72c5dd80625340b39d93400a4638e3). **Verified:** initializer, first-group assignment and archive submission instruction checks distinguish the paths; missing export metadata alone does not establish runtime flags. Actor host integration and later overrides remain open.
- [ ] Complete standalone/generated model filtering defaults and remaining texture admission/animation paths.
- [x] Compare Windows static START models and ISLAND terrain against the original and a forced-point control. **Verified:** six matched 1024×768 regions have native/browser pixel agreement; mean RGB error against the original is 2.45–2.84 versus 3.72–5.01 for forced point sampling. Actors, water, sky and full parity remain open. [Comparison evidence](https://github.com/michidk/redguard-decomp/tree/39784738385f4e615d07d7c448094cbc46cd172a/research/supplements/2026-10-16-windows-verification/filtering-webgl).
- [x] Pack model and sky material parameters within Windows WebGL uniform-buffer limits. **Verified:** the failing model pipeline requested 14 fragment blocks against the device's 12-block limit; packed bindings render matched START/ISLAND browser views without changing native static pixels. Focused tests and 963 installed gate executions pass. [Runtime evidence](https://github.com/michidk/redguard-decomp/tree/39784738385f4e615d07d7c448094cbc46cd172a/research/supplements/2026-10-16-windows-verification/filtering-webgl).
- [x] Publish model texture allocation/reduction and original pixel-coordinate export contracts. **Verified:** [preservation layout API](https://github.com/michidk/redguard-preservation/commit/85afd92c81ce077ee3a5f73118bf3f16ea8dfa55) matches 867 original descriptor cases; [UV metadata export](https://github.com/michidk/redguard-preservation/commit/34ece4490762d9a2c2941ae3eb97f8e74a037713) passes the GLB roundtrip and 216 preservation tests. Runtime consumption and matched sampling are delivered in [Cyrus](https://github.com/michidk/cyrus/commit/fda96dc).
- [x] Sample ordinary clipped models from original pixel coordinates through reduced, padded texture allocations. **Verified:** [preservation wide-coordinate correction](https://github.com/michidk/redguard-preservation/commit/c1fa0cf3699df053b2ffeae2516a3d3492969fd3), fixed-plane GPU checks and measured native/browser START, ISLAND and five held BELLTOWR frames against retained originals. START deck error remains about 1.061; an isolated wrong-shift control raises it to 10.382. ISLAND shoreline error is 1.476 on both backends. Browser uniform packing fits WebGL limits. Original cadence and residual shading/water differences remain **Unknown**.
- [x] Verify [fixed model sampling](https://github.com/michidk/cyrus/commit/fda96dc) after START book-menu entry. **Verified:** browser New Game completes through ordinary intro skips and preserves the measured view; frozen sky/waves and held texture frame give identical deck, rail and sky pixels to direct browser entry. Matched native New Game and retained original menu-return images retain deck error about 1.061; the wrong-shift control is 10.382. Water-field synchronization and complete transition parity remain **Unknown**.
- [x] Compare isolated ISLAND dry terrain after [fixed sampling](https://github.com/michidk/cyrus/commit/fda96dc). **Verified:** saved `084466d` original/native/browser comparison uses a measured 640×480 view and frozen sky/waves; ground-region error is 1.489/1.215 RGB levels, versus 3.257/3.446 for an incorrect-filter control. Residual error and full parity remain **Unknown**.
- [x] Compare coastal sun/clouds after native menu-to-world scene control. **Verified:** [saved build](https://github.com/michidk/cyrus/commit/084466d) at the measured original ISLAND view averages 0.023/0.300 RGB error; direct browser entry averages 0.022/0.299. A zero-alpha sun control raises sun error to 25.035 with identical clouds. Browser coastal menu-transition parity remains **Unknown**; its debug picker reloads the page.
- [x] Apply six-bit world palette precision before terrain filtering using the [published COL upload rule](https://github.com/michidk/redguard-preservation/commit/395e6693514482fb4a8c8881d5998843efee3542). **Verified:** measured 640×480 ISLAND original/native/browser captures with frozen sky/waves and independently recovered per-image frames reduce shoreline mean RGB error from 1.281 to 0.096 and land error from 1.405 to 0.030. Restoring only the old sampler reproduces the previous error; native/browser differences stay below 0.003. Both 9,409-case GPU sampler checks and full installed gates pass.
- [x] Apply [world palette ownership and six-bit precision](https://github.com/michidk/redguard-preservation/commit/fe530cc92dce2c3375b6c2ef3befac5209518af3) before ordinary-model filtering. **Verified:** measured START original/native/browser captures after native New Game and browser menu Load, with frozen sky/waves and held water texture, reduce lower-deck mean RGB error from 1.179 to 0.038 against an isolated old-sampler control. Native/browser deck and rail differences stay below 0.002. Native BELLTOWR controls improve wall/table colors; ISLAND terrain regions are unchanged. The production sampler passes 18,818 GPU cases and full installed gates. Standalone-model palette handling and full parity remain **Unknown**.
- [x] Apply [converted world-palette precision](https://github.com/michidk/redguard-preservation/commit/fe530cc92dce2c3375b6c2ef3befac5209518af3) to point-sampled placed flats. **Verified:** all five held BELLTOWR native/browser flame frames improve against the retained original and isolated old-sampler control. Frame 2 mean RGB error falls from 0.837 to 0.733 native and 0.725 browser; native/browser flame differences stay below 0.013. The control leaves wall/table pixels unchanged. Production GPU checks pass 6,912 cases and installed gates pass without skips. Original flame phase, cadence and full parity remain **Unknown**.
- [x] Apply [indexed world solid-palette initialization](https://github.com/michidk/redguard-preservation/commit/395b1b37c0dc1f5ed47d086c721358d5d7809f0b) before retained and live lighting. **Verified:** [renderer delivery](https://github.com/michidk/cyrus/commit/f9dda580bc1d789ff9be1a49680117fa91d16b68), 73 renderer tests and installed gates with zero skips. Measured 640×480 BELLTOWR original/native/browser views reduce sampled window error from 0.810 to zero and four ring-region errors from 0.29–0.33 to at most 0.002 against an isolated old-conversion control. Native/browser ring regions match; textured controls and five held candle-frame regions remain unchanged. Renderer tests and installed gates pass without skips. Full-scene parity and actor preparation remain **Unknown**.
- [ ] Resolve remaining model palette/shading and terrain sampling differences. Compare sky/sun state and fog/clipping across transitions. **Unknown:** full pixel parity, real-time texture/wave cadence and browser coastal menu-transition parity. The controlled palette comparison uses native menu-to-world scene control and direct browser entry.
- [ ] Verify synchronized texture/sky/wave cadence and overlapping-flat rendering against the original.

PVO runtime culling is excluded at user request. **Unknown:** whether its exclusions
change visible output. Revisit only with a demonstrated fidelity or performance need.

### Animation and scripts

- [x] Portable SOUP VM, logical actors, installed defaults, retained animation frames and supported requests/tasks. Completed native effects can preserve an unavailable numeric result; consuming it remains explicitly unsupported.
- [x] Shared explicit hosts for attributes, flags, waits, routing, activation, spatial transforms, movement/rotation and player queries.
- [x] Shared clock and random arithmetic, ordinary actor ordering, native-hook dispatch and supported automatic traversal.
- [x] Initial actor collision shapes, ordered static/live/transient candidates and live static-model support geometry.
- [x] Shared combat attack metadata and actor attack/defense bindings. **Verified:** installed export and public-interface tests cover contiguous definitions, ordered combo motions, sparse attack slots, defense-prefix termination and contextual missing-definition errors. Omitted numeric fields remain absent. Combat decisions and actor-specific motion timing remain open.

**Verified:** native New Game advances START sharks without debug stepping.
**Verified:** the current draft plays New Game Introduction before world loading;
a rebuilt native virtual-display run displayed INTRO.SMK, followed its installed
skip points, and reached a ready starting world. A sustained 120-update native
probe also confirms shark movement and Brennan animation-frame advancement.
Browser acceptance remains open. The probe identifies missing shared sample-reset
inputs on BOAT1 and WATERSND. A later native START probe confirms both pirates
complete their authored wait, enter combat lifecycle six and select drawing
group 91 through automatic dispatch. Shared combat ground weapon admission now
advances both actors to drawing frame 98 with group 8 pending; the next combat
decision stage remains unsupported and blocks further opening-story acceptance.
Explicit-stage tests and export coverage do not close automatic acceptance.

- [x] Standalone player-role animation entry and initial mesh binding without a controller tick. **Verified:** headless scheduling checks cover ordered CYRUS/GREMLIN setup, marker-position cues, combat association, weapon validation, generation replacement and unload cleanup; omitting initialization fails the comparison. Installed entry-frame comparison, full installed gates and browser check pass. Automatic locomotion and complete player lifecycle remain open.
- [ ] Complete production eligibility and invocation timing, model/default ownership, initialization, group types, cancellation and model replacement.
- [ ] Complete receivers, properties, variable references, scriptless objects, allocation/recycling and model-coupled attributes.
  Full-width property references, installed-name local-heading reads and signed local-position reads/writes are implemented. Heading public/installed shrine non-standing contracts and WASM check pass; four local-position contracts pass; the combined gate passes 983 Rust test executions with zero installed-data skips. Remaining embedded animation/session program fixtures now use the current schema. Program schema 3 requires re-export of older script caches. Current-animation reads and shared torch-counter reads/writes are implemented; all nine property contracts pass; combined verification passes 986 Rust test executions with zero installed-data skips. Receiver/player animation frame waits and retained progress are implemented; four focused task contracts and eleven animation-persistence tests pass; final WASM and workspace verification passes 992 Rust test executions with zero installed-data skips. Animation snapshot version 2 rejects older captures that lack progress.
- [ ] Complete NPC collision/support/response, model-state refresh, transient geometry and populated walk-map navigation/perception.
- [ ] Complete hooks, synchronization, lights, visibility and remaining native services through the SOUP completion checklist.
  `PlayerLooking` now uses the published effective-position and signed angular-threshold rules. Three public contracts, WASM checking and final workspace gates pass; installed automatic traversal remains pending.
  `PlayerStand` now reads the selected player's retained supporting actor without a new collision query. Public contracts, the installed non-standing branch, WASM verification and final workspace gates pass; complete standing interaction remains pending.
  `PlayerArc` now routes the published receiver-relative angular query through the existing spatial owner. Three public contracts, the installed distant-player branch, WASM checking and final workspace gates pass with zero installed skips; complete gameplay acceptance remains pending.
  Authored type-six light retention, `LightOff`, delayed cleanup and renderer replacement are implemented in `fb06f4d`. Focused lifecycle tests, installed-map light projection, Clippy and the WASM session check pass; final workspace gates pass with zero installed skips. Installed rendered comparison remains pending. Dynamic creation, modulation and type-four light controls remain unsupported.
- [ ] Handle script-requested world loads and verify removal/replacement with active scripts, tasks, meshes and sounds.
- [ ] Sustain automatic scripts and rendered animation across every required world on native and browser hosts.

### Sound and HUD

- [x] Shared native/browser effects, music, animation cues and script sound ownership/cleanup with synchronous channel feedback.
- [x] Installed SURFACE.INI remapping and supported ground-derived actor surfaces. **Verified:** [grounded surface-cue comparison](https://github.com/michidk/cyrus/commit/4f66c96). Complete player traversal and audible parity remain open.
  **Verified:** native effect selection from captured surfaces, playback with rate offset 0 Hz and cleanup pass for known zero, sand, wood and rock; original live Stop and group-replacement Forget also pass. [Cleanup evidence](https://github.com/michidk/redguard-decomp/tree/b5752668b4fab7c8d8e9281ab4e83452ae2899ef/research/supplements/2026-10-14-surface-audio-acceptance). **Verified:** native/browser preparation retains original PCM width, and decoded gain uses the documented integer table or scalar rule, including live volume/pan updates. The isolated original wood recording supports the scalar correction. **Unknown:** direct native/reference waveform and duration parity.
  **Verified:** native queue startup, idle transitions and rate changes retain complete interpolated cues. The corrected known-zero clip matches independently scaled PCM before system resampling; complete original audible parity remains open.
  **Verified:** actual native mixer output for all four surfaces matches independently scaled PCM before system resampling. The reference emulator's interpolation/filter model reproduces all four original streams sample-for-sample from that output; wrong gain, filter and rate controls fail. [Native/reference output evidence](https://github.com/michidk/redguard-decomp/tree/7d1e0e8d52b2a96016189eba426a649254ba458f/research/supplements/2026-10-15-surface-audio-acceptance/actual-native-reference-output). Complete automatic player walking and direct native/browser audible acceptance remain open.
- [x] Installed HUD elements, activation prompts and persistent F8/Tab/F7 preferences. Placed-player heading/world and script-actor health/type feed the HUD.
- [x] Preserve HUD artwork and text pixel edges when enlarged. **Verified:** silent Windows original/native captures at 1024×768 cover all three candle display modes; the nearest-sampling control sharpens artwork against the previous linear sampler, and the native correction also preserves health-text pixels. [Initial comparison](https://github.com/michidk/redguard-decomp/tree/19d39a6c3869ef1b5e097b82e8785a63ef2e362c/research/supplements/2026-10-16-windows-verification). Browser acceptance covers all three modes, persistence across scene reload and exact native agreement across 784 opaque sabre pixels. [Browser comparison](https://github.com/michidk/redguard-decomp/tree/39784738385f4e615d07d7c448094cbc46cd172a/research/supplements/2026-10-16-windows-verification/filtering-webgl). Complete HUD pixel parity and representative candle cadence remain open.
- [x] Measure optimized native full-health candle cadence without compilation load. **Verified:** three settled visible recordings show all five frames at approximately 59.9–60.2 advances/s; the copied GOG original shows about 31.6/s on this runtime. This establishes the bounded native rate and sequence, not exact timing parity. Browser and health-fixture checks follow below; full original snuff comparison remains open. [Timing evidence](https://github.com/michidk/redguard-decomp/tree/69b5c01ce72775993657c34235bfc5c22dba60df/research/supplements/2026-10-16-windows-verification/candle-cadence).
- [x] Measure visible browser full-health candle cadence with background canvas capture. **Verified:** two six-second recordings of the retained rendering build observe all five frames at 59.94 and 60.05 advances/s, consistent with native; incorrect-scale classification is rejected. No sample gap spans a full loop. Original hardware timing parity and current-main browser gameplay remain open. [Five-check evidence](https://github.com/michidk/redguard-decomp/tree/3bd53242121e5d93cfd32a09a3e25baa008f1ed0/research/supplements/2026-10-16-five-validations).
- [x] Exercise native HUD health transitions through a rendering-state fixture. **Verified:** health 100→50→100→0→100 produces the published damage/healing frame order and every snuff state 66–80. Matching 1024×768 health-79 captures also agree on all 241 sampled gold number pixels. This does not establish original snuff pixel parity or ordinary combat damage feedback. [Fixture and comparison evidence](https://github.com/michidk/redguard-decomp/tree/3bd53242121e5d93cfd32a09a3e25baa008f1ed0/research/supplements/2026-10-16-five-validations).
- [x] Capture all fifteen original candle-snuff states against installed artwork. **Verified:** two console-driven health recordings show frames 66–80 in order; the confirmed run retains each transition image and distinguishes every frame from its runner-up. The sequence agrees with the native fixture. This is not ordinary combat damage or complete pixel parity. [Original snuff evidence](https://github.com/michidk/redguard-decomp/tree/e8e9944464699d128640ee30c5869e1cbf3e0fa1/research/supplements/2026-10-16-next-five-validations).
- [x] Native restart and browser reload acceptance for F8/Tab/F7 preferences, including restored candle visibility and subsequent toggle notices. **Verified:** native health-number placement matches the original at 640×480; both builds cycle starting items and display the installed gold count of 53. Complete pixel parity remains open.
- [ ] Complete positional/ambient scheduling, maintenance/reset ownership, StopAllSounds results, shell/code objects, KillMe/Resurrect cleanup, rope/torch behavior and moving-emitter gain/pan.
- [ ] Complete global lifecycle and menu/dialogue maintenance. Lockout support and an explicit ordered reset adapter exist; automatic player-controller inhibition, topic execution and death/load resets remain open. Portable RTX preparation, the 20-entry cache, explicit combat-taunt cache replacement and shared-sample dialogue playback are implemented. Authored actor taunt lists, explicit ordered selection and prepared shared-sample presentation are implemented; explicit weapon draw/cleanup and synchronous resultless combat entry share authoritative actor state; automatic nonplayer invocation now derives shared inventory and prepared-model admission and prepares taunt dependencies; complete eligibility and remaining weapon lifecycle remain open. Full New Game story acceptance remains open.
- [x] Shared `ShowBitmap`/`UnShowBitmap` slots and `AddLog` persistence/notification dispatch. **Verified:** original-instruction acceptance checks, shared dispatch/render tests and native/browser START entry. Additional logbook styles/editing, save integration and original notification cadence remain open.
- [x] Shared compass use through installed bindings with known actor gates. **Verified:** headless checks cover ownership, rebinding, held-use consumption, lockout and HUD/world recreation. Shared inventory and production actor inputs are connected below; save restore remains open.
- [x] Investigate bottom-right compass shifting against the original. **Verified:** the original draws COMPASS2 frames equal to `((-(heading + world_compass)) mod 2048) >> 6` at 11 headings in START, ISLAND and BELLTOWR (`world_compass` 512), at the same (546,396) anchor where native is pixel-identical at 640×480; a unit test pins the observations. The reported shifting is not reproduced; **Inferred:** it came from comparing a 640×400 DOSBox presentation with native 4:3. **Unknown:** native heading changes (the native player does not turn on current main) and native windows that are not 4:3 (the HUD stays in a centred 4:3 box over a full-width view). [Evidence](https://github.com/michidk/redguard-decomp/tree/c9fc7a82/research/supplements/2026-10-17-compass-headings)
- [ ] Connect remaining production activation/sword state and player walk-mode/auto-defend behavior.
- [x] Shared script/HUD inventory, capped `AddItem`, `HaveItem`, `SelectItem` and compass use from logical-player state. **Verified:** headless script and HUD checks cover acquisition, live counts/selection, surface gates and world/HUD replacement; installed export checks cover all item artwork. New-game/death resets, saves and original timing comparison remain open.
- [x] `ActiveItem` and receiver-owned `HandItem` services. **Verified:** headless VM tests cover selected hidden/unowned records, admission gates, full-ID lookup before byte assignment, routed receivers and unavailable host state; a truncation mutation is rejected. Initial held-item reconstruction and attachment geometry remain open.
- [x] Shared logbook layout and native/browser reading screen. **Verified:** headless contracts, installed artwork export and native/browser input, scrolling and suspension checks. Scroll shading/filtering, other styles, editing and resets/saves remain open.
- [x] Correct logbook reader lettering colour. **Verified:** at 640×480 the original's normal-font glyphs are half the font palette colour, the selected font is unchanged ([preservation](https://github.com/michidk/redguard-preservation/pull/111)); glyph-pixel mean difference falls from 52.7 to 2.1 (24 normal entries) and 2.2 (mixed), wrong-scale controls stay at 24–27. Other font-table indices and the 1024×768 softness (window scaling) are not checked. [Evidence](https://github.com/michidk/redguard-decomp/tree/1e3a9676/research/supplements/2026-10-17-reader-lettering)
- [ ] Correct logbook scroll lighting. **Verified:** at 640×480 the original top scroll is about 0.53× the native unlit brightness and the bottom about 0.98×; the original reader is identical in START and ISLAND (MAE 0.002), so it does not use the world's lighting. `SCROLL.3D` is a v5.0 cylinder with authored radial normals. A directional-plus-ambient fit with preservation's model-lighting formula matches the bottom scroll but not the top. **Unknown:** the reader's light inputs, so no correction ships. [Evidence](https://github.com/michidk/redguard-decomp/tree/fa500f29/research/supplements/2026-10-17-reader-lighting)
  **Verified:** a sampler-only control reduces first-line error from 16.58 to 10.38 while scroll error remains about 76.83; sampling and lighting need separate corrections. [Control evidence](https://github.com/michidk/redguard-decomp/tree/e8e9944464699d128640ee30c5869e1cbf3e0fa1/research/supplements/2026-10-16-next-five-validations).
  **Verified:** settled single-entry captures show the native flat paper matching the original (ratio 0.98-1.00) and the original's shading as a grey scalar (channels equal within 1%) that dips to about 0.53 on the top roll near 45-55 degrees and stays 0.96-0.99 on the bottom roll; native draws the scroll unlit. The rule is **Unknown**; no correction ships ([evidence](https://github.com/michidk/redguard-decomp/tree/b43a5591/research/supplements/2026-10-17-book-lighting-poses)).
  **Status (2026-10-17, startup-comparison session). Done:** reader reproduction in the original (seed `LOGBOOK.TXT` after the world finishes loading; the reader key toggles), matched 640×480 captures, settled-frame procedure, retracted a wrong gain claim. **Left, in order:** (1) a rotated-pose capture (Up key, 20 per press) to test whether the top-roll dip follows depth, a per-face table or the scroll's brightness bytes; poll until two native frames match before comparing. (2) Investigate why the native reader drew no lettering for a single short entry in the settled frame (not yet reproduced or explained). (3) Implement the rule in cyrus once it is recovered, after publishing it in preservation.
- [x] Compare original candle health restoration and flicker frame order and installed sabre-to-gold cycling. Correct stale-health animation ordering, use shared nominal ticks for HUD countdowns and suspend HUD updates in the console. **Verified:** original recordings identify healing frames 29,23,17,11,5,0 and the five-frame flicker loop; original/native item cycling displays gold count 53. The stale-order control fails the integrated health check.
- [x] Establish an original activation reference at ISLAND Siona. **Verified:** the installed prompt changes to gold/count 53 after Period and returns after release; Ctrl starts conversation. A captured-state classifier rejects a shifted sequence. Gold artwork placement matches the configured anchor in original and retained native captures; a shifted-placement control is worse. This does not establish production activation or full pixel parity.
- [x] Supply live ordinary activation inputs and shared HUD prompt presentation. **Verified:** host and HUD-system tests cover installed-label lookup, live actor/menu state, held-use ownership and nominal-tick holdoff without inspection inputs; a reversed-input control fails. Standalone initialization owns the lineup result; unsupported alignment invalidates it. Full installed interaction remains open.
- [ ] Complete representative real-time candle timing, original/native snuff pixel comparison, matched production activation prompt/item timing and remaining HUD layout against the original. **Unknown:** the slow software-emulated capture does not establish hardware cadence or complete prompt interaction parity. Ordinary activation now samples live actor/input/RTX state and the initialized shared lineup result. PlayerLineUp movement remains unsupported; failed alignment invalidates the result. **Verified:** [published alignment rules](https://github.com/michidk/redguard-preservation/compare/main...docs/player-lineup-execution) establish per-task mode/reference ownership and distinct distance thresholds, checked against original instructions. Directional movement, collision-query cleanup and installed prompt/conversation acceptance remain open.
- [ ] Compare effects, speech, subtitles and music by ear. Establish listener/camera rules, pan direction, text-only subtitle timing and attached-emitter eligibility.
- [x] Optional Modern music mutes the CD data bytes at the start of track 2; Faithful retains installed playback. See [accepted differences](DIFFERENCES.md).

### Startup and book menu

- [x] Installed Windows splash/title and data-driven book motion, settings, keyboard navigation, feedback and authored Quit.
- [x] Selectable DOS overlay splash (`--splash dos`); the Windows GOG splash stays the default. See [accepted differences](DIFFERENCES.md).
- [x] Streamed native/browser Movies with skip points, subtitles/interlace and retained menu selection.
- [x] Movie subtitles draw in REDSEL with a (2, 2) shadow at alpha 180 and the per-record colours unused, and native skips swap to a decoder already prepared for the skip point. **Verified (native, Linux, 2026-10-17):** INTRO frame 4586 matches a Windows GOG capture in wording, wrapping and placement with text colour within about 5 percent; skips to frames 1508 and 4502 show the target after 0.24 s and 0.28 s (10.6 s before); with subtitles off the flag-0 line draws nothing and flag-1 lines draw, wrapped at the first space after column 35. Font rule: [preservation PR 110](https://github.com/michidk/redguard-preservation/pull/110) (open). **Unknown:** the browser skip path still decodes every skipped frame at 8 ms per update and may stall; the CD staying paused through a skip by ear (the container has no audio device; `state.music_paused` reports it on a host with sound); browser skip timing (measured: nothing).
- [x] The retro container plays the original's movies: the disc mounts as `game.cue`, and `RETRO_PRISTINE=1 just retro` starts the original as a player sees it. **Verified (Linux, 2026-10-17):** the Introduction plays from boot, and INTRO.SMK frames 1702 to 1777 show the same two-line gold REDSEL subtitle as cyrus at that moment; the original's text averages about 4 to 8 percent brighter than cyrus's. **Unknown:** whether the blend is alpha, intensity or a palette index. See [SETUP](SETUP.md).
- [x] New Game and temporary Load enter the installed starting world behind artwork and dependency readiness. Original-style world-entry progress text is implemented.
- [x] Compare settled startup title, Options/Display layout and native loading-bar presentation with the original; verify native/browser startup skipping and subtitle persistence across restart/reload. Full pixel and timing parity remain **Unknown**.
- [ ] Complete splash, camera clipping, book-transition timing, loading-artwork and audible comparisons. **Verified:** every book turn, camera move and the Quit close match the original's visible update sequence (DOSBox-X captures). Startup Quit has no fade; the in-game Quit [fade to black](https://github.com/michidk/redguard-preservation/pull/89) belongs to the future in-game book. Menu world entry [fades](https://github.com/michidk/redguard-preservation/pull/90) the loading screen in and out and the first world view in, as in the original. Main→Options book silhouettes match the original in every state after correcting the sweep depth ([preservation](https://github.com/michidk/redguard-preservation/pull/97)); no near-plane clipping difference is visible. The original's page turn does play rustle audio (installed effects 66 and 67 rank first by correlation), but the DOSBox-X capture underruns, so timing, gain and waveform parity are **Unknown** and need a real-time capture. The browser build loads with `?splash=dos` and prepares the same DOS splash frames as native (byte-identical); on-screen playback and the browser fades are unchecked, since software rendering is too slow to capture them. Wall-clock cadence remains open.
  **Status (2026-10-17, startup-comparison session). Done:** book transition parity and sweep depth, Windows and DOS splash selection (`--splash`), CD data-noise mute (`--music modern`), world-entry fades, mouse rebinding, binding-name cutoff, web build `CARGO_TARGET_DIR` override; accepted differences are in `docs/DIFFERENCES.md`. **Left:** wall-clock timing and audio parity need a real-time capture; browser playback of the fades and the DOS splash needs hardware rendering; loading-artwork pixel comparison; live original mouse-input observation.
- [x] Browser Movies acceptance. **Verified:** maintainer playtest. Keyframe seeking remains unsupported.
- [ ] Complete New Game initialization, save slots/thumbnails, overwrite/delete, Continue/pause and progression movie unlocks as gameplay state becomes available.
- [x] Mouse-button rebinding: Controls capture accepts left/right release (128/129) and bound player actions read the release edge ([input state table](https://github.com/michidk/redguard-preservation/pull/98)). **Verified:** static original code paths; no live original observation (the emulator never delivered guest mouse clicks). The original book menu has no pointer label selection.
- [ ] Support joystick rebinding and mouse-bound inventory, log, map and sword actions (codes 132-139 need a device adapter; those actions read keys only).

Load starts a fresh world; it does not restore a save. Browser Quit leaves the
menu usable when tab closure is refused. Movie keyframe seeking remains unsupported.

Done when every world renders, animates, sounds and executes its required scripts
against repeatable original-game comparisons.

## 2. Player controller

Bounded sheathed forward walking is connected to the ordinary script stage.
**Verified:** a synthetic headless keyboard route covers idle, held forward,
release continuation, logical-to-presentation pose updates and an authored sound
cue. **Unknown:** complete installed traversal and four-surface acceptance.
Running, composite actions and the original follow camera remain incomplete.

- [x] Installed keyboard bindings, marker placement, static/dry-ground settling and independent flycam.
- [x] Explicit grounded walking/turning/support/wall response with shared player, script, animation and render poses.
- [x] Player runtime and placement snapshot APIs, decoded player-source identity, and restored-entry preservation are checkpointed for the controller owner. **Verified:** runtime/placement contracts pass workspace verification; complete save/load and installed traversal acceptance remain open.
- [x] Bounded ordinary keyboard walking adapter with retained walk-mode state, controller prelude, selected-player phase-zero combat maintenance and shared sound-maintenance timing. **Verified:** synthetic scheduler and ordinary-stage contracts.
- [ ] Complete installed idle/held-forward/release acceptance across zero, sand, wood and rock; preserved offline audio evidence does not establish this route.
  Installed weapon/fighting queries now allow the selected-player script to reach movement. **Verified:** a headless ISLAND probe with explicit view context completes idle, held forward and release without selected-player failure. An installed-data headless regression now loads the production sidecars and inventory, samples the installed forward binding and checks idle, movement, release settling and logical/presentation pose agreement. Other world actors still report unsupported services/queries; full traversal remains unaccepted. Fighting selection follows the [published ordered query contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#fighting-query).
- [ ] Establish complete eligibility/timing and compare fractional startup, steady movement, release, walls and ground updates.
- [ ] Complete Faithful movement, turning, follow camera, jumping, gravity and quick actions. Recover sword, walk-mode and auto-defend rules before connecting them.
  **Verified:** the default START camera copies the player transform and intersects the animated CYRUS model, producing a moving black outline above the ship rail. Model-color, depth, separate inspection-view and origin-discard controls isolate it to the player. Fix the missing follow camera; diagnostic hiding is not a shipped fix. [Attribution evidence](https://github.com/michidk/redguard-decomp/tree/36572041780f637ebec6a0d67398e24ef1da7d29/research/supplements/2026-10-16-start-camera-outline).
- [ ] Complete slopes, steps, animated/dynamic collision, sliding, airborne/rollback response, ledges and moving platforms.
- [ ] Complete swimming, climbing, falls, exits and restoration after scripted motion; expose standing/contact state to scripts.
- [ ] Add optional Modern camera-relative movement, mouse camera, Space jump and held Shift run.
- [ ] Add native/browser gamepad navigation and character control with bindings, analog input, deadzones and connection handling.

Done when recorded routes match original movement, collision, camera and
transitions in native and browser builds.

## 3. Interaction and persistence

Requires the player controller.

- [ ] Production interaction with one eligible target, including overlapping pickups and removed objects on revisits.
- [x] Recursive ITEM.INI prerequisites, ordered add/drop-add effects, removal dependencies/flags and shared startup acquisitions. **Verified:** nine behavioral checks include installed startup counts; reversed drop order and post-effect cap recomputation fail the corresponding tests. Full gates and isolated dependency consumers pass.
- [x] Empty-selection acquisition fallback and explicit selected-player weapon validation. **Verified:** headless host checks cover fallback order, first-unit admission, selected-player assignment and validity gates; installed HUD export retains weapon classification. Moving fallback after count increment fails the negative control. Full installed gates and browser check pass. Ordinary scene updates now validate the selected player after standalone animation initialization.
- [ ] Complete pickups, remaining item-use handlers, automatic weapon validation, inventory-screen candle rules, resets and save integration.
- [ ] Dialogue choices, disabled choices, synchronized speech/subtitles/animation and results returned to waiting scripts.
- [ ] Scripted cameras and player-control lockout with reliable restoration.
  **Verified:** `script::camera` now provides off-live pose preparation, ordered hit retention, movement/contact/terrain correction and logical actor writes. Five public contracts pass for correction order, rejection without live mutation, task/selection state, request precedence and model admission filtering. Full integration remains unfinished. [Camera and terrain contracts](https://github.com/michidk/redguard-preservation/pull/95) and [draft evidence](https://github.com/michidk/redguard-decomp/tree/786417821979b357282033449f87a9e4fe98d93d/research/supplements/2026-10-16-camera-model-filter) are published. Camera-specific candidate eligibility, retained terrain queries, installed initialization, actor rebinding and live publication remain unfinished. Ordinary collision admission excludes the selected player and is not the camera admission contract.
- [ ] Authored world transitions and old sound/script/transient cleanup.
- [ ] Pause/resume of simulation and audio. Saving from the book needs this boundary: snapshot capture currently requires an active gameplay world.
- [x] Prepare installed Save/Load pages and atomic browser slot storage. **Verified:**
  installed page/motion contracts and compiled browser capture, failed-save protection,
  fresh-page restore and revision-safe deletion. Book dialogs, thumbnails and
  gameplay save eligibility remain open.
- [ ] Original save-slot menus on the prepared pages (unassigned). Open: installed row labels and positions (labels cut at forty characters), selection after deletion, description editing and Yes/No dialogs, host request/result ownership, thumbnails, quick/autosaves and native integration; Load still enters a fresh world. Unregistered row-mapping and dialog-input drafts, with passing contracts and rejected controls, are in the private archive at [redguard-decomp `a3ca080`](https://github.com/michidk/redguard-decomp/tree/a3ca0805880608572b7589404f05cd0766a5211a/research/supplements) (`2026-10-16-slot-row-draft`, `2026-10-16-dialog-input-draft`); port them as production modules, they are not accepted behavior. An unfinished, unverified save-state restoration prototype (script registry `saved.rs`, script flags, order and spatial state, actor animation state, a `cyrus-script` snapshot API) is kept at tag [`prototype/savegame-menu`](https://github.com/michidk/cyrus/tree/prototype/savegame-menu) (`1e83be9`), branched from an old main; use it as reference only, do not merge it. Rules: preservation menu docs and the PRs linked below.
- [x] Publish [quick-save request and reserved-slot rules](https://github.com/michidk/redguard-preservation/commit/efb9deabefcb41665dfb3528bac38c16a8fb4a26) and document [gameplay book shortcuts](https://github.com/michidk/redguard-preservation/pull/79) and [menu-before-quick-save ordering](https://github.com/michidk/redguard-preservation/pull/84). **Verified:** original input/consumer instructions and rejected altered-branch controls. Complete save eligibility remains open.
- [x] Document [save-list ordering](https://github.com/michidk/redguard-preservation/pull/82). **Verified:** original x86 ranking on synthetic records, save-file timestamp provenance and rejected ordering mutations. Live menu comparison remains open.
- [x] Publish [Save/Continue context restrictions](https://github.com/michidk/redguard-preservation/commit/2f0e0eedb4d61259e9a9293e74950dcaa78a3c6d), [death restrictions](https://github.com/michidk/redguard-preservation/commit/7e70b7c66c0dd754f6c071112de7f6a704a95106), [Escape/death restrictions](https://github.com/michidk/redguard-preservation/commit/d37c93d) and [Quit autosave lifecycle](https://github.com/michidk/redguard-preservation/commit/c42034e). **Verified:** original context/death branches, installed entry mapping and twelve bounded x86 cases with three rejected inverted-branch controls. Other save eligibility and runtime handlers remain open.
- [x] Publish [original inventory save records](https://github.com/michidk/redguard-preservation/compare/main...feat/save-inventory) and direct restoration rules. **Verified:** original instructions, typed codec tests, two original save-slot round trips and a rejected wrong-endian control. Runtime restoration remains open.
- [x] Validated VM snapshot/restore with exact source compatibility and task-token rebinding. **Verified:** headless resume, foreign-task, pause/counter and malformed-state checks. Complete actor/world snapshots and storage remain open.
- [x] Complete animation playback snapshots with installed-source validation and effect-free restoration. **Verified:** headless resume, delay/loop/exit, sound-latch and malformed-state contracts. Game-owned frame/sound binding and complete actor/world restoration remain open.
- [x] Complete clock snapshots with saved settings and explicit host-epoch rebasing. **Verified:** headless continuation, warmup/smoothing, fractional carry, wrap/backward samples and malformed-state contracts. Original SAV import and whole-game restore remain open.
- [x] Atomic global-flag value/latch snapshots with exact installed-schema validation. **Verified:** native-operation continuation, signed values, malformed-state rejection and installed-definition round trips. Cross-world retention, original SVGF import and whole-game restore remain open.
- [x] Native-task replacement snapshots validated against VM calls and explicit token/service bindings. **Verified:** headless wait/motion continuation, allocator history, paused slots, malformed bindings and animation request/cue retention. Complete service-owner and actor/world restoration remain open.
- [x] Journal replacement snapshots with exact installed-text validation. **Verified:** headless text/tag/notification continuation, failed-write suppression and malformed-state rejection without storage I/O; an installed-data document also survives native publication and fresh-process reconstruction, with truncated data rejected. Complete session restoration remains open.
- [x] Inventory replacement snapshots retain counts, mutable limits, visibility and selection without acquisition replay. **Verified:** focused continuation and malformed-state contracts.
- [x] Dialogue-record replacement snapshots with actor/sample rebinding. **Verified:** public continuation/rejection contracts and controlled browser save/reload of an active record/cache/slot with exact voice PCM. A spatially supported native seed also preserves five real dialogue updates interleaved with exact voice PCM across browser reload, including a pending half-frame and retained sample state. Authored conversations/choices, fresh-world speaker initialization and audible synchronization remain open.
- [x] Texture and terrain playback replacements retain image deadlines, holds, historical normals and texture admission with a rebased presentation clock. **Verified:** headless production updates compare retained state and visible mesh attributes across fresh entity bindings, camera/window changes and removal; malformed sources, bindings and geometry are rejected. The terrain owner also retains pre-scaling engine heights and exposes a settled camera reader; snapshots validate source, phase and the original window independently of Modern rendering. Older terrain snapshots require recapture. Whole-session publication and native/browser save acceptance remain open.
- [x] Complete configured script-HUD replacement snapshots compose inventory and journal with compass, held-use latch, power-up slots and replacement order. **Verified:** public continuation/malformed-state contracts and installed-data native publication with fresh-process reconstruction, including failed-write protection and altered-state controls. Replacement performs no storage I/O or startup acquisitions. Unconfigured and failed local state cannot be captured. Whole-session restoration remains open.
- [x] Exact source-decoder replacement checkpoints for effects, speech and CD music retain cursor, gains, looping and pause state. **Verified:** public continuation/rejection contracts, installed effects/CD through the headless host, unchanged CD PCM and browser compilation. Production browser save and fresh-page restore also retain an active installed looping effect and CD track at nonzero cursors; 257 subsequent PCM samples match per source, with shifted-cursor controls rejected. Complete dialogue assembly and downstream output buffers remain open.
- [x] Document [original restoration audio lifecycle](https://github.com/michidk/redguard-preservation/pull/83). **Verified:** original instructions and altered controls cover cleanup, reset-flag lifetime, saved native-task continuation and CD track-selection paths. Live music continuity and installed runtime revisit acceptance remain open.
- [ ] Complete session candidate and coordinated live commit. **Verified:** focused logical-world/native-task continuation and first-plugin-update checks preserve current/visited identities, clock and player entry state. Focused sample-pool tests retain source-decoder progress, pause and silent animation ownership while rejecting incomplete replacements. Focused presentation replacement checks retain HUD continuation, camera/framing, sky scrolling, the presentation epoch and per-world environment edits. Complete rendered publication, full audio/backend policy, installed host assembly, revisits and restart acceptance remain open.
- [ ] Reduce snapshot capture/load CPU cost. **Verified:** five optimized two-world browser fixture pairs preserve logical state; median capture is 11.7 seconds and load 34.3 seconds under shared-machine load. Snapshot construction dominates the separate capture probe.
- [ ] Safe native/browser persistence for world, player, NPC, object, quest, inventory, script and HUD state. **Verified:** native restart and CPU browser save/reload and world-picker round trips preserve logical owners, dormant worlds and model associations. Current-schema production-worker checks retain both worlds, all 173 ISLAND actor models and the player model. Invalid sources/documents, missing saves, denied reads and failed writes preserve accepted state; retries and request/result ownership pass. Browser writes request strict durability. Rendered/audio continuation, complete gameplay revisits and power-loss durability remain open. Original-save formats belong in preservation.

Done when a reference route uses an item, chooses dialogue, transitions worlds,
saves, restarts and resumes with matching state and control.

## 4. Complete gameplay

Requires interaction and persistence.

- [ ] Complete dialogue conditions, quests, item-use rules, NPC navigation/perception, combat, damage, death and restart.
- [ ] Complete fonts, settings and remaining gameplay feedback.
- [ ] Verify interrupted tasks, object/quest persistence and integrated gameplay across save/resume.

Original-save compatibility requires format evidence before a commitment.

## 5. Full-game fidelity

Requires complete gameplay.

- [ ] Cover all worlds, quests, actors, items, encounters and endings.
- [ ] Test revisits, alternate quest order, failure paths and repeated transitions with active runtime state.
- [ ] Complete a new-game-to-ending playthrough with original-game checkpoints.
- [ ] Resolve every unsupported behavior required to finish the game.

## 6. Release

Requires full-game fidelity. Packaging and publication already exist.

- [x] Tagged native/browser packages, checksums and release-triggered GitHub Pages deployment. **Verified:** [v0.1.7 release pipeline](https://github.com/michidk/cyrus/actions/runs/38526911906) passed, including Pages deployment.
- [x] Native package-content audit and Linux menu/New Game smoke; Windows/macOS packaging and runner help checks.
- [x] Fix the release shell-block extraction in the artifact test. **Verified:** [fix](https://github.com/michidk/cyrus/commit/70069b2); all five artifact tests pass.
- [ ] Test supported platforms/hardware and clean setup with a purchased GOG install.
- [ ] Measure loading, frame pacing and bounded memory across repeated transitions; fix observed bottlenecks.
- [ ] Publish final controls and limitations; verify install, play, save and resume on a clean machine.

## Console and cheats

Lowest priority: gameplay comes first. Do not start new console work ahead of
stages 1–5. Pull a command forward when it helps debug or verify gameplay, and
note that reason in its claim. Sessions already working on a related part may
finish it. Cheat effects come before the remaining developer commands; most of
them land naturally as their gameplay owners are implemented.

- [x] Shared console editor, keyboard/TCP dispatch, installed paper/font and original command catalog with explicit unsupported diagnostics.
- [x] Native/browser CHT restore/save, import/export, cheat-name entry and assignments. `neversaydie` reaches automatic and explicit script queries.
- [x] CPF preferences, log export, live SOUP flag commands and supported sky/wave controls.
- [x] Input isolation, restart persistence, malformed-state protection and install immutability checks.

### Cheats

- [ ] Connect remaining documented cheat effects as their gameplay owners land. **Unknown:** `oddjob` behavior.
  - `magiccarpet`, `yeahbaby`, `moonraker`: player controller owner.
  - `drevil`, `goldfinger`: health, damage and death handling.
  - `task`, `animation`, `drno`: SOUP actor task, animation and NPC update ownership.
  - `nodemarker`: navigation node-map display.

### Developer commands

Static recovery (2026-10-15; raw notes in the private research archive, not yet
published to preservation):

- **Verified:** `load world[,marker][,angle]` prints a usage error without a world
  and nothing on success. Marker and angle default to zero and use the console
  number reader; a later `load` replaces the request. The request applies after the
  console closes with F12; Escape discards it. The angle is the entry heading,
  wrapped modulo 2048, as in marker-based world entry.
- **Verified:** `render` prints `XnGine Render turned on`/`off` and toggles static-object
  rendering with `render static`; missing or unknown arguments print usage. Turning it
  off skips the world and sky draws. **Unknown:** what the screen shows meanwhile and
  where the static flag is consumed.
- **Verified:** `getmapmarker number` reports a closed session, an undefined marker or
  success. **Unknown:** how the player is re-placed.
- **Unknown:** the `REDGUARD.MRK` user-marker layout, `fxtexturelist` order and
  `fxshowtexture` presentation.

- [ ] Commands without an owner dependency, after original-game observation of the unknowns above: `render`, `getmapmarker`, `setmarker`/`getmarker`, `fxtexturelist`, `fxshowtexture` and `delay`.
- [ ] Owner-dependent commands when their owners land: `load` (authored in-world transitions), `cameradist`, `playerdist`, `item` and `object` (SOUP inventory/actors and console object selection), `fxsun*`, `fxfog*` and `fxambient*` (rendering comparison), remaining `show` topics and node/PUT editors. PVO commands are excluded at user request.

Follow [METHOD.md](METHOD.md) for evidence and fidelity, [WORKFLOW.md](WORKFLOW.md)
for ownership/handoffs and AGENTS.md for verification. Formats and recovered
engine facts belong in redguard-preservation.

### Unfinished SOUP/session checkpoint

Session capture avoids duplicate content serialization when source references
identify the same immutable asset. Shared and separately decoded sources produce
identical documents; changed source content still rejects. Focused contracts,
WASM checking and full workspace gates pass. Optimized browser capture/load timing
remains open; this does not complete save/load acceptance.

Load planning reuses source fingerprints after validating each represented world
and retains immutable source borrows through reconstruction. Direct restoration,
extra supplied sources and live publication retain their source checks. Current
and visited world rejection contracts cover mismatched identities, unsupported
versions and invalid retained state. Browser load timing after this change remains
unmeasured.

Shared aggregate, registry/service replacements, rendered staging, capture hooks
and asynchronous load preparation are committed as work in progress. The latest
asynchronous load integration test passes after isolating its asset fixture from
late plugin registration. Delayed-asset waiting and rejected-load preservation
have synthetic coverage. Native diagnostic load reached ready ISLAND gameplay
from START and recaptured both worlds. Retained START matches the input after
actor-generation rebinding. This does not establish revisit execution or complete
render/audio continuation. Native malformed-document rejection and retained-result
polling pass. Full workspace gates pass with the browser caller and zero
installed-data skips. The current draft adds ordinary actor model replacement and
off-live model/mesh staging to address the geometry gap exposed by the native load
probe. Focused replacement and session tests pass, and the checkpoint passed full gates
with 950 Rust test executions and zero installed-data skips. Installed rendered
round-trip acceptance remains pending. A paused installed capture now represents
current ISLAND, retained START and all 173 required ordinary model replacements
in 17,600,858 encoded bytes. Native load and recapture preserve all 173 ordinary
model replacements with matching displayed poses and unique roots. Retained START
matches after fresh actor-generation binding. Missing-model and mismatched-frame
candidates reject while the logical session, entity identities, model replacements
and other captured non-presentation owners remain unchanged. Presentation state
continues advancing. Pixel comparison remains pending.
The unfinished revisit integration initializes
entry roles, validates retained actors and clears shared sound services before
restoring saved execution state. Focused registry/task and audio cleanup tests,
WASM Clippy and the native build pass. The final workspace gate passed 955 Rust
test executions with zero failures, ignored tests or installed-data skips.
Installed ISLAND-to-START revisit preserves all 34 retained actors and task
identities under fresh generation binding; seven invocation counters advance by
two, and departed ISLAND remains exactly unchanged. Player, alternate player and
camera records are unchanged. Eighteen pre-existing controller/combat update
errors persist, so this proves retained-state continuation, not successful
gameplay traversal. Returning to ISLAND preserves all 975 actor records exactly
under fresh generation binding and leaves dormant START unchanged; its 70
pre-existing update failures persist. Complete gameplay revisit acceptance remains open.
The production browser caller passes CPU-only restore/save/reload with compatible
exports. Native/browser preparation now produces identical files for ISLAND and
START through portable placement math. A fresh native capture from those exports
retains both worlds, 173 ordinary rendered replacements and the player; its copied
cache matches the native runtime byte-for-byte. Fresh-process native restore preserves
both worlds after actor-generation rebinding, all ordinary model replacements and
the player model. Current and dormant source mismatches reject without changing
accepted registry, player, HUD or model state. CPU-only browser restore/save/fresh-page
reload of that untouched capture passes using independent worker exports, with both
worlds, all 173 model associations and logical HUD preserved. Malformed/missing
loads and an aborted write preserve accepted state. In-place browser world-picker
round trips retain the page, actor/program/native-task identities, dormant world
state and logical HUD. START resumes authored object movement; returning ISLAND
retains all 975 actor records exactly. A subsequent browser save and fresh-page reload
retain both worlds, all 173 ordinary models, the player model and logical session
state after generation/entity rebinding. Unsupported player behavior, rendered/audio
continuation and complete gameplay revisits remain open.
A refreshed native animation-version-2 capture retains current ISLAND and visited
START, 174 playback records and 173 ordinary models. Its 461 exports match the
runtime and the document is 17,248,094 bytes. Restoration of this refreshed fixture
remains pending; preceding checks used the earlier animation schema.
Source-decoder audio checkpoints
do not include downstream converter/device buffers. See the
SOUP completion plan (local working memory).

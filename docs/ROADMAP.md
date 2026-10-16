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

Next independent HUD task: compare remaining HUD layout and representative
real-time candle cadence. Production activation prompt/item acceptance depends on
complete player-lineup execution and installed SOUP traversal acceptance.

- Complete START actor movement, collision and ambient maintenance so New Game
  sustains authored scripts and animation. Repeat acceptance across installed worlds.
- Resolve remaining terrain/model shading differences, actor prelighting and
  synchronized texture playback. Verify sky, sun and water across menu transitions.
- Complete automatic sound triggers and moving emitters. Surface remapping exists.
- Connect remaining gameplay state to the HUD and verify its timing and presentation.
- Compare original, native and browser behavior and audible output. Verify the
  selected-player idle/walk/release path before extending the controller.

The [SOUP completion checklist](plans/2026-10-12-soup-completion.md) owns detailed
script dependencies. [MAP.md](MAP.md) owns module boundaries;
[SETUP.md](SETUP.md) owns launch, cache and comparison instructions.

### Active assignments

| Work | Status | Session |
| --- | --- | --- |
| Session storage: native/browser publication, browser caller, reload and failure acceptance; shared assembly excluded | In progress | Codex, Build Logbook Reading Screen, `t3/simplify-roadmap`; `magic/artifacts/2026-10-15-save-runtime/` |
| SOUP integration: complete session candidate/commit, automatic combat, dialogue, lifecycle, remaining script services and surface-sound acceptance | In progress | Codex, Integrate SOUP Runtime Services, `t3/soup-implementation-roadmap`; `magic/artifacts/2026-10-15-session-candidate/` |
| Player controller: movement, input, placement/runtime, collision and installed walking acceptance | Transferred by maintainer; incoming agent | Separate controller owner; player runtime/placement checkpoint committed for handoff; final checkpoint gates pending |
| Startup comparisons: splash, camera clipping, book-transition timing, loading artwork and audio against the original | In progress | Claude, cyrus-84, `startup-comparison` worktree; `magic/artifacts/2026-10-15-startup-comparison/` |

Remaining rendering follow-ups and HUD integration are unassigned.
Player-controller implementation and walking acceptance belong to the incoming controller owner.
SOUP consumes that owner through the existing ordinary-stage and snapshot contracts.
Browser session-storage and its caller belong to the storage owner; shared load
preparation and the native caller belong to the SOUP owner.

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
- [x] Finite terrain window, clipping, fixed-point texture sampling, fog and water-edge normal retention.
- [x] Resizable World Inspector with a scrollable entity viewport. **Verified:** the egui pointer-drag regression checks window and viewport growth; headless browser v0.1.12 START captures at 1000×800 confirm growth and shrinkage with the entity viewport filling the resized window.
- [x] Modern render distance expands terrain and water together. **Verified:** native v0.1.12 ISLAND captures at the same elevated view show extended terrain and river water at the default Modern distance; returning Fog to Faithful restores the original 3,800-unit view. Water remains Faithful during the check. **Verified:** headless browser START checks accept typed distances of 7,600 and 5,200, select Modern Fog, and restore 3,800 with Faithful Fog through Original.
- [x] Sky phase controls, plane-crossing behavior, cloud coordinates and integer sun filtering/blending.
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
- [ ] Complete NPC collision/support/response, model-state refresh, transient geometry and populated walk-map navigation/perception.
- [ ] Complete hooks, synchronization, lights, visibility and remaining native services through the [SOUP checklist](plans/2026-10-12-soup-completion.md).
- [ ] Handle script-requested world loads and verify removal/replacement with active scripts, tasks, meshes and sounds.
- [ ] Sustain automatic scripts and rendered animation across every required world on native and browser hosts.

### Sound and HUD

- [x] Shared native/browser effects, music, animation cues and script sound ownership/cleanup with synchronous channel feedback.
- [x] Installed SURFACE.INI remapping and supported ground-derived actor surfaces. **Verified:** [grounded surface-cue comparison](https://github.com/michidk/cyrus/commit/4f66c96). Complete player traversal and audible parity remain open.
  **Verified:** native effect selection from captured surfaces, playback with rate offset 0 Hz and cleanup pass for known zero, sand, wood and rock; original live Stop and group-replacement Forget also pass. [Cleanup evidence](https://github.com/michidk/redguard-decomp/tree/b5752668b4fab7c8d8e9281ab4e83452ae2899ef/research/supplements/2026-10-14-surface-audio-acceptance). **Verified:** native/browser preparation retains original PCM width, and decoded gain uses the documented integer table or scalar rule, including live volume/pan updates. The isolated original wood recording supports the scalar correction. **Unknown:** direct native/reference waveform and duration parity.
  **Verified:** native queue startup, idle transitions and rate changes retain complete interpolated cues. The corrected known-zero clip matches independently scaled PCM before system resampling; complete original audible parity remains open.
  **Verified:** actual native mixer output for all four surfaces matches independently scaled PCM before system resampling. The reference emulator's interpolation/filter model reproduces all four original streams sample-for-sample from that output; wrong gain, filter and rate controls fail. [Native/reference output evidence](https://github.com/michidk/redguard-decomp/tree/7d1e0e8d52b2a96016189eba426a649254ba458f/research/supplements/2026-10-15-surface-audio-acceptance/actual-native-reference-output). Complete automatic player walking and direct native/browser audible acceptance remain open.
- [x] Installed HUD elements, activation prompts and persistent F8/Tab/F7 preferences. Placed-player heading/world and script-actor health/type feed the HUD.
- [x] Native restart and browser reload acceptance for F8/Tab/F7 preferences, including restored candle visibility and subsequent toggle notices. **Verified:** native health-number placement matches the original at 640×480; both builds cycle starting items and display the installed gold count of 53. Complete pixel parity remains open.
- [ ] Complete positional/ambient scheduling, maintenance/reset ownership, StopAllSounds results, shell/code objects, KillMe/Resurrect cleanup, rope/torch behavior and moving-emitter gain/pan.
- [ ] Complete global lifecycle and menu/dialogue maintenance. Lockout support and an explicit ordered reset adapter exist; automatic player-controller inhibition, topic execution and death/load resets remain open. Portable RTX preparation, the 20-entry cache, explicit combat-taunt cache replacement and shared-sample dialogue playback are implemented. Authored actor taunt lists, explicit ordered selection and prepared shared-sample presentation are implemented; explicit weapon draw/cleanup and synchronous resultless combat entry share authoritative actor state; automatic nonplayer invocation now derives shared inventory and prepared-model admission and prepares taunt dependencies; complete eligibility and remaining weapon lifecycle remain open. Full New Game story acceptance remains open.
- [x] Shared `ShowBitmap`/`UnShowBitmap` slots and `AddLog` persistence/notification dispatch. **Verified:** original-instruction acceptance checks, shared dispatch/render tests and native/browser START entry. Additional logbook styles/editing, save integration and original notification cadence remain open.
- [x] Shared compass use through installed bindings with known actor gates. **Verified:** headless checks cover ownership, rebinding, held-use consumption, lockout and HUD/world recreation. Shared inventory and production actor inputs are connected below; save restore remains open.
- [ ] Connect remaining production activation/sword state and player walk-mode/auto-defend behavior.
- [x] Shared script/HUD inventory, capped `AddItem`, `HaveItem`, `SelectItem` and compass use from logical-player state. **Verified:** headless script and HUD checks cover acquisition, live counts/selection, surface gates and world/HUD replacement; installed export checks cover all item artwork. New-game/death resets, saves and original timing comparison remain open.
- [x] `ActiveItem` and receiver-owned `HandItem` services. **Verified:** headless VM tests cover selected hidden/unowned records, admission gates, full-ID lookup before byte assignment, routed receivers and unavailable host state; a truncation mutation is rejected. Initial held-item reconstruction and attachment geometry remain open.
- [x] Shared logbook layout and native/browser reading screen. **Verified:** headless contracts, installed artwork export and native/browser input, scrolling and suspension checks. Scroll shading/filtering, other styles, editing and resets/saves remain open.
- [x] Compare original candle health restoration and flicker frame order and installed sabre-to-gold cycling. Correct stale-health animation ordering, use shared nominal ticks for HUD countdowns and suspend HUD updates in the console. **Verified:** original recordings identify healing frames 29,23,17,11,5,0 and the five-frame flicker loop; original/native item cycling displays gold count 53. The stale-order control fails the integrated health check.
- [x] Establish an original activation reference at ISLAND Siona. **Verified:** the installed prompt changes to gold/count 53 after Period and returns after release; Ctrl starts conversation. A captured-state classifier rejects a shifted sequence. Gold artwork placement matches the configured anchor in original and retained native captures; a shifted-placement control is worse. This does not establish production activation or full pixel parity.
- [x] Supply live ordinary activation inputs and shared HUD prompt presentation. **Verified:** host and HUD-system tests cover installed-label lookup, live actor/menu state, held-use ownership and nominal-tick holdoff without inspection inputs; a reversed-input control fails. Standalone initialization owns the lineup result; unsupported alignment invalidates it. Full installed interaction remains open.
- [ ] Complete representative real-time candle cadence, all snuff intermediates, matched production activation prompt/item timing and remaining HUD layout against the original. **Unknown:** the slow software-emulated capture does not establish hardware cadence or complete prompt interaction parity. Ordinary activation now samples live actor/input/RTX state and the initialized shared lineup result. PlayerLineUp movement remains unsupported; failed alignment invalidates the result. **Verified:** [published alignment rules](https://github.com/michidk/redguard-preservation/compare/main...docs/player-lineup-execution) establish per-task mode/reference ownership and distinct distance thresholds, checked against original instructions. Directional movement, collision-query cleanup and installed prompt/conversation acceptance remain open.
- [ ] Compare effects, speech, subtitles and music by ear. Establish listener/camera rules, pan direction, text-only subtitle timing and attached-emitter eligibility.
- [ ] Evaluate optional Modern suppression of the startup CD burst after paired original/cyrus audio capture. Faithful retains installed playback; no suppression exists.

### Startup and book menu

- [x] Installed Windows splash/title and data-driven book motion, settings, keyboard navigation, feedback and authored Quit.
- [x] Streamed native/browser Movies with skip points, subtitles/interlace and retained menu selection.
- [x] New Game and temporary Load enter the installed starting world behind artwork and dependency readiness. Original-style world-entry progress text is implemented.
- [x] Compare settled startup title, Options/Display layout and native loading-bar presentation with the original; verify native/browser startup skipping and subtitle persistence across restart/reload. Full pixel and timing parity remain **Unknown**.
- [ ] Complete splash, camera clipping, book-transition timing, loading-artwork and audible comparisons.
- [x] Browser Movies acceptance. **Verified:** maintainer playtest. Keyframe seeking remains unsupported.
- [ ] Complete New Game initialization, save slots/thumbnails, overwrite/delete, Continue/pause and progression movie unlocks as gameplay state becomes available.
- [ ] Support mouse/joystick rebinding. **Verified:** the original book menu has no pointer label selection.

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
- [x] Player runtime and placement snapshot APIs, decoded player-source identity, and restored-entry preservation are checkpointed for the controller owner. Final isolated checkpoint gates are pending; this does not establish complete save/load or installed traversal acceptance.
- [x] Bounded ordinary keyboard walking adapter with retained walk-mode state, controller prelude, selected-player phase-zero combat maintenance and shared sound-maintenance timing. **Verified:** synthetic scheduler and ordinary-stage contracts.
- [ ] Complete installed idle/held-forward/release acceptance across zero, sand, wood and rock; preserved offline audio evidence does not establish this route.
  Installed weapon/fighting queries now allow the selected-player script to reach movement. **Verified:** a headless ISLAND probe with explicit view context completes idle, held forward and release without selected-player failure. Other world actors still report unsupported services/queries; full traversal remains unaccepted. Fighting selection follows the [published ordered query contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md#fighting-query).
- [ ] Establish complete eligibility/timing and compare fractional startup, steady movement, release, walls and ground updates.
- [ ] Complete Faithful movement, turning, follow camera, jumping, gravity and quick actions. Recover sword, walk-mode and auto-defend rules before connecting them.
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
- [ ] Authored world transitions and old sound/script/transient cleanup.
- [ ] Pause/resume of simulation and audio; original save-slot menus.
- [x] Publish [quick-save request and reserved-slot rules](https://github.com/michidk/redguard-preservation/commit/efb9deabefcb41665dfb3528bac38c16a8fb4a26) and document [gameplay book shortcuts](https://github.com/michidk/redguard-preservation/pull/79) and [menu-before-quick-save ordering](https://github.com/michidk/redguard-preservation/pull/84). **Verified:** original input/consumer instructions and rejected altered-branch controls. Complete save eligibility remains open.
- [x] Document [save-list ordering](https://github.com/michidk/redguard-preservation/pull/82). **Verified:** original x86 ranking on synthetic records, save-file timestamp provenance and rejected ordering mutations. Live menu comparison remains open.
- [x] Publish [Save/Continue death restrictions](https://github.com/michidk/redguard-preservation/commit/7e70b7c66c0dd754f6c071112de7f6a704a95106), [Escape/death restrictions](https://github.com/michidk/redguard-preservation/commit/d37c93d) and [Quit autosave lifecycle](https://github.com/michidk/redguard-preservation/commit/c42034e). **Verified:** original branches, shared death override, installed entry mapping and rejected inverted-guard controls. Other save eligibility and runtime handlers remain open.
- [x] Publish [original inventory save records](https://github.com/michidk/redguard-preservation/compare/main...feat/save-inventory) and direct restoration rules. **Verified:** original instructions, typed codec tests, two original save-slot round trips and a rejected wrong-endian control. Runtime restoration remains open.
- [x] Validated VM snapshot/restore with exact source compatibility and task-token rebinding. **Verified:** headless resume, foreign-task, pause/counter and malformed-state checks. Complete actor/world snapshots and storage remain open.
- [x] Complete animation playback snapshots with installed-source validation and effect-free restoration. **Verified:** headless resume, delay/loop/exit, sound-latch and malformed-state contracts. Game-owned frame/sound binding and complete actor/world restoration remain open.
- [x] Complete clock snapshots with saved settings and explicit host-epoch rebasing. **Verified:** headless continuation, warmup/smoothing, fractional carry, wrap/backward samples and malformed-state contracts. Original SAV import and whole-game restore remain open.
- [x] Atomic global-flag value/latch snapshots with exact installed-schema validation. **Verified:** native-operation continuation, signed values, malformed-state rejection and installed-definition round trips. Cross-world retention, original SVGF import and whole-game restore remain open.
- [x] Native-task replacement snapshots validated against VM calls and explicit token/service bindings. **Verified:** headless wait/motion continuation, allocator history, paused slots, malformed bindings and animation request/cue retention. Complete service-owner and actor/world restoration remain open.
- [x] Journal replacement snapshots with exact installed-text validation. **Verified:** headless text/tag/notification continuation, failed-write suppression and malformed-state rejection without storage I/O; an installed-data document also survives native publication and fresh-process reconstruction, with truncated data rejected. Complete session restoration remains open.
- [x] Inventory replacement snapshots retain counts, mutable limits, visibility and selection without acquisition replay. **Verified:** focused continuation and malformed-state contracts.
- [x] Dialogue-record replacement snapshots with explicit actor/sample rebinding. **Verified:** public headless contracts cover ordered voiced continuation, inactive references, signed lifetimes, skip latch, presentation selection and rejected malformed/aliased bindings. Audio reconstruction and complete session restoration remain open.
- [x] Texture and terrain playback replacements retain image deadlines, holds, historical normals and texture admission with a rebased presentation clock. **Verified:** headless production updates compare retained state and visible mesh attributes across fresh entity bindings, camera/window changes and removal; malformed sources, bindings and geometry are rejected. Whole-session publication and native/browser save acceptance remain open.
- [x] Complete configured script-HUD replacement snapshots compose inventory and journal with compass, held-use latch, power-up slots and replacement order. **Verified:** public continuation/malformed-state contracts and installed-data native publication with fresh-process reconstruction, including failed-write protection and altered-state controls. Replacement performs no storage I/O or startup acquisitions. Unconfigured and failed local state cannot be captured. Whole-session restoration remains open.
- [x] Exact source-decoder replacement checkpoints for effects, speech and CD music retain cursor, gains, looping and pause state. **Verified:** public continuation/rejection contracts, installed effects/CD through the headless host, unchanged CD PCM and browser compilation. Channel/dialogue assembly and downstream output buffers remain open.
- [x] Document [original restoration audio lifecycle](https://github.com/michidk/redguard-preservation/pull/83). **Verified:** original instructions and altered controls cover cleanup, reset-flag lifetime, saved native-task continuation and CD track-selection paths. Live music continuity and installed runtime revisit acceptance remain open.
- [ ] Complete session candidate and coordinated live commit. **Verified:** focused logical-world/native-task continuation and first-plugin-update checks preserve current/visited identities, clock and player entry state. Focused sample-pool tests retain source-decoder progress, pause and silent animation ownership while rejecting incomplete replacements. Focused presentation replacement checks retain HUD continuation, camera/framing, sky scrolling, the presentation epoch and per-world environment edits. Complete rendered publication, full audio/backend policy, installed host assembly, revisits and restart acceptance remain open.
- [ ] Safe native/browser persistence for world, player, NPC, object, quest, inventory, script and HUD state. **Verified:** native publication and fresh-process typed readback preserve the complete installed current-plus-visited document; malformed input and failed writes preserve accepted bytes. Writer-interruption controls pass before sync and rename. **Verified:** the installed START session document exceeds localStorage capacity in Chrome; failed publication preserves the accepted record across reload. The typed Rust/WASM IndexedDB prototype retains the complete two-world document across reload; malformed candidates and aborted writes preserve accepted bytes. Production browser integration, complete session restoration and power-loss durability remain open. Format readers/writers belong in preservation.

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

Shared aggregate, registry/service replacements, rendered staging, capture hooks
and asynchronous load preparation are committed as work in progress. The latest
asynchronous load integration test remains pending instead of completing; its
cause is unresolved. Final checkpoint gates have not run. Native load wiring,
browser-owner integration, installed live restore/restart/reload/revisits and
complete gameplay acceptance remain unchecked. Source-decoder audio checkpoints
do not include downstream converter/device buffers. See the
[SOUP completion plan](plans/2026-10-12-soup-completion.md#unfinished-session-checkpoint).

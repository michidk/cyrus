# Roadmap

Rebuild the original single-player Redguard. No multiplayer, new content, or
functionality the original did not have.

**Start with development tooling. Then world playback, the player controller, and interaction and saves.**

## At a glance

| Stage | Status | What it delivers |
| --- | --- | --- |
| Foundations | Done | Install loading, world exports, browser viewer, and development tools |
| 0. Development tooling | In progress | Build, run, drive, and compare both games |
| 1. Complete world playback | In progress | The whole world renders, animates, sounds, and runs scripts |
| 2. Player controller | In progress | Original movement, camera, collision, and interaction |
| 3. Interaction and persistence | Later | Dialogue UI, inventory, scripted camera control, and save/load |
| 4. Complete gameplay | Later | Dialogue, quests, combat, inventory, menus, and saves |
| 5. Full-game fidelity | Later | A verified playthrough from new game to ending |
| 6. Release | Later | A reproducible install and tested distribution |

“Done” means the listed capability exists in committed code. It does not mean
its appearance or behavior already matches the original.

## Who is working on what

Last checked: 10 October 2026. These are task claims, not completion claims.
Update or release ownership when work changes.

| Roadmap items | Agent / session | Current work |
| --- | --- | --- |
| Step 0: preload report, retro audio, native/reference captures | Claude Code: development tooling | Classifier, audio configuration, and capture command landed; current session activity unconfirmed |
| Roadmap, resource coverage, RGUnity review, and preview | Codex: Plan Project Completion and Preview | Review and preview complete; documentation ownership released after this commit |
| Step 2: original player parity | Codex: player-parity worktree | Input/flycam foundation landed; tested animation-gated walking startup core landed; movement eligibility and collision still block locomotion integration; `magic/artifacts/2026-10-10-player/` |
| Step 1: sky, fog, and clipping | Codex: faithful-sky worktree | Native fog and original clipping verified; workspace gates pass; browser build and F1 runtime checks pending |

| Startup and main menu | Codex: Faithful Startup Sequence | Startup and settled main-page menu verified through native MCP; ownership released. Motion, lighting, remaining actions, and New Game introduction are still pending |
| Step 1: actor and mesh animation playback | Codex: animation-playback worktree | Owns animation playback/export modules, rendered-frame binding, animation tests and related scene wiring here; explicit forward groups and mesh poses verified by native contracts, installed FLAG2 binding, and WASM compilation; next owns original activation, group transitions, and clock research |
| Steps 3–6 and remaining step-1 items | Unassigned | No confirmed active owner |

Completed handoff: Codex's Cache Downloaded Game Files session landed GLB caching.
Later engine-cache edits are not marked complete here.

## Done

- [x] Rust/Bevy workspace, install discovery, settings, and fidelity switch.
- [x] Startup preload, progress display, and parse-failure logging.
- [x] Installed 3dfx splash and title playback through the book menu; CLI and MCP skip stop at the menu.
- [x] MENU.INI-defined main-page appearance, keyboard selection and Quit, using installed geometry and rgpre fonts. Other actions remain unavailable.
- [x] Native scene loading and headless scene export.
- [x] WORLD.INI map, terrain, and palette selection; numeric IDs and map aliases.
- [x] Export all 29 installed worlds, with structural checks.
- [x] Browser scene viewer with download progress and persistent GLB caching.
- [x] Inspection flycam, TCP control, and MCP tools.
- [x] DOSBox-X runner for observing the original game.
- [x] Repository checks and CI.

Evidence: [foundation](https://github.com/michidk/cyrus/commit/8d179e4),
[world exports](https://github.com/michidk/cyrus/commit/1df0990),
[browser rendering](https://github.com/michidk/cyrus/commit/f3ff4ea),
[GLB cache](https://github.com/michidk/cyrus/commit/24ee5c4), and
[flycam](https://github.com/michidk/cyrus/commit/ce8ce02).

## Resource coverage

Parsing a file does not mean its behavior works in-game. This table covers
committed support; unverified work stays in the TODO column.

| Resource | Already loaded or used | Still TODO |
| --- | --- | --- |
| `WORLD.INI` | World list, map, terrain, and palette selection | Verify every world against the original |
| `RGM`, `WLD` | Static scene placement and terrain | Actor behavior and full world playback |
| `3D`, `3DC`, `ROB` | Static export, retained animation frames, and explicit source-mapped i32 mesh poses; settled book-menu geometry | Automatic actor playback, other frame encodings, model replacement, and book motion |
| `BSI`, `COL` | Textures and palettes; documented terrain waves in both graphics modes | Other animated textures and visual comparison with the original |
| `GXA` | Startup title artwork and preload parsing | Software sky projection |
| Glide splash overlay | Installed authored meshes, matrices and textures rendered into a local startup cache | Exact hardware edge coverage, mip selection and shadow interpolation |
| Glide sky BSI/COL | Static initial sky plane in both graphics modes, per preservation | Full projection comparison, scrolling, tilt, and sun disc |
| `SFX` | Preload parsing | Verified sound playback and ambient/script triggers |
| `RTX` | Preload parsing | Speech audio and text, subtitles, dialogue choices, and quest integration |
| `FNT` | Startup labels, indexed book-menu glyphs, and rgpre-generated status font | In-game text and remaining menus |
| `CHT`, `PVO` | Preload parsing | Runtime use and behavior verification |
| SOUP scripts | No working interpreter accepted | Execution, events, native bindings, and synchronization |
| `MENU.INI` | Main-page labels, layout, selection flags, texture slots and action IDs | Submenu behavior and supported positive actions |
| `KEYS.INI` | Menu activation key and held player-input foundation | Gameplay locomotion |
| `COMBAT.INI`, `ITEM.INI` | No gameplay integration accepted | Combat and items |
| CD audio | No playback accepted | Music playback, timing, and volume controls |
| Save data | No save/load support accepted | Persistent world, player, script, and quest state |

**Verified:** Glide sky textures, their independent COL palettes, and the static
plane mapping are documented in preservation's
[sky renderer notes](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/sky.md).
Installed day, night, sunset, and Necro exports pass palette and plane checks;
native Vulkan and browser WebGL runs show the static sky behind scene geometry.
The former GXA cubemap and approximate water animation are removed.
**Verified:** uncovered pixels use the Glide `world_fogrgb` clear color. Re-export
old environment sidecars to replace legacy palette backgrounds. Sky
scrolling, tilt, and the sun disc remain unsupported; a static sky does not
establish complete original-game fidelity.

**Verified:** terrain water uses preservation's corrected selected-corner mask,
whole-grid spatial phase, quarter-shifted rotation table, half-amplitude bias,
and BIOS-rate timing. Behavioral and installed-world export checks pass.
Native Vulkan and headless browser WebGL rendering use private terrain meshes
and shared displaced normals. A native loaded-world probe records changing
water heights; scene cleanup and clock continuity pass behavioral tests.
A matched original-game camera comparison remains unchecked.
The session clock does not synchronize absolute phase with a DOS clock.

**Verified:** native MCP menu checks cover disabled-entry skipping, circular
selection, repeat, unavailable actions, Quit, and startup/menu/world transitions.
Editing the local MENU.INI label changes the displayed entry without changing
its Quit handler. The original settled menu confirms framing and text styles.
Lighting, opening/closing motion, menu audio, mouse selection, gameplay snapshots,
and neighboring-page text remain unsupported. This is main-page presentation,
not full menu functionality.

Sound remains uncommitted and unverified.
Configuration-selected language and audio assets also need runtime verification.

The preload classifier now reports results by file type. Run and review its
report to confirm installed-file coverage; parser availability alone is not proof.

## 0. Development tooling

**In progress. Supports every stage.**

Agents and developers must run, drive and compare both games on a machine without a GPU, a screen or root.

- [x] Original game in DOSBox-X (Voodoo emulation) behind noVNC, mixer on so WAV capture works. Preview recovery after restart verified by health checks and a browser capture of ship combat, ocean, and sunset sky.
- [x] Browser build with a download progress bar; stems or WORLD.INI indices select the scene.
- [x] Headless mode, TCP control protocol, MCP server, project skills for Claude Code and Codex.
- [x] `just reference` fetches and updates the preservation and RGUnity reference checkouts under gitignored `magic/reference/`.
- [x] Preload coverage report per file type and unsupported extension (`cyrus-config/preload-report.json`).
- [ ] World-resource audit: skipped placements, missing assets, failed animation/script initialization, and unsupported script operations.
- [x] Shared debug menu for native and WASM ([`ba0ab4a`](https://github.com/michidk/cyrus/commit/ba0ab4a)).
- [x] `just smoke-mcp` drives the headless game through MCP; movement and the full loop verified in [c713082](https://github.com/michidk/cyrus/commit/c713082).
- [x] Native game loads a scene under the virtual display; runtime image and asset-root fixes landed in [9535ce6](https://github.com/michidk/cyrus/commit/9535ce6).
- [x] `just compare <label>` captures both games into `cyrus-config/compare/` ([9535ce6](https://github.com/michidk/cyrus/commit/9535ce6)).
- [ ] Capture matching scenes and viewpoints with that command. The verified pair showed cyrus START and the original load-game book, not a matched comparison.
- [x] The fidelity switch gates graphics; browser sky/water toggling verified in [4a7696d](https://github.com/michidk/cyrus/commit/4a7696d). This proves switching only; unsupported rendering approximations are being removed.
- [ ] Split `cyrus-game` into crates by concern (assets, world, rendering, control) once stage 1 has landed.
- [ ] Codex set up end to end from a fresh clone, including project trust.
- [ ] CI green since the repository restructure.

**Done when:** A fresh clone with a linked install builds, runs both games, is driven from an agent, and produces a comparison capture using only the commands in `docs/SETUP.md`.

The debug-menu agent verified a fresh WASM build in the browser: F1 opens and
closes the menu, changing speed from 100 to 500 changes movement, and switching
ISLAND to START updates the URL and fetches the matching GLB and environment.
Fidelity controls select live, and typing WASD in a text field does not move the
player. Fresh loads showed no WebGL shader error. These checks establish debug
controls, not faithful world playback.

The shared gate log records formatting, strict workspace clippy, all 14
executable tests, and the enabled doc-test targets passing. The WASM build and
preview health checks also passed. The debug menu landed in
[ba0ab4a](https://github.com/michidk/cyrus/commit/ba0ab4a).

## 1. Complete world playback

**In progress.** Static loading works. Complete playback is still open.

- [x] Load and export worlds with their terrain, textures, and palettes.
- [ ] Compare placement, scale, lighting, sky, transparency, and visibility with the original.
- [x] Retain actor animation metadata, original section payloads and model frames in native/WASM assets. **Verified:** all 29 installed worlds export without unresolved animation models; native asset tests and WASM compilation pass.
- [x] Apply explicit i32 model poses and forward groups using supplied simulation ticks. **Verified:** 17 native animation contracts, installed FLAG2 GLB deformation/restoration, and WASM compilation.
- [ ] Play actor and mesh-frame animations, including loops, exit sequences, queued transitions, and animation event cues.
  Explicit forward groups and source-mapped mesh poses are supported.
  Automatic activation, the host-clock driver, default/queued transitions and
  event cues remain unsupported. Original-game visual comparison is pending.
  Frame reference, countdown and walking-marker facts are published in preservation
  commits `8c3856e`, `9b4206b` and `92b9933`. Request priority and default fallback
  are documented in `84cc21d`; they are not integrated into playback yet.
- [x] Play documented terrain water waves in native and browser builds.
- [ ] Compare water with the original; play other animated textures and effects.
- [ ] Play positional and looping effects, ambient sound, speech, and music with original timing and volume behavior.
- [ ] Run SOUP control flow, variables, global flags, object references, main/interrupt execution, waits, and concurrent tasks.
- [ ] Activate every required object script; report unsupported operations rather than silently excluding objects.
- [ ] Support scripted object groups, attachments, movement, lights, visibility, and world-required NPC navigation and perception.
- [ ] Synchronize scripts, animation events, speech/subtitles, and sound; resume waiting scripts at the correct time.
- [ ] Execute script-requested world loads and reinitialize playback state; player-facing transition UI follows in step 3.
- [ ] Carry the required playback data into both native and browser world views.
- [ ] Verify every world and palette variant with repeatable timed comparisons.

**Done when:** every world plays its required visuals, animations, sound, and
scripts correctly against the original. Missing behavior remains a blocker.
Use the inspection camera and controlled events until the player controller exists.

**Next tasks:**

1. Capture the same scene in cyrus and the original.
2. Run and review the preload classifier, including unsupported and failed data.
3. Choose reference sequences that exercise animation, water, sound, and scripts.
4. Verify reference audio now that the retro runner enables it.
5. Inventory runtime data and SOUP bindings; record operations actually called by the reference sequences and unsupported paths.

## 2. Player controller

**In progress. Gameplay integration requires animation and collision support.**

Published prerequisites:

- Marker-array decoding corrected in preservation ([c7aea52](https://github.com/michidk/redguard-preservation/commit/c7aea52)); validated against 215 markers in 27 installed maps, with parser regression tests.
- Player coordinate units and ordinary ground-turn arithmetic documented ([1058051](https://github.com/michidk/redguard-preservation/commit/1058051)). Original walking and turning coordinates are captured, including the right-turn heading sign. These observations do not establish controller parity.

FPS-derived frame-scale smoothing is now documented and checked against a native
original-game recording ([16703d1](https://github.com/michidk/redguard-preservation/commit/16703d1)).
Forward-walking startup advances while animation can suppress translation.
Live original-game observations confirm the first displacement can use a later
startup phase ([e839ed8](https://github.com/michidk/redguard-preservation/commit/e839ed8)).
This explains the recorded integer-position sequence, but its fractional endpoint
still differs by two raw coordinate units. Complete animation eligibility,
collision, and release timing remain unresolved; locomotion is not enabled.

Input foundation: eight held directional/action bindings from
`KEYS.INI`, Modern WASD/Space/held-Shift input, an independent debug flycam
toggle, and input/camera-isolation tests. A deterministic ground-turn core
matches captured original heading changes, including a frame-scale change and
release. It is not connected to live gameplay while timing and state eligibility
remain unresolved. Workspace tests, strict Clippy, formatting, publication checks,
and an MCP smoke test pass using isolated worktree artifacts. Player tests cover
installed bindings, Modern input, camera isolation, ground-turn arithmetic, and
animation-gated walking startup. The walking core supports step
type 1 and rejects other step types. It is not connected to gameplay. Translation
and the original follow camera remain unsupported.

- [x] Land and verify the keyboard-input and independent debug-flycam foundation.
- [ ] Read and apply the original player spawn selection and position.
- [ ] Implement quick actions and the walk-mode toggle, alongside the held bindings.
- [ ] Establish a repeatable simulation-timing oracle and animation movement gates; compare the same walking/turning sequence in both games.
- [ ] Add Modern camera-relative WASD, mouse camera, Space jump, and held Shift run.
- [ ] Reproduce movement, turning, camera behavior, jumping, and gravity.
- [ ] Match collision on walls, slopes, steps, and animated geometry; reproduce slope sliding.
- [ ] Match ledge hanging, sideways movement, dropping, and pull-up clearance.
- [ ] Carry the player on moving/rotating platforms and expose standing/contact state to scripts.
- [ ] Support swimming, climbing, falls, and scene exits where the original does.
- [ ] Connect player actions to world scripts, animations, and sound; hand control to scripted animations and restore it afterward.

**Done when:** recorded input routes match the original and interact with the
world correctly, including collision and scene transitions.

## 3. Interaction and persistence

**Later. Requires the player controller.**

These are concrete gaps found in RGUnity's source, not a goal to copy RGUnity.
Implement their original-game behavior, including the original UI.

- [ ] Interaction prompts and one eligible target at a time, including overlapping pickups; collected objects stay removed on revisits.
- [ ] One inventory shared by scripts, quantities/icons, active-item selection, item-use rules, and saves.
- [ ] Dialogue choices, including disabled choices, and subtitles tied to speech and animation; return the selected choice to the waiting script.
- [ ] Scripted camera shots and temporary player-control lockout, followed by a reliable return to player control.
- [ ] World transitions with the correct destination spawn, orientation, and loading artwork; stop old sounds/scripts and remove transient objects.
- [ ] Pause/resume for world simulation, scripts, animation, and audio; save/load menus with the original's slot-management behavior.
- [ ] Save and restore world identity, player position, script inventory, NPC state, and changed world objects across revisits. Include script threads, waits, tasks, flags, and object references needed to resume. Write safely into `cyrus-config/`.

**Done when:** a reference sequence can pick up an item, select it, make a
dialogue choice, transition worlds, save, restart, and resume with the same state.
Scripted camera sequences must restore control correctly.

**Source check:** [RGUnity revision 6789058](https://github.com/RGUnity/redguard-unity/tree/678905846bec4942bf1492c54fdd6f479b1482fe),
inspected as source only; not playtested here.

RGUnity has separate execution paths. Its enabled build scenes are Launcher,
Setup, ModelViewer, and IslandTest. The scripted GameTest scene and older Menu
scene are excluded from that list. Source presence does not establish that a
feature is reachable in the packaged game.

| Source evidence | Implementation and limits | Our stage |
| --- | --- | --- |
| [Build scenes][rg-build] and [world loader][rg-worlds] | Packaged area viewer and separate scripted test world; scripts use an explicit object allowlist | 0, 1 |
| [Model loader][rg-models] | Geometry, terrain, lights, colliders; texture frames decode, but no world frame-cycling consumer was found | 1 |
| [Scripted objects][rg-scripts] | Task scheduler, animation, speech, camera and native commands; many operations stubbed, navigation incomplete | 1, 2 |
| [Animation groups][rg-animation] | Frame sequences and transitions; sound/break/rumble markers skipped | 1 |
| [Player prototype][rg-player] | Platform contact and script state; older ledge/slope controller is disconnected from scenes | 2 |
| [Interaction and items][rg-interactions] | Older pickup/door prototypes; runtime activation can affect several nearby objects at once | 3 |
| [Inventory UI][rg-inventory] and [runtime inventory][rg-store] | Separate string-keyed and numeric inventories; runtime item use unfinished | 3, 4 |
| [Dialogue UI][rg-ui] | Speech/subtitle and selection bridge; disabled-choice flag ignored | 3 |
| [Save system][rg-saves] and [menus][rg-menus] | Older scene/prefab prototypes; saves do not include the numeric script inventory | 3, 4 |

No integrated world sky/water or music playback was established by this review.
Sound handlers exist, but spatial/volume semantics remain incomplete. Script
execution and decoded assets therefore cannot be treated as complete playback.

These sources identify missing work. They do not prove faithful behavior or a
complete game. RGUnity's model viewer, editor tools, and GLTF export add no
gameplay scope.

## 4. Complete gameplay

**Later. Requires interaction and persistence.**

- [ ] Complete dialogue conditions, quest state, and consequences across content.
- [ ] Complete item-use rules, NPC navigation/perception, combat, damage, death, and restart.
- [ ] Complete original fonts, settings, and remaining gameplay feedback.
- [ ] Verify script and quest persistence across all content, including interrupted tasks and changed object attributes.
- [ ] Verify the interaction and persistence systems across the full gameplay route.

**Done when:** a reference route with dialogue, an item, combat, and a scene
transition completes with matching outcomes, and can be saved and resumed.
Original-save compatibility needs format evidence before a commitment.

## 5. Full-game fidelity

**Later. Requires complete gameplay.**

- [ ] Cover all worlds, quests, actors, items, encounters, and endings.
- [ ] Test revisits, alternate quest order, failure paths, and repeated transitions with active audio, animation, and scripts.
- [ ] Finish a new-game-to-ending playthrough with reference checkpoints.
- [ ] Resolve remaining blockers and unverified behavior.

**Done when:** the full playthrough succeeds and the coverage list has no
unexplained omissions or unsupported behavior required to finish the game.

## 6. Release

**Later. Requires full-game fidelity.**

- [ ] Choose and test supported platforms and hardware.
- [ ] Measure loading, frame pacing, and bounded memory across repeated world transitions; fix observed bottlenecks.
- [ ] Verify a clean setup with a purchased GOG install.
- [ ] Run release checks and audit packages for game data and research artifacts.
- [ ] Publish setup instructions, controls, and known limitations.

**Done when:** a clean machine can install, play, save, and resume the tested
release. Distributed packages contain no game assets.

## Rules throughout

- The original game is the specification. RGUnity source helps identify missing work, not define behavior.
- `rgpre` owns formats and engine facts; contribute discoveries there.
- Read the install only. Keep local state in `cyrus-config/` and research untracked.
- Default to Faithful. The fidelity switch does not authorize extra functionality.
- Compare behavior before marking it done. Run `just gates` before each commit.
- No decoders derived from third-party GPL sources.

This file is the plan's source of truth. Update checkboxes when evidence exists.
See [METHOD.md](METHOD.md) for verification and [SETUP.md](SETUP.md) to run both games.

[rg-interactions]: https://github.com/RGUnity/redguard-unity/blob/678905846bec4942bf1492c54fdd6f479b1482fe/Assets/Scripts/Interactables/Item.cs
[rg-inventory]: https://github.com/RGUnity/redguard-unity/blob/678905846bec4942bf1492c54fdd6f479b1482fe/Assets/Scripts/Inventory/InventoryUI.cs
[rg-scripts]: https://github.com/RGUnity/redguard-unity/blob/678905846bec4942bf1492c54fdd6f479b1482fe/Assets/Scripts/RGFileImport/RGMData/RGScriptedObject.cs
[rg-ui]: https://github.com/RGUnity/redguard-unity/blob/678905846bec4942bf1492c54fdd6f479b1482fe/Assets/Scripts/Core/UIManager.cs
[rg-saves]: https://github.com/RGUnity/redguard-unity/blob/678905846bec4942bf1492c54fdd6f479b1482fe/Assets/Scripts/Save%20System/GameSaver.cs
[rg-menus]: https://github.com/RGUnity/redguard-unity/blob/678905846bec4942bf1492c54fdd6f479b1482fe/Assets/Scripts/Menu/PauseMenuLoader.cs
[rg-worlds]: https://github.com/RGUnity/redguard-unity/blob/678905846bec4942bf1492c54fdd6f479b1482fe/Assets/Scripts/Core/WorldLoader.cs

[rg-build]: https://github.com/RGUnity/redguard-unity/blob/678905846bec4942bf1492c54fdd6f479b1482fe/ProjectSettings/EditorBuildSettings.asset
[rg-models]: https://github.com/RGUnity/redguard-unity/blob/678905846bec4942bf1492c54fdd6f479b1482fe/Assets/Scripts/FFI/FFIModelLoader.cs
[rg-animation]: https://github.com/RGUnity/redguard-unity/blob/678905846bec4942bf1492c54fdd6f479b1482fe/Assets/Scripts/RGFileImport/RGMData/RGRGMAnimStore.cs
[rg-player]: https://github.com/RGUnity/redguard-unity/blob/678905846bec4942bf1492c54fdd6f479b1482fe/Assets/Scripts/GameTest/PlayerMain.cs
[rg-store]: https://github.com/RGUnity/redguard-unity/blob/678905846bec4942bf1492c54fdd6f479b1482fe/Assets/Scripts/RGFileImport/RGMData/RGObjectStore.cs

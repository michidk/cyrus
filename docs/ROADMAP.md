# Roadmap

Rebuild the original single-player Redguard. The original game is the specification;
no multiplayer or new content. Build tooling first, then world playback, the player
controller, interaction and saves.

## At a glance

| Stage | Status | Goal |
| --- | --- | --- |
| Foundations | Done | Install loading, world exports, browser viewer and development tools |
| 0. Development tooling | In progress | Build, run, drive and compare both games |
| 1. Complete world playback | In progress | Render, animate, sound and execute every required world script |
| 2. Player controller | Parked at user request | Original movement, camera, collision and interaction |
| 3. Interaction and persistence | Later | Dialogue, inventory, scripted cameras, transitions and saves |
| 4. Complete gameplay | Later | Quests, combat and complete gameplay services |
| 5. Full-game fidelity | Later | A verified new-game-to-ending playthrough |
| 6. Release | Later | A tested distribution and clean-machine setup |

Checked items exist in committed code. They do not imply original-game fidelity.
Unchecked items remain required. **Verified** identifies recorded evidence;
**Unknown** identifies unresolved behavior. Verification details belong in commits
and implementation docs, rather than repeated status histories here.

## Next work

- Connect New Game world entry to automatic eligible actor updates, using the shared clock and authored script/animation order. START must visibly advance without inspection commands; repeat across all installed worlds. Supplied-tick inspection is already supported.
- Finish terrain/model lighting, water-edge normal retention and sky/sun filtering comparisons.
- Connect automatic sound triggers, surface remapping and moving emitters.
- Feed production player/script state into the HUD and verify native/browser presentation.
- Compare matched original/native/browser scenes and audible sequences. Keep controller work parked until requested.

The [SOUP completion checklist](plans/2026-10-12-soup-completion.md) owns detailed
script service, scheduling and persistence dependencies. Crate ownership is in
[MAP.md](MAP.md); launch and cache requirements are in [SETUP.md](SETUP.md).

### SOUP completion status

**Verified:** SOUP is not complete from an implementation standpoint. The portable
VM, asset preparation and explicit game hosts exist and have native/WASM checks.
Normal world entry prepares actors and render bindings. Camera-centered ordinary
updates run the supported script/animation stages; unsupported collision, combat,
and native services stop the affected actor with a contextual diagnostic.
Loading the correct map does not establish complete animated or playable world behavior.

| Requirement | Current state |
| --- | --- |
| Build the existing native and browser cores | Verified on committed SOUP increments; compilation is not gameplay acceptance |
| Load installed worlds and retain scripts/models | Implemented; all 29 WORLD.INI entries have export coverage |
| Run script/animation stages explicitly | Implemented for the documented supported services |
| Start eligible world animations and scripts automatically | Incomplete; first acceptance is New Game into START without debug stepping |
| Execute every required native service and actor lifecycle | Incomplete: navigation, remaining receivers/properties, synchronization and gameplay services |
| Preserve progression through revisits and saves | Not implemented |
| Complete original/native/browser playthrough | Not verified |

### Active assignments

| Work | Session |
| --- | --- |
| Stage 0 audit triage, capture provenance and gate automation | Codex, tooling follow-ups; `magic/artifacts/2026-10-13-stage0-followups/` |
| Rendering: authored texture animation and remaining sky/filtering evidence | Codex, `t3/rendering-stage-one`; `magic/artifacts/2026-10-13-texture-animation/`; protected SOUP/save files excluded |
| SOUP execution and script integration | Codex, Plan the SOUP Implementation, `t3/soup-implementation-roadmap`; `magic/artifacts/2026-10-12-soup-spatial-runtime/` |
| Menu save/load slots and save-state capture | Codex, Implement Menu Save and Load, `t3/implement-savegame-menu` |

The SOUP assignment includes remaining animation and script scheduling. Sound and HUD sessions have ended; their open items, startup fidelity follow-up and the rest of stages 3–6 are unassigned.

### Crate boundaries

- [x] Keep shared font metrics, wrapping and rasterization in `cyrus-text`; screen-specific text placement remains with presentation and FNT parsing with preservation.
- [x] Extract `cyrus-media`: portable movie decoding, packet input, PCM conversion and waveform envelopes shared by asset preparation, the viewer and playback. Keep installed menu metadata, platform I/O and audio/render backends outside the codec core. **Verified:** installed streamed frame/PCM contracts, native headless START, installed WASM worker exports, metadata parity and isolated core/browser builds.
- [x] Extract `cyrus-audio`: effects, speech playback, music and browser sound-bank playback. Presentation supplies listener, startup readiness and cinematic pause state. **Verified:** installed decoder/channel/cleanup contracts, preference/animation/cue ordering, native menu/control/CD state, headless START, WASM and isolated core checks; full gates passed with zero install skips.
- [x] Extract `cyrus-startup`: splash, title, book menu, settings, cinematic presentation and world-entry loading, with one presentation facade and native preparation boundary. **Verified:** installed startup/movie/preferences contracts, full gates with zero install skips, native menu/movie/world-entry/Quit, integrated optimized browser and installed WASM worker; presentation schedules and cache formats preserved. Fidelity comparison remains open.
- [x] Extract `cyrus-collision` with terrain, model and static-world query contracts; keep player admission and response in gameplay and serialized inputs in assets.
- [ ] Revisit `cyrus-animation` after production scheduling and actor ownership settle; retain explicit requests/ticks in the core and mesh presentation outside it.

## Verification still required

**Verified:** committed increments have native contracts, installed export checks
and WASM compilation. Explicit animation, sound, spatial and activation hosts have
headless contracts. These establish the supported cores, not automatic gameplay.

**Verified:** optimized native default and skipped startup reach the ready menu;
restart retains Display preferences and a rebound key. Installed INTRO skips
reach the authored points and return to the selected Movies entry; authored Quit
exits. Native Load enters the starting world, F8/Tab/F7 update HUD state, and
held flycam input leaves the player position unchanged. Browser audio/cache,
movie streaming and server/package contracts pass. The earlier served WASM matched
its optimized build. The subsequent rebuild was terminated before emitting a
bundle, and the shared server returns 404. A replacement optimized build is in
progress; current browser runtime remains unverified until its served bundle passes
the remaining interaction checks. Headless browser checks at 1280×800 exercise URL startup
skip, temporary Load/New artwork/progress, world entry, F1 in the menu and
after New Game, and F1 in a direct START launch. F8/Tab/F7 write browser HUD
preferences and show notices.
Asset inspection now includes automatic WLD heightmaps, texture-rotation maps and
direction arrows, model normals/bounds/face picking/UVs, world record diagrams,
PVO scene/object inspection, authored movie-track selection and expanded decoded
JSON. **Verified:** integrated workspace gates pass with the install and no skipped
install tests; optimized browser/native builds and the isolated core consumer pass.
Native checks cover WLD maps/arrows, model picking/UVs, sky palettes and movie
controls. Browser checks cover restricted TEXBSI palettes, BSI JSON and the
BELLTOWR world/actor inspector. Browser WLD capture and audible playback remain
unverified because the software-rendered shared tab times out and the native
container has no audio device.

Native viewer GUI checks cover textured/unlit/solid rendering, edge overlays,
JSON inspection, font preview, ANM playback and movie frame stepping. Tagged Linux
viewer checks also verify visible wireframe geometry, Orbit drag, Flycam look and
held-key movement, and Frame object recovery; the virtual display requires window
focus for keyboard input. Image Fit/1:1/zoom and CD waveform, seek, pause and
playback and loop wrap also pass; a paced virtual audio sink receives nonzero
samples. Installed LBM palette replacement and movie play/seek/pause/step controls
respond through the GUI. These
checks establish runtime behavior, not visual or audible parity with the original. Further shared-tab inspection and
headless control captures time out, so remaining browser interaction/audio
checks stay open. **Verified:** the subsequent native development build also reaches a ready menu
and New Game world 0 with initialized script actors, health/world HUD state and
F8/Tab/F7 updates. Its shared loading edits are recorded separately from the tagged
package evidence; browser checks after these integrations remain open. Starting-world
settling reports unsupported water/terrain-edge support and retains the marker.

| Area | Remaining acceptance checks |
| --- | --- |
| World rendering | Matched original/native/browser views across worlds and palettes; placement, scale, visibility, transparency, clipping and terrain/model lighting |
| Sky and water | Tilt, synchronized scrolling, above-plane views, sun alpha/filter state after transitions, waves, water-edge normals and hardware color precision |
| Animation and scripts | Production clock and eligible callers; rendered/audible sequences; actor removal and world replacement with active scripts, meshes and sounds |
| Player | Complete startup/steady/release updates, ground and wall responses, native/browser input, original follow camera and flycam independence after integration |
| HUD | Every element and notice, original candle cadence, prompt/item behavior, compass direction and starting inventory; browser restart persistence |
| Startup and menu | Browser keyboard startup skip and restart persistence; original book motion, loading artwork and lighting/clipping comparisons |
| Movies and audio | Picture, skips, subtitles, interlace, A/V sync, integrated cancellation/download failures and seek responsiveness; audible effects, speech, CD music and pan handedness |
| Cheats and console | Original non-cheat commands, console preferences/presentation fidelity and remaining gameplay effects |
| Asset viewer | Other-platform runtime and browser media checks; native inspection controls pass, without a Faithful rendering claim. Game and viewer share asset code; the installed WASM worker prepares game assets |
| Releases | Clean-machine play/save/resume and other-platform runtime checks |

Browser tooling verification is deferred at user request: F1 during unskipped
startup, fidelity controls, render-distance editing/reset and cache reuse/invalidation.

## Resource coverage

Parsing does not establish runtime behavior. **Verified:** all 29 installed
WORLD.INI entries export geometry and supported animation references. Coverage
of every palette and world still needs original-game comparison.

| Resources | Implemented | Still required |
| --- | --- | --- |
| WORLD.INI, RGM, WLD | World selection, placements, terrain, per-world ambient/directional lighting, static map-light baking and authored object lights | Scripted light changes, actor prelighting eligibility, exact water-edge normals and actor behavior |
| 3D, 3DC, ROB | Static models, retained frames, supplied-tick poses/groups and authored book motion | Production animation, remaining encodings/group types and all-actor model replacement |
| BSI, COL, Glide sky | Textures/palettes, sky plane/tilt/sun, scrolling inputs, fog and terrain waves | Other animated textures and matched filtering/blending/lighting |
| GXA, splash DLL, FNT | Installed splash/title, menu widgets, HUD artwork and shared text rendering | Remaining gameplay screens, software sky projection and exact splash hardware rendering |
| SFX, RTX, CD audio | Effects/speech decode, subtitles, shared channels, explicit script sound ownership, native/browser music, preferences and animation surface remaps | Automatic gameplay triggers, nonplayer ground updates, moving emitters and audible parity |
| SOUP | Portable VM, actor locals/flags/tasks, supported automatic ordinary traversal, explicit animation/spatial/wait/routing/sound/activation services, shared clock/random stream and typed native-hook dispatch | Remaining automatic actor stages, receivers/native services, ambient mixer feedback, synchronization and persistence |
| MENU.INI, KEYS.INI | Installed menu content, keyboard navigation, preferences, rebinding, held player input, New/temporary Load and Movies | Gameplay handlers, device capture, Continue/pause, save slots and movie progression unlocks |
| ITEM.INI, COMBAT.INI | Starting-item HUD icons and counts | Shared inventory, pickups, item use and combat |
| SMK | Native/browser video, first-track audio, installed skip/subtitle rules and streamed browser playback | Original comparison, A/V sync and player-hardware seek/audio checks |
| BMP, PCX, LBM, ANM | Cursor/indexed image inspection and authored-rate ANM playback | Game cursor integration and palette cycling |
| CHT | Preload/viewer parsing; native/browser runtime restore/save, import/export, keyboard/console commands and `neversaydie` | Remaining gameplay effects and console fidelity; see the cheat checklist below |
| PVO | Preload parsing and viewer inspection | Runtime culling excluded at user request |
| Save data | Unsupported | Safe world/player/script/quest persistence |

## 0. Development tooling

In progress. Both games must be runnable and comparable without a physical GPU,
screen or root access.

- [x] Install discovery, preload coverage report, world exports and local-state separation.
- [x] DOSBox-X/Voodoo original-game runner, capture tools, TCP/MCP control, headless mode and replay fixtures.
- [x] Native/browser debug menu, independent flycam, world picker and F1 registration across camera replacement.
- [x] Separate asset viewer with Orbit/Flycam, four shading modes, edge overlay and staged loading progress. **Verified:** [viewer increment](https://github.com/michidk/cyrus/commit/4ee41d8) includes viewer tests, installed gates and headless browser checks.
- [x] Shared CLI/URL options, explicit browser install source, digest-based caching, byte-range serving and read-only install preparation.
- [x] Browser play from a locally selected GOG folder through read-only File handles without upload; native install selection and preparation in the game window without a launcher process. **Verified:** [implementation](https://github.com/michidk/cyrus/commit/8279a2f) passes focused native tests, browser builds and networkless worker tests. Windows and the complete native Continue transition remain unverified.
- [x] CI checks and native/browser packaging. Stable version tags build and publish packages; push/PR CI runs checks without packaging. **Verified:** [release workflow checks](https://github.com/michidk/cyrus/commit/938ae35); [v0.1.1 publication](https://github.com/michidk/cyrus/releases/tag/v0.1.1) passed with native/browser packages.
- [x] Preservation release v0.8.0 integration, including the Windows splash parser. **Verified:** [migration checks](https://github.com/michidk/cyrus/commit/10a953b) cover installed workspace gates and optimized browser builds.
- [x] Asset viewer review: failed previews are never cached, CD images fingerprint by size and time, asset lookup and catalog classification are shared, and the viewer UI is split per content kind. Game and viewer share one world geometry pipeline, cue-sheet parser, WAV writer, file lookup, world resolver, font conversion, atomic cache publish and loading overlay; native world preparation reports its stages on the entry bar. **Verified:** [viewer fixes](https://github.com/michidk/cyrus/commit/949ff34), [shared assets](https://github.com/michidk/cyrus/commit/e389da4) and [stage progress](https://github.com/michidk/cyrus/commit/9139981) pass installed tests and a headless START export; the painted stage bar is unobserved.
- [x] Browser game runtime parsing: remaining exporters use the byte-source trait and the browser worker prepares game assets without a server-side exporter. **Verified:** installed startup/menu/HUD/sound/START worker exports, bounded CD movie reads and native/WASM script metadata parity. Browser splash/menu interaction passed; complete movie interaction and audible parity remain open.
- [x] Audit skipped placements, missing assets, animation/script initialization failures and unsupported operations. **Verified:** [headless audit](https://github.com/michidk/cyrus/commit/49b16a8c1a662e98342a844d7132e8396b1046c4) loads every installed WORLD.INI entry through existing preload/export/preparation paths and writes contextual text and stable JSON. Uninitialized animations and unexercised operations remain explicit.
- [x] Produce matched original/cyrus scene and viewpoint captures. **Verified:** [fixture replay](https://github.com/michidk/cyrus/commit/49b16a8c1a662e98342a844d7132e8396b1046c4) produced original/native/browser triplets for ISLAND, BELLTOWR and START; all nine images inspected, browser headless at 640×480. Fixtures verify static scene/viewpoint matching; animation and gameplay synchronization remain unknown. Art stays under `magic/`.

Done when a fresh clone with a purchased install builds and runs both games,
accepts agent control and produces a repeatable comparison through SETUP commands.

### Original console UI and cheats

**Verified:** preservation documents the [13 cheats and activation rules](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/cheats.md)
and [REDGUARD.CHT format](https://github.com/michidk/redguard-preservation/blob/main/docs/formats/CHT.md).
Cyrus parses CHT during preload and exposes decoded values in the asset viewer.
Native startup restores local or installed CHT state; TCP/MCP supports named
toggles, integer assignment, `oracle` and `savecheats`. Automatic world updates and explicit script steps
use runtime `neversaydie` state; inspection callers may override it.
Other gameplay effects report unsupported, including the unknown `oddjob` effect.
F12 opens the shared native/browser editor; browser CHT import/export lives in F1.

- [x] Extract `cyrus-console` with subsystem command registration, editor/input, dispatch and retained reports. Native TCP and keyboard entry share dispatch. Independent-consumer, command/cheat tests and WebAssembly checking verify the boundary. Decimal, hexadecimal, percent-prefixed and packed four-byte values share one parser; short quoted input remains unsupported.
- [x] Resolve console assignments from live SOUP flag values and preserve numeric-prefix/wrapping conversion. **Verified:** isolated command tests cover script-written values, duplicate names, unknown-name diagnostics and stale-world rejection. Preservation documents temporary equate-table lifetime; no persistent equates are fabricated.
- [x] Shared cheat state, native CHT restore/save and TCP/MCP commands, including explicit SOUP `neversaydie` integration. Native storage shares byte read/replacement code with preferences. Verification is recorded in the implementation commit.
- [x] Native/browser F12 console with installed paper/font, world-palette inks, text editing, retained output and command submission. **Verified:** presentation rules are [published in preservation](https://github.com/michidk/redguard-preservation/compare/main...feat/console-presentation). Pixel parity remains part of the comparison item below.
- [x] Implement gameplay cheat-name entry followed by Enter, console toggles, explicit integer assignment and `oracle` output through shared runtime cheat state. Use preservation's names and rules; keep unsupported effects explicit.
- [x] Add browser cheat persistence and CHT file import/export using the shared state and preservation codec. Share storage, safe file replacement, import/export and error handling with save-game persistence where their requirements match; keep each format's encoding separate. Native startup restore and `savecheats` writing to `cyrus-config/REDGUARD.CHT` are implemented; the install remains read-only.
- [ ] Connect each documented cheat effect to its owning gameplay, script, animation or rendering system as those systems become available in stages 1–4. `neversaydie` already reaches automatic and explicit script queries; leave the unknown `oddjob` gameplay effect unsupported. The inspection flycam does not implement player flight cheats.
- [ ] Inventory and implement the original console's non-cheat commands from preservation evidence and original-game observations, including documented sky/water controls. Report unsupported commands explicitly; publish missing command contracts first.
- [x] Verify native input isolation, CHT save/restart, malformed state and failed writes, and install immutability. Verify browser keyboard entry, persistence/reload, malformed import rejection and byte-exact CHT import/export in headless Chrome at desktop and narrow widths.
- [ ] Complete original-console pixel parity, preferences and cursor-timing comparisons, and compare remaining cheat gameplay effects as their owners become available.

### Startup and book menu

- [x] Installed Windows metallic-logo/swoosh splash, title and data-driven book pages, including authored turns, camera moves and Quit closing.
- [x] Options, Display, Sound, Controls and Movies; keyboard navigation, persistent validated preferences and installed feedback effects. **Verified:** native MCP/export contracts and browser menu/movie/world-entry checks.
- [x] Native/browser Smacker playback, skip points, subtitles/interlace and retained menu selection. Native video/audio streams from the installed file or CD image without whole-movie preparation; browser video/audio streams before full download. CD tracks and world assignments export for shared music playback.
- [x] New Game and temporary Load enter the installed starting world/marker behind destination artwork and a dependency barrier.
- [ ] Compare splash pixels and authored animation, inherited menu lighting, camera-plane clipping, book transitions and audible output with the original. Loading duration need not match.
- [x] Bottom-right red loading status reports cyrus preparation stages and pending assets. **Verified:** installed export contracts and native menu-to-world captures; original internal progress weights remain unsupported.
- [ ] Add save slots/thumbnails, overwrite/delete handling, Continue/pause, full New Game initialization and progression movie unlocks when the corresponding gameplay state exists.
- [ ] Support mouse/joystick rebinding and gameplay snapshots. **Verified:** the original ordinary book menu has no pointer label selection.

Load currently starts a fresh world. It does not read saves. Browser Quit can be
refused by the browser and leaves the menu usable. Forward movie seeks decode
preceding delta frames; keyframe seeking is not implemented.

## 1. Complete world playback

In progress. Static loading and explicitly driven runtime cores work; automatic
world playback is incomplete.

### Rendering

- Won't implement PVO runtime culling at user request. No performance benefit or visible fidelity difference has been demonstrated. Keep PVO parsing and viewer inspection. **Unknown:** whether the original's PVO exclusions change visible output; revisit only if profiling or original-game comparison establishes a need.
- [x] Worlds, terrain, textures and palettes; dry/water terrain lighting, faithful sky/tilt/sun/scroll inputs, fog/range defaults and water waves.
- [x] Independent fidelity controls; optional Modern sky/fog/water presentation. Modern lighting currently reuses Faithful results.
- [x] Match the original world camera's horizontal projection across viewport sizes. **Verified:** original viewport arithmetic and matched BELLTOWR inspection poses in native/browser builds. Original follow-camera behavior and lighting/pixel parity remain open.
- [x] Apply [dirty water-edge normal admission/retention](https://github.com/michidk/cyrus/commit/96824363121633c891982215d8beb20f9e332361) to the current terrain grid. **Verified:** [published admission rules](https://github.com/michidk/redguard-preservation/commit/ebd7d9e), renderer tests for retained RGB, ticks, mode switches and environment replacement, and a failing omitted-diagonal negative control. Matched ISLAND ground captures compare original/native/headless browser at 640×480; native/browser shoreline captures share a pose but not wave phase.
- [x] Apply the [original finite terrain window](https://github.com/michidk/cyrus/commit/c03a3aa05de6ea74d9a0ff9e7a45acda4f143937). **Verified:** original draw/admission ranges, live rounding mode and renderer tests for cell changes, retained normals and wave-angle precision.
- [x] Compare [water edges](https://github.com/michidk/cyrus/commit/c03a3aa05de6ea74d9a0ff9e7a45acda4f143937) at a matched ISLAND shoreline pose in original/native/browser at 640×480. **Verified:** controlled zero-speed waves hold phase constant; the camera window follows the [published draw range](https://github.com/michidk/redguard-preservation/commit/0913cd1). **Unknown:** complete pixel parity and default-speed timing; edge texture/color differences remain.
- [x] Replace fixed Bevy model lighting with installed ambient/directional RGB, authored face brightness, supplied normals and solid-color tint. **Verified:** original-code/GPU arithmetic comparisons, render contracts and native ISLAND/BELLTOWR/START scene switches.
- [x] Compare static map-light baking in matched BELLTOWR original/native/browser views at 640×480. **Verified:** wall/table captures share the camera pose; lighting and filtering differences remain, so pixel parity is not established.
- [x] Recover [actor prelighting eligibility](https://github.com/michidk/redguard-preservation/commit/d1cbf68). **Verified:** the world-entry pass requires nonzero `attr_non_moving`, the runtime static-model kind and a loaded model, plus the shared static/map-light list gates.
- [ ] Apply actor prelighting from authoritative world-entry state; recover dynamic lighting overrides.
- [x] Add authored object lights after retained static RGB. **Verified:** original-code/GPU arithmetic, installed export switches, renderer contracts and matched BELLTOWR native/browser views. Scripted changes and complete pixel parity remain open.
- [x] Prepare terrain clipping, projected snapping and fixed color interpolation before integer texture modulation. **Verified:** original clipping/setup arithmetic comparisons, a failing subpixel-bias negative control, renderer contracts and improved matched ISLAND native/browser terrain regions. Fog updates exclude retained materials without live entities; the native ISLAND/BELLTOWR round trip passes, with substantial scene-loading delays still present.
- [x] Preserve selected ROB attributes on world nodes and select encoded point/bilinear model filtering. **Verified:** archive fallback/cache and shared-mesh metadata tests, material flag separation, and improved matched BELLTOWR native/browser regions. Standalone/generated model defaults and filtered chroma rejection remain unsupported.
- [ ] Compare remaining terrain sampling precision and sky/sun filtering/alpha after menu/world transitions. **Verified:** cloud-filter arithmetic has isolated GPU checks. Filtered model chroma rejection, complete pixel parity and menu transition comparisons remain open.
- [x] Recover [authored texture frame selection](https://github.com/michidk/redguard-preservation/commit/5ab06cf). **Verified:** frame requests share image state, advance once at the BIOS deadline and retain explicit selections. Rendering admission controls advancement; runtime playback remains open.
- [x] Retain [authored texture frames in exported GLBs](https://github.com/michidk/redguard-preservation/commit/d0eec6c98e6917f942e643204fc155f53b34b649). **Verified:** installed START, ISLAND and BELLTOWR export checks compare frame order, delay, palette, row orientation and transparency against TEXBSI data. Runtime playback remains open.
- [x] Retain [authored polygon admission inputs](https://github.com/michidk/redguard-preservation/commit/a4b007242d7a72eea1a54a42d5bd7afe57d0d459) alongside texture frames. **Verified:** installed START, ISLAND and BELLTOWR checks compare source corners, face normals and model bounds after material splitting and triangulation. Runtime admission and playback remain open.
- [x] Preserve [opaque terrain texture frames](https://github.com/michidk/redguard-preservation/commit/9ac35c15c66b9a5671320e42951e0a53e05ae39e), including palette-index-zero RGB. **Verified:** preservation tests cover nonblack zero colors and unchanged model transparency; installed START, ISLAND and BELLTOWR export checks pass. Re-export existing caches; this does not establish shoreline pixel parity.
- [ ] Compare sky scrolling, plane crossing, fog/clipping and water across original/native/browser views; implement other authored texture animation.

### Animation and scripts

- [x] **Verified:** actor-load preparation exports attribute defaults through preservation [PR59](https://github.com/michidk/redguard-preservation/pull/59), using installed SYSTEM.INI. Installed-map tests and a native headless PALACE check verify the initialized attributes. Model-path message-group assignment and complete actor initialization remain open.
- [x] Retained animation metadata and source-mapped i32/reconstructed-i16 poses; supplied-tick type-0/type-1 requests, priorities, default replacement, exit tails and authored sound markers. **Verified:** animation transition and grounded-update contracts cover changed current/pending defaults and retained frames.
- [x] Portable SOUP VM and logical actors; explicit animation, waits, flags, attributes, routed tasks, spatial attachment/rotation and player queries share state with presentation. **Verified:** FLAG2 supplied-tick sequences and PALACE headless spatial contracts.
- [x] Explicit activation shares receiver ownership, pending prompts and timers with HUD drawing. **Verified:** [activation increment](https://github.com/michidk/cyrus/commit/5358aa0) includes script/headless HUD contracts and installed RTX labels, including text-only entries.
- [x] Shared clock arithmetic consumes validated installed settings and explicit BIOS samples, retaining startup warmup, smoothing and fractional nominal ticks. Menu entry/exit and world replacement preserve the documented shared clock state. **Verified:** clock contracts, installed native/browser schema-6 preparation and headless START loading. This does not schedule actors.
- [x] The script schema retains typed RAHK native calls separately from script continuations; explicit dispatch preserves cursors and tasks. Fresh-world actor ordering, map defaults and axis-lock restoration have supplied-stage contracts. **Verified:** all installed maps decode, native PALACE stage/error checks and installed WASM worker preparation pass. Automatic eligibility and remaining hook services are not implemented.
- [x] Ordinary `AtNodeMarker` queries share receiver-local spatial state and matching world marker metadata. **Verified:** installed-map preparation, routed attached queries and original-instruction distance/error comparisons. Explicit HIDEOUT invocation reports its out-of-range marker; that map is not selected by installed WORLD.INI.
- [x] Explicit ordinary stages honor EnableScript/DisableScript, task bypass, installed task/animation switches and unpaused-task hook admission. **Verified:** [script-control contracts](https://github.com/michidk/cyrus/commit/3419b23), [schema-8 update switches](https://github.com/michidk/cyrus/commit/4509128), original-instruction comparisons, installed worker exports and native PALACE checks. These gates do not supply automatic callers.
- [x] Normal world updates automatically traverse supported ordinary actors with the shared clock, defaults, hooks, tasks and family transforms. **Verified:** normal-update contracts and native menu New Game into START move all 15 sharks without debug stepping. Seven crew/pirate actors still stop at unsupported collision, BOAT1 at AmbientSound, and five WATERSND objects lack their separate object path. Full world animation acceptance remains open.
- [x] Same-list duplicate family roots select the original first match instead of failing spatial preparation. **Verified:** installed CATACOMB attachment contract, rebuilt native spatial/family control checks and NPC primary-support/fallback separation test. The traversal rules are contributed in [preservation PR72](https://github.com/michidk/redguard-preservation/pull/72). Cross-list duplicate roots and complete NPC collision admission remain open.
- [x] Absolute `RotateToAxis` and elapsed `sRotateToAxis`/`sRotateByAxis` tasks share logical actor slots and local angles. **Verified:** fourteen original-instruction angle sequences and installed CAVERNS fish rotation with concurrent movement through explicit stages and normal ordinary traversal. Arithmetic is published in [preservation PR73](https://github.com/michidk/redguard-preservation/pull/73). The fish's later navigation and full world playback remain incomplete.
- [x] Relative `MoveByAxis` and `sMoveByAxis` tasks update actor-owned local positions with deferred family refresh. **Verified:** [movement increment](https://github.com/michidk/cyrus/commit/73e983a), installed ISLAND mechanism and original rounding/completion comparisons.
- [x] `Rnd` and `QuickRnd` use one game-owned stream retained across world replacement. **Verified:** [random increment](https://github.com/michidk/cyrus/commit/d5a77cb), original result/state comparisons and native VILEDOG draws across world changes. Full consumer order and sky integration remain open.
- [x] Ordinary actor cells use decoded map coordinates and retain authored traversal order. **Verified:** [coordinate fix](https://github.com/michidk/cyrus/commit/3c467ce), installed PALACE selection and a deliberately wrong original-instruction comparison. Deferred MoveObjectAxis cell relocation retains registration order and invalidates distant cached traversal; dynamic allocation remains open.
- [ ] Establish production invocation policy, eligible actors/callers and timing. Do not substitute a guessed fixed rate for original behavior.
- [ ] Finish state-dependent defaults, remaining group types, cancellation/event commands and model replacement for all actors.
- [ ] Complete object routing, variable references, scriptless receivers, allocation/recycling and model-coupled attributes; run all required scripts automatically through the [SOUP checklist](plans/2026-10-12-soup-completion.md).
- [x] Explicit empty-map `Wander` hooks consume shared random candidate draws and restore excessive displacement. **Verified:** [implementation](https://github.com/michidk/cyrus/commit/5c90392), original-function comparisons, installed VILEDOG native hook, 443 workspace tests with zero missing-install skips and WASM check. Automatic invocation remains incomplete.
- [ ] Implement populated walk-map routing/steering, remaining object groups, movement, lights, visibility, NPC perception, hooks and synchronization with animation, speech and sound. Empty-map Wander is not a substitute for navigation or automatic scheduling.
- [ ] Handle script-requested world loads and cleanup; verify actor removal and world replacement with active runtime state.

### Sound and HUD

- [x] Shared native/browser 16-channel effects, animation cues and explicit script flat starts/selective cleanup with receiver/effect ownership. **Verified:** [sound increment](https://github.com/michidk/cyrus/commit/ea836da) includes receiver/result and headless mixer contracts, queued browser cleanup and old-world expiry.
- [x] Installed HUD candle, health, compass, items/counts, power-up slots, logbook, prompts and notices; F8/Tab/F7 preferences persist.
- [x] Parse SURFACE.INI remapping in preservation and set actor SoundSurface from ground/collision.
  Preservation parser and engine rules: [PR #65](https://github.com/michidk/redguard-preservation/pull/65); native/browser animation playback consumes the installed remaps. Logical actor state projects to SoundSurface; supported grounded updates and animation cues retain the emission-time class. Synthetic playback tests cover same-update projection and queued remapping. Automatic scheduling and wider nonplayer ground updates remain with SOUP.
  The native `surface_audio` example records static support, captured animation cues and actual mixer PCM. Its report comparator rejects wrong classes/targets, missing events, ambiguous PCM and absent playback starts. Installed probes cover known zero, sand and rock. **Verified:** an original CYRUS walking call and native mixer agree on sound 9 mapping to effect 103 on rock class 10. Full original runtime audio parity remains unverified.
- [ ] Add positional/ambient starts, StopAllSounds results, shell/code-object ownership, KillMe/Resurrect cleanup, rope thresholds and torch loops; refresh moving-emitter gain/pan.
- [ ] Feed production activation conditions, health, sword/lockout/player type, heading/world and compass state into the HUD. Connect bitmap/logbook script handlers and player walk-mode/auto-defend behavior.
- [ ] Verify native/browser HUD layout, candle cadence, prompt/item timing and compass semantics. **Unknown:** compass frame-zero direction and item-zero compass activation.
- [ ] Compare effects, speech/subtitles and music by ear; establish listener/camera rules, pan handedness, text-only subtitle timing and attached-emitter eligibility.
- [ ] Consider optional Modern suppression of the startup CD burst, retaining current playback in Faithful. **Verified:** installed and exported track bytes preserve the GOG disc-data overlap documented in the [preservation contribution](https://github.com/michidk/redguard-preservation/compare/main...docs/gog-audio-boundary). The user reports the burst in original Redguard and the web build; a paired audio capture remains open. No audio fidelity area or suppression is implemented.

Done when every world renders, animates, sounds and executes its required scripts
against repeatable original-game comparisons. Missing behavior remains a blocker.

## 2. Player controller

Parked at user request. Keyboard input and bounded supplied-tick cores do not
enable live locomotion or the original follow camera.

- [x] Installed keyboard bindings, independent debug flycam, marker placement and static/dry-terrain startup settling.
- [x] Explicit GroundedTick adapter composes ordinary walking/turning, support and wall response with player/render/animation poses. **Verified:** installed ISLAND startup and twelve supplied-tick walking-release updates, plus headless lifecycle/input contracts. Production ticks are not supplied.
- [x] Walking and SOUP share logical-player animation, selected frames and sound ownership. **Verified:** [headless interleaving contracts](https://github.com/michidk/cyrus/commit/21b9b40); combined-tree native startup/world-entry smoke passes; explicit walking/script interleaving still needs rendered verification. Playback defaults have one owner; model-default attribute writes remain unsupported.
- [ ] Establish complete eligibility and simulation timing; compare fractional startup, steady movement, release, walls and ground updates.
- [ ] Implement movement, turning, original follow camera, jumping, gravity and remaining quick actions.
- [ ] Complete Faithful controls and the original follow camera.
- [ ] Add an optional Modern character controller with camera-relative movement, mouse camera, Space jump and held Shift run; keep flycam independent and Faithful as the default.
- [ ] Add native/browser gamepad support for menu navigation and character control, including configurable bindings, analog movement/camera input, deadzones and connection/disconnection handling. Route gameplay input through the selected Faithful or Modern controller.
- [x] HUD takes heading and world ID (compass) from the placed player and loaded world, and health and player type from the player script actor while activation inputs are supplied.
- [ ] Feed sword state to the HUD and consume walk mode and auto-defend in movement. Preservation records the HUD consequences of the sword and the walk-mode key, but not how the sword is drawn, what walk mode changes in movement, or the quick-sword and quick-health actions; recover and publish those rules first.
- [ ] Complete wall/slope/step and animated-geometry collision, subgroup/dynamic admission, orientation refresh, slope sliding and airborne/rollback responses.
- [ ] Match ledge hanging, sideways movement, drops and pull-up clearance.
- [ ] Carry the player on moving/rotating platforms and expose standing/contact state to scripts.
- [ ] Support swimming, climbing, falls and exits; connect player actions to authored scripts/animation/sound and restore control after scripted motion.

Done when recorded input routes match original movement, collision, camera,
interaction and transitions in native and browser builds.

## 3. Interaction and persistence

Later. Requires the player controller.

- [ ] Production interaction and one eligible target, including overlapping pickups and removed objects on revisits.
- [ ] One inventory shared by scripts, HUD, item selection/use and saves; update quantities/icons and compass behavior. The inventory screen must retain its candle rules.
- [ ] Dialogue choices, including disabled choices, synchronized speech/subtitles/animation and results returned to waiting scripts.
- [ ] Scripted cameras and player-control lockout with reliable restoration.
- [ ] World transitions with authored spawn/orientation/artwork and old sound/script/transient cleanup.
- [ ] Pause/resume simulation, scripts, animation and audio; original save-slot menus.
- [ ] Safely persist world/player/NPC/object/quest state, inventory, script threads/waits/tasks/flags/references, and HUD timers/power-up slots/compass in cyrus-config.
- [ ] Coordinate save-game and cheat-file persistence: share storage and safe read/write operations across native/browser hosts, while keeping game snapshots, cheat state and format-specific encoding separate. Original-format readers and writers belong together in `rgpre`; cyrus owns state capture, restoration and storage policy.

Done when a reference route picks up and uses an item, chooses dialogue,
transitions worlds, saves, restarts and resumes with matching state and control.

## 4. Complete gameplay

Later. Requires interaction and persistence.

- [ ] Complete dialogue conditions, quest consequences and item-use rules.
- [ ] Complete NPC navigation/perception, combat, damage, death and restart.
- [ ] Complete fonts, settings and remaining gameplay feedback.
- [ ] Verify interrupted tasks, object attributes and quest/script persistence across content.
- [ ] Verify integrated dialogue, items, combat and transitions with save/resume.

Original-save compatibility requires format evidence before a commitment.

## 5. Full-game fidelity

Later. Requires complete gameplay.

- [ ] Cover all worlds, quests, actors, items, encounters and endings.
- [ ] Test revisits, alternate quest order, failure paths and repeated transitions with active runtime state.
- [ ] Complete a new-game-to-ending playthrough with original-game checkpoints.
- [ ] Resolve every unsupported behavior required to finish the game.

## 6. Release

Later. Requires full-game fidelity; packaging infrastructure already exists.

- [ ] Choose and test supported platforms/hardware and clean setup with a purchased GOG install.
- [ ] Measure loading, frame pacing and bounded memory across repeated transitions; fix observed bottlenecks.
- [x] Audit tagged native packages. **Verified:** all eight archives from the [tagged release run](https://github.com/michidk/cyrus/actions/runs/38272630723) contain only the executable and README. Linux game/viewer help runs in a clean runtime container; the packaged game reaches the menu and enters New Game with a read-only install. Windows and both macOS build/package/help jobs passed on their runners.
- [x] Complete browser packaging and tagged publication. **Verified:** [v0.1.1](https://github.com/michidk/cyrus/releases/tag/v0.1.1) is published with all eight native archives, the browser archive and `SHA256SUMS`. The [release pipeline](https://github.com/michidk/cyrus/actions/runs/38314713622) passed every job; all nine archive checksums match GitHub upload digests.
- [x] Add release-triggered GitHub Pages deployment of the packaged browser build. **Verified:** [workflow implementation](https://github.com/michidk/cyrus/commit/ecd885f). Deployment success remains separate from workflow presence.
- [ ] Fix the artifact-packaging test to stop its extracted release shell block before later workflow jobs, and verify the Pages deployment. **Verified:** the new Pages job exposes this [CI failure](https://github.com/michidk/cyrus/actions/runs/38356786059); local `python3 dev/scripts/test-build-artifacts.py` reproduces it. This is not a SOUP compiler failure.
- [ ] Publish setup, controls and known limitations; verify install, play, save and resume on a clean machine.

Follow [METHOD.md](METHOD.md) for evidence and fidelity, [WORKFLOW.md](WORKFLOW.md)
for ownership/handoffs and AGENTS.md for verification. Formats and recovered
engine facts belong in redguard-preservation. Read the install only; keep local
state and research outside the tracked tree.

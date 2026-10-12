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
| 2. Player controller | Parked at user request | Original movement, camera, collision, and interaction |
| 3. Interaction and persistence | Later | Dialogue UI, inventory, scripted camera control, and save/load |
| 4. Complete gameplay | Later | Dialogue, quests, combat, inventory, menus, and saves |
| 5. Full-game fidelity | Later | A verified playthrough from new game to ending |
| 6. Release | Later | A reproducible install and tested distribution |

“Done” means the listed capability exists in committed code. It does not mean
its appearance or behavior already matches the original.

Checked items are **Done**. Unchecked items are **To do**, or **In progress**
when an active task owns them.
An unchecked item prefixed with **In progress** has an active task claim;
it remains incomplete until verified. The ownership table identifies its agent.
Partial work marks only its current subtask, not the whole capability.

## Who is working on what

Domain sessions resumed on 2026-10-11. The captured implementation milestones
are consolidated; implementation continues against each original brief and later
user clarifications. A research handoff or isolated core does not complete a
requested runtime feature. Reduce repeated verification, not implementation scope.

| Roadmap items | Session | Remaining domain work |
| --- | --- | --- |
| Step 1: actor and mesh animation | Published animation increments; remaining work unassigned | Explicit pose, type-1 transitions and native/browser cue adapter implemented; production clock, actor eligibility and full script scheduling remain open |
| Step 1: SOUP execution and stages 3–4 script integration | Published inspection increments; remaining work unassigned | Portable VM, receiver identities, explicit waits and independent attribute services implemented; automatic scheduling, complete receivers and native services, hooks, synchronization, progression, revisits and saves remain open |
| Step 1: sky | Fix Faithful Mode Skybox, `sky-domain-research` | Capture sun blend/alpha output and filtering across world/menu transitions; integrated visual comparison |
| Step 1: rendering and lighting | Match Faithful Rendering and Lighting, `t3/faithful-rendering-analysis` | Terrain lighting shipped; next: water-edge normal retention and matched native/browser terrain views; model/local-light inputs and runtime shading remain open |
| Step 1: sound | Implement Game Sound Effects, resumed | `SoundCue` backend on main; next: SURFACE.INI remap, channel ownership, script/rope/torch triggers |
| HUD | Implement the Game Overlay UI, resumed | Every documented overlay element is implemented; gameplay must still feed health, sword, lockout, player type, heading/world, compass flag, `Activate`/`ShowBitmap`/logbook events and walk/auto-defend behaviour |
| Startup and book menu | Published increments; fidelity follow-up unassigned | Installed loading, New/temporary Load, settings, Movies and Quit implemented; saves, full gameplay, lighting/clipping, device capture and audible comparisons remain open |
| Steps 3–6 | Unassigned | Interaction, persistence and complete gameplay |

Earlier bounded research milestones remain published, with their limitations
recorded below. They are not feature-completion claims. Obsolete checkouts were
archived; resumed work uses isolated branches and retains the research evidence.
The separate asset-viewer project is outside this game-domain closeout.

The old crate-split branch is an unfinished architectural experiment, not a
replacement for the current implementation. Its history and pending files are
preserved separately; adopting it requires a fresh migration plan.

## Verification still required

These checks are separate from implementation. Passing unit tests does not
establish original-game timing, visual fidelity or audible parity.

- [x] Integrated `just gates`: publication/map/format/strict-Clippy and workspace tests passed with the install present and absent on 2026-10-11; zero installed tests skipped. The updated WASM target passes `cargo check`.
- [x] MCP smoke against the integrated native binary passed, including independent flycam input and an unchanged install. The smoke script used the already-built test-profile executables.
- [x] Compared 499 consecutive captured FLAG2 states through End/restart against the tested integrated library; a wrong asset schema is rejected. This proves supplied-tick state parity only.
- [x] Explicit group/script sound dispatch passes headless runtime contracts with the native backend: pose updates, zero supplied ticks, starts, stops, script restart and multiple invocations in one update. The 16-channel limit and same-update Start/Stop regressions pass. These checks use synthetic audio and do not prove audible fidelity.
  Delivered in [a23c0ca](https://github.com/michidk/cyrus/commit/a23c0ca).
  **Verified:** 138 native workspace tests with zero install skips, strict
  workspace Clippy, WASM compilation, and the same compiled test executables
  with the install link absent. Thirteen focused animation/sound contracts
  and the WASM check also pass after merging the newer main tree. Fresh combined
  native/browser visual and audible playback remain unchecked. The dedicated animation worktree
  and build output can be removed; ignored research is retained centrally for
  the unfinished clock and eligibility work.
- [ ] Verify type-1 mesh transitions, audible native/browser cues, eligibility, host timing and matched rendering. Preservation [PR #32](https://github.com/michidk/redguard-preservation/pull/32) documents static actor/stage eligibility gates; [PR #39](https://github.com/michidk/redguard-preservation/pull/39) documents caller/clock facts. Scale and nominal-tick recurrence match 659 archived transitions given observed FPS; the full BIOS/FPS replay fails at transition 8, so the production host clock remains unresolved. Recorded video frame rate is not simulation-tick evidence.
- [x] The ordered-contact regression retries a first-sphere miss, suppresses successful duplicates, and keeps omitted faces omitted. This tests a caller-supplied candidate list, not gameplay collision.
- [x] Polygon-segment contacts and ground-support selection pass integrated gates and WASM compilation. Four new contracts cover the model fraction scale, crossing/containment, terrain truncation, different rank scales and later-equal replacement. These cores still require caller-supplied admission, transforms and candidate order; live walking is not enabled.
- [x] Schema-2 collision export retains installed static placement order, ROB flags, original polygons and ordered subobject lists in the common native/browser asset. Installed ISLAND export covers 1,691 placements; ambiguous archive names remain explicit errors for future runtime queries.
- [x] Compose the explicitly admitted ordinary walking stages: animation/default requests, forward support and surface state, startup translation, ground-height assignment, ordered wall response and residual separation. Native gates pass with zero installed skips; focused contracts include stationary overlap and invocation ordering. The ordinary idle default reads `attr_panic_group`; special-state default selection remains unsupported. Forward probes use installed bounds/walk distance and nearest-even integer rotation. Eligibility, timing and actor orientation remain caller-supplied. This does not enable live gameplay.
- [x] Export authored player-model radii and resolve static candidates by active model name; missing named radii require re-export. Upright orientation initialization retains original rotation-table residuals. Installed export, ground/walking contracts, strict workspace gates with zero installed skips and WASM checks pass. Incremental orientation refresh and automatic runtime movement remain pending.
- [ ] Compare complete player updates for walking startup, steady movement, release, walls and ground; verify native/browser controls and flycam independence after runtime integration. Preservation PR #27 now retains authored model subsets and documents subgroup admission and static spatial ordering. Its 1,691-placement reconstruction matches 223 original active static nodes, including per-cell order. Segment subgroup admission now has published rules and 16 live decisions in preservation PR #41; interior and threshold cases, dynamic/transient eligibility and v4 extents remain open. Published support ranking retains different terrain/model scales and later-equal replacement; full support integration remains open. The authored-subobject parser now feeds the schema-2 portable collision cache. Static collision now has an explicit supplied-tick player adapter. Automatic runtime admission remains unsupported; ambiguous archive names remain explicit errors when active-world selection cannot resolve them.
- [ ] Verify sky tilt, sun, synchronized scrolling and fog in the integrated Bevy renderer and browser. Retain full original-game comparisons for filtering, alpha, lighting and hardware color precision. Capture filter/blend state after menu and world transitions, translucent sun edges and dither phases. Models still use a fixed Bevy light; terrain now uses recovered per-world lighting. Recover model and local-light inputs before replacing model shading.
- [x] **Verified: terrain lighting implementation.** [Terrain increment `ad6dc04`](https://github.com/michidk/cyrus/commit/ad6dc04), integrated in [main `8391580`](https://github.com/michidk/cyrus/commit/8391580), exports original world ambient/directional inputs and shades private dry/water terrain meshes in the shared native/browser path. [Preservation PR #43](https://github.com/michidk/redguard-preservation/pull/43) publishes the parser, coordinate mapping, quantized normals and texture-combine facts, extending PR #33. Original checks cover 192 arithmetic inputs (reversed light sign fails 88), 65,280 matched heights, 1,024 matching recomputed normals and 1,024 live colors within 0.000022 channel units.
- [x] Terrain contracts cover flat/sloped normals, displaced-water colors, dry terrain, shared-mesh isolation and same-world rebinding. Native ISLAND rendered without shader errors; MCP Lighting mode changes, same-world reload and screenshot passed. WASM compilation passed; integration passed strict workspace Clippy and install/no-install tests. These checks establish arithmetic and runtime wiring, not pixel parity.
- [ ] Compare matched original/native/browser terrain views, including day/night world changes, slopes and water. Re-export browser environments for lighting inputs. **Unknown:** terrain texture filtering, hardware combiner precision and exact dirty water-edge normal retention. Cyrus recomputes the whole grid; an original water update retained two older normals among 1,024 samples. Browser lighting has compile coverage but no rendered comparison.
- [x] Native START screenshot shows the candle above health 100; retained frame handles fix unloading during flicker. The equal-count item icon regression passes.
- [ ] Compare native/browser HUD layout with the original. The candle targets the documented 60 Hz Glide retrace rate independently of rendering; original in-world cadence, transparency and starting inventory still need comparison. Health, pickups and inventory integration remain separate work.
- [x] All eleven installed movie definitions decode through their final video frame with the in-process decoder; their first audio stream decodes when present.
- [x] Rebuild the optimized browser bundle from combined main. Shared-browser checks confirm the ready menu, New world entry, and Introduction playback/subtitles/return; earlier headless checks cover temporary Load, failed-world recovery and refused-tab Quit. Original-game comparisons and player-hardware seek/audio checks remain below.
- [ ] Compare native/browser picture, audio, skip points, subtitles, interlace and menu return against the original. Check cancellation, browser download failure and skip responsiveness on player hardware. Browser seeks decode preceding delta frames cooperatively; keyframe seeking is not implemented. Progression-based movie unlocks remain unavailable.
- [ ] Compare effects, positional gain/pan, speech/subtitle preferences and CD music by ear with the original; verify triggers once implemented. Resolve pan handedness, listener position per camera mode, subtitle font and text-only timing, multitask cadence, rope speed source and attached emitter eligibility. Sound ownership and ambient/rope/torch/surface triggers are documented in preservation PR #31 but still require script and world integration.
- [x] **Verified: startup splash source corrected.** The DOS Glide overlay contains the older shield; the installed Windows `3DfxSpl2.dll` contains the metallic lowercase logo and orange swoosh. The [preservation contribution](https://github.com/michidk/redguard-preservation/compare/main...fix/gog-splash-resource) documents the driver variants, PE resource and rendering rules and supplies the parser. Both installed DLL payloads match; final preservation library/installed-DLL tests and strict Clippy pass. Cyrus renders the authored 90 timed frames plus fade, invalidates shield caches and loads the shared 150-frame sequence. The installed export contract checks the orange logo, initial black frame, dark final fade, cache reuse and missing-frame recovery; its color check rejects the shield reference. Native virtual-display captures show the corrected logo, fade, title and ready book menu after replacing a copied shield cache. CLI and MCP skip checks reach the ready menu without entering gameplay; the install is mounted read-only. Full gates pass on combined main with 171 tests and zero install skips. All 22 compiled test executables also pass with the install explicitly unavailable.

Remaining intro implementation and verification are listed under
[startup intro closeout](#startup-intro-closeout).

- [x] Book/page/cover and direction/key-capture cues dispatch installed effects through the shared native/browser sound path, following [preservation PR #34](https://github.com/michidk/redguard-preservation/pull/34). Native MCP/PCM checks cover navigation, reserved/accepted capture, mute/unmute and authored Quit; the muted interval is silent. The cue contract catches a wrong-effect mutation. Menu-only export includes the effects bank and cue WAVs. Full gates pass with the install present and absent; the browser compile check passes.
- [ ] Exercise startup and animated book transitions in the browser, saved settings after restart and actual browser audio. Recheck native playback on combined main; existing MCP/PCM evidence predates shared-text and menu-only sound export. Compare cues by ear with the original. Recover inherited startup lighting and camera-plane clipping before implementing them. **Verified:** preservation PR #34 and an original-game observation establish that the ordinary book menu has no pointer label selection; retain keyboard/action navigation rather than adding hitboxes. Mouse/joystick rebinding and gameplay snapshots remain unsupported.
- [x] The integrated native F1 inspector opens Entities, Resources and Assets, checked through the live renderer.
- [ ] Wire the HUD inputs. Only F8 (candle mode), Tab (walk mode notice) and F7 (auto-defend notice) work today, persisted in `cyrus-config/hud.json` / localStorage `cyrus-hud`. No system yet calls `HudState::set_health`, `set_sword_drawn`, `activate`/`deactivate`, `show_bitmap`/`unshow_bitmap` or `logbook_entry`, or sets `lockout`, `player_type`, `heading`, `world` or `compass_on`; so the mode-0 show timer, sword-lock item cycling, compass, power-up slots, logbook icon and activation prompts never appear in play. The player does not consume walk mode or auto-defend. The inventory screen (candle pinned at 50), save-game storage of the timers, slots and compass flag, and the unknown player states that suppress prompts remain unsupported.
- [ ] Screenshot-compare every HUD element and notice with the original; compass frame 0 direction, whether receiving item 0 turns the compass on, and the original in-world frame rate are unverified.
- [ ] Repeat inspector/HUD/sky runtime checks in the browser; confirm served browser assets are current and cache reuse/invalidation works.
  **Verified:** the rebuilt preview passes local/public HTML health checks. Shared environment-port checks show the current book menu, installed world entry and HUD without fatal WebGL errors. Headless checks cover settings and world-load recovery. Full authenticated public-URL interaction, inspector/HUD controls and matched sky views still need verification.

## Done

- [x] Rust/Bevy workspace, install discovery, settings, and fidelity switch.
- [x] Startup preload, progress display, and parse-failure logging.
- [x] Installed Windows Glide metallic-logo/orange-swoosh splash and title playback through the book menu, with legacy shield-cache replacement; CLI and MCP skip stop at the menu.
- [x] MENU.INI-defined book pages, keyboard selection, Options edits, Controls rebinding, Movies browsing, and Quit, using installed geometry and rgpre fonts. Settings persist and affect native audio; remaining actions are pending.
- [x] Native and browser Smacker playback with installed skip points, subtitle/interlace preferences and retained menu selection. Browser cinematics load on demand from the local startup export.
- [x] Authored book page turns, submenu camera moves, startup camera entry, and closing Quit sequence; input waits for settling.
- [x] Installed book/menu effects, Sound preference ordering and menu-only effects-bank export through the portable sound backend.
- [x] Native scene loading and headless scene export.
- [x] WORLD.INI map, terrain, and palette selection; numeric IDs and map aliases.
- [x] Per-world terrain lighting, quantized normals and encoded texture modulation in shared native/browser rendering; independent Lighting setting defaults to Faithful, with Modern reusing it.
- [x] Export all 29 installed worlds, with structural checks.
- [x] Browser scene viewer with download progress, persistent GLB/engine caching, and cache-write failure fallback.
- [x] Inspection flycam, TCP control, and MCP tools.
- [x] In-game HUD from installed data: candle, health number and show timer, compass, logbook icon, power-up slots, active item, count and activation prompt, walk-mode/auto-defend notices; F8/Tab/F7 toggles persist locally. Gameplay inputs are not wired (see stages 1–3).
- [x] DOSBox-X runner for observing the original game.
- [x] Repository checks and CI.
- [x] Main-push, release-tag and manual artifact builds for Linux x64, Windows x64, macOS Intel/Apple Silicon, and browsers. The asset-viewer branch uses the same workflow; version tags create draft releases with archives and checksums. Hosted game and viewer matrices passed, with archive contents and native executable architectures checked.

Evidence: [foundation](https://github.com/michidk/cyrus/commit/8d179e4),
[world exports](https://github.com/michidk/cyrus/commit/1df0990),
[browser rendering](https://github.com/michidk/cyrus/commit/f3ff4ea),
[GLB cache](https://github.com/michidk/cyrus/commit/24ee5c4), and
[flycam](https://github.com/michidk/cyrus/commit/ce8ce02),
[game artifacts](https://github.com/michidk/cyrus/actions/runs/38178965225), and
[asset-viewer artifacts](https://github.com/michidk/cyrus/actions/runs/38181794700).

## Resource coverage

Parsing a file does not mean its behavior works in-game. This table covers
committed support; unverified work stays in the TODO column.

| Resource | Already loaded or used | Still TODO |
| --- | --- | --- |
| `WORLD.INI` | World list, map, terrain, palette selection, and terrain ambient/directional lighting inputs | Verify every world against the original; model-lighting inputs |
| `RGM`, `WLD` | Static scene placement; terrain with quantized shading normals and per-world vertex RGB, including displaced water | Exact dirty water-edge normals, model/local lighting, actor behavior and full world playback |
| `3D`, `3DC`, `ROB` | Static export, retained animation frames, explicit source-mapped i32 mesh poses, forward type-0/type-1 request core and literal animation-task playback; authored book frames and runtime menu motion | Automatic actor eligibility and host clock, other frame encodings, model replacement and matched original playback |
| `BSI`, `COL` | Textures and palettes; documented terrain waves in both graphics modes | Other animated textures and visual comparison with the original |
| `GXA` | Startup title artwork, book checkboxes/sliders, movie previews, HUD candle, compass, logbook, power-up and item artwork, and preload parsing | Software sky projection |
| Glide splash DLL | Installed Windows metallic logo and swoosh, authored camera, textures and fade rendered into a local startup cache; legacy shield cache invalidated | Rebuilt browser playback; original-driver timing/pixel comparison; exact hardware edge coverage, floating-point approximations and pixel quantization; upstream contribution merge |
| Glide sky BSI/COL | Faithful tilted sky plane, sun, fog and synchronized scroll inputs; camera-centered Modern sky | Full original-game projection/motion, filtering, alpha and lighting comparison |
| `SFX` | Every effect decoded at its stored format; native/browser flat and explicit animation cues share 16 channels, loop flags, volume/rate clamps, positional gains and per-actor Stop/Forget; native `play_sound` remains available | Automatic world/script/ambient triggers, nonzero surface remapping, channel ownership, moving-emitter refresh and audible comparison with the original |
| `RTX` | All voice clips load in the background; playback by label via `play_speech` with dialogue subtitles; voice and subtitle preferences honoured | Dialogue and combat triggers, Cyrus' subtitle colour, dialogue choices, and quest integration |
| `FNT` | Startup labels, book-menu glyphs, movie and dialogue subtitles, HUD numbers, prompts and notices, and status font; one shared layout/raster crate (`cyrus-text`) | Remaining prompts and menus |
| `CHT`, `PVO` | Preload parsing | Runtime use and behavior verification |
| SOUP scripts | Versioned installed definitions and logical actors retained; one portable core executes control flow, values and shared task slots through explicit hosts; supplied-tick animation playback and MCP inspection use that core; explicit frame-scale task passes support researched wait commands; explicit routed calls use receiver locals and task slots; standalone player/camera instances and signed namespace ordering are retained | Automatic actor scheduling and timing, complete object routing, other native bindings, hooks, synchronization, progression and saves |
| `MENU.INI` | Main, Options, Display, Sound, Controls, and Movies definitions, navigation, widgets, action IDs, New Game and temporary fresh-game Load | Save slots, Continue/pause, and progression unlocks |
| `KEYS.INI` | Menu activation, installed key names/capture prompt, fifteen saved bindings, and held player-input foundation | Gameplay locomotion and remaining quick-action handlers |
| `COMBAT.INI`, `ITEM.INI` | Starting-item HUD icons and counts from ITEM.INI | Combat, item use, pickups and shared inventory |
| `SMK` | Native and browser video via the in-process Smacker decoder, first embedded audio stream, installed skip points and subtitle definitions; native CD music pauses during playback | Audible output, A/V synchronization and original-game comparison |
| CD audio | World tracks and the book menu's track 2 play from GOG's CD image and repeat; the Music slider and `redbook_volume` set the drive level | Credits and ending music, audible comparison with the original |
| Save data | No save/load support accepted | Persistent world, player, script, and quest state |

**Verified:** Glide sky textures, their independent COL palettes, and the static
plane mapping are documented in preservation's
[sky renderer notes](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/sky.md).
Installed day, night, sunset, and Necro exports pass palette and plane checks;
native Vulkan and browser WebGL runs show the static sky behind scene geometry.
The former GXA cubemap and approximate water animation are removed.
**Verified:** uncovered pixels use the Glide `world_fogrgb` clear color. Re-export
old environment sidecars to replace legacy palette backgrounds. Sky
tilt and the sun disc are implemented; sky playback does not
establish complete original-game fidelity. Scrolling follows per-world speed
and BIOS ticks; original-session random direction and above-plane camera
behavior have not been compared.

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
Options, Display, Sound, Controls, and Movies browsing now render installed
settled pages, including neighboring-page text, widgets, key names, and movie
previews. Native MCP checks cover page navigation, bounded edits, reserved keys,
capture cancellation, conflict clearing, the edited primary activation key,
restart persistence, malformed state/cache protection, failed saves, and direct
world entry.
Installed export tests cover changed labels, forced movie visibility, configured
prompt limits, the Glide Display omissions, and all authored book frames. Native
MCP checks cover every supported page route, input gating, reversible turns and
animated Quit. The settled Movies image matches the previous renderer exactly;
Main restores identical pixels after returning. Intermediate page-curl geometry
was inspected. No matched original animation/timing claim is made. Lighting,
camera-plane clipping, mouse/joystick rebinding, gameplay snapshots and saves
remain unsupported. New Game and temporary Load now enter the installed starting
world; complete story/gameplay initialization remains pending. Native MCP checks cover all five browsable movies, forward skips,
held-key handling, natural EOF and unsupported-path recovery in the prior decoder,
Quit during playback, and decoder cleanup on world entry. Video and subtitle
screenshots were inspected; the first audio stream decodes to a seekable WAV.
The combined native build also passes Introduction activation, advancing frames,
all three installed skips, retained selection and replay through the control port.
Audible output and A/V synchronization remain unchecked. Subtitle/interlace
preferences apply to cinematics; saved Sound preferences apply to native effects,
speech, and CD music.
Headless browser checks at 640×480 cover startup/menu navigation, Movies
activation, changing Introduction footage, subtitles, forward skips and return
to the same selected entry and replay. Browser audio and matched original timing remain
unchecked. Full workspace gates pass with the install present and absent, with
zero installed-test skips. Verification artifacts stay in the primary checkout
under `magic/artifacts/2026-10-11-movies/`; task build outputs and runtime
containers are cleaned up after publication, while shared caches and the existing
preview remain. The native/headless test processes are stopped and their small
evidence archive is retained; worktree removal includes its target and export cache.

**Verified:** native startup prepares the menu and HUD behind the installed title;
New and the temporary fresh-game Load enter the configured starting world and
marker behind installed transition artwork. The barrier waits for sidecars,
recursive asset dependencies and spawned scene instances. Native checks cover
persistent edits, malformed preferences, failed-world Escape/retry, movie playback
and authored Quit. Browser bootstrap downloads the engine first; world requests
begin after menu activation. Headless browser checks cover world entry, loading
failure/Escape/retry and refused-tab Quit. Shared-browser captures confirm Introduction playback, subtitles and return
to the selected Movies entry. WebGL terrain preserves affine vertex color without
requiring unsupported interpolation qualifiers. Exact original loading labels,
stage weights, pixels, timing and audible parity remain unverified.

**Verified:** book turns, cover crossings and direction/key-capture feedback emit
installed effects through the portable `SoundCue::Flat` path. Sound preference
edits apply before cues in the same update. Menu export writes the effects bank
before a book-cache early return, supporting browser startup without a world.
The installed export test checks all cue WAVs and their header gain. Native
MCP/PCM evidence establishes audible navigation and capture feedback plus
mute/unmute; it predates the later shared-text/export integration. Full workspace
gates and the browser compile check pass. Actual browser audio and original
audible parity remain unchecked. **Verified:** the original default menu has no
pointer hover/click label selection; mouse/joystick rebinding remains unsupported.

**Verified:** all 118 `MAIN.SFX` effects decode through Bevy's WAV decoder
with their stored sample rate, channel count and sample count, all 3933 RTX
voice clips load, and CD track 2 reads as 44.1 kHz stereo from the GOG image
(install-backed tests). World and menu music follow preservation
`docs/engine/music.md`; effect and voice volume, distance falloff and Sound
page sliders follow `docs/engine/sound.md` and `docs/engine/menu.md`, with
tests for the documented curves and slider table. **Verified:** a dialogue
subtitle draws the same glyph extent (x 73–565, y 423–439) as the original's
capture of the same line; subtitle facts are
[published for preservation review](https://github.com/michidk/redguard-preservation/pull/25).
Loudness and stereo pan follow the original's Miles mixer: gain `(v + 1) / 128`
and its pan table ([PR 29](https://github.com/michidk/redguard-preservation/pull/29)),
tested on the documented vectors; which speaker is "right" follows the Miles
convention and is not yet confirmed by a stereo capture. Nothing in the world
triggers effects or speech yet. Nothing has been compared by ear with the original.
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
- [ ] **In progress** Add the debug world dropdown and inspector toggle.
- [x] `just smoke-mcp` drives the headless game through MCP; movement and the full loop verified in [c713082](https://github.com/michidk/cyrus/commit/c713082).
- [x] Native game loads a scene under the virtual display; runtime image and asset-root fixes landed in [9535ce6](https://github.com/michidk/cyrus/commit/9535ce6).
- [x] `just compare <label>` captures both games into `cyrus-config/compare/` ([9535ce6](https://github.com/michidk/cyrus/commit/9535ce6)).
- [ ] Capture matching scenes and viewpoints with that command. The verified pair showed cyrus START and the original load-game book, not a matched comparison.
- [x] The fidelity switch gates graphics; browser sky/water toggling verified in [4a7696d](https://github.com/michidk/cyrus/commit/4a7696d). This proves switching only; unsupported rendering approximations are being removed.
- [x] Split assets, world execution and presentation out of `cyrus-game`; keep TCP transport in `cyrus-control` and platform adapters in the hosts. See [crate ownership](#crate-ownership).
- [x] CI green since the repository restructure ([run 38053971267](https://github.com/michidk/cyrus/actions/runs/38053971267), revision `92aac25`). This run does not cover pending shared changes.

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

### Startup intro closeout

**Done:** the corrected native startup is on main in
[35b980d](https://github.com/michidk/cyrus/commit/35b980d).
The remaining work is unassigned.

- [x] **Verified:** identify the driver-specific splash variants and publish the Windows DLL parser and rendering facts in the [preservation contribution](https://github.com/michidk/redguard-preservation/compare/main...fix/gog-splash-resource).
- [x] **Verified:** select the installed Windows splash, render its authored frames and fade, replace legacy shield caches, and recover missing cached frames in the shared native/browser export path.
- [x] **Verified:** native playback reaches the title and ready book menu; CLI and MCP skip reach the menu. Full gates pass with 171 tests and zero install skips; all 22 compiled test executables also pass without the install.
- [ ] **To do:** merge the preservation contribution upstream and update the rgpre pin to the accepted revision.
- [ ] **To do:** rebuild and re-export browser startup; verify the corrected logo, title, ready menu and both skip routes in the browser.
- [ ] **To do:** compare Windows-driver splash timing and pixels with the original, including hardware edge coverage, reciprocal-square-root approximations and pixel quantization.
- [x] Set an explicit larger stack on native startup and scene preparation workers; installed parsers no longer depend on the launcher's `RUST_MIN_STACK`.
- [ ] Replace the generic startup `Loading...` label with the original map-registration text when that registration stage exists. World-entry progress counts completed cyrus preparation/asset groups; original internal stage weights are not reproduced.
- [ ] Implement original save slots, thumbnails, overwrite/delete handling and safe persistence. Replace the temporary fresh-game Load handler only after saves exist; do not expand startup/menu work into the save system.
- [ ] Implement Continue and pause/resume once a resumable world exists, and full New Game initialization through authored story/scripts and gameplay runtime.
- [ ] Complete progression movie unlocks, menu lighting/camera clipping, mouse/joystick capture and gameplay snapshots, with original-game evidence.
- [ ] Add browser CD music and compare audible effects/cinematics and A/V synchronization on player hardware.
- [ ] **To do:** verify optimized native release startup and skip playback.
- [ ] **To do:** check startup audio on a machine with an audio device and the installed CD image; the native virtual-display check did not verify audio.

## Crate ownership

The split keeps dependencies one-way: presentation consumes gameplay and world
state; world execution consumes portable assets. `rgpre` remains the owner of
formats and recovered engine facts.

| Crate | Responsibility |
| --- | --- |
| `cyrus-assets` | Validated portable cache records and loaders. The native feature adds read-only install discovery, parsing, export and media reads. |
| `cyrus-world` | World requests and committed metadata, explicit animation execution, sound intents, terrain and static collision queries. |
| `cyrus-game` | App state, fidelity, player input and deterministic rules, player runtime sidecars, and the preference storage contract. |
| `cyrus-render` | Shared native/browser world presentation, mesh animation, environment, inspection camera, startup/menu, HUD and media playback. |
| `cyrus-control` | Existing protocol/client and a native std-only TCP server module. Command application belongs to the native host. |
| `cyrus`, `cyrus-web` | Platform composition, preferences, binding initialization and world preparation. |
| `cyrus-debug` | Inspection UI through public APIs and host-supplied world choices. |

Native media tasks remain with presentation so extraction preserves cancellation
and scheduling. Their file and parser access goes through `cyrus-assets`.
Headless assembly omits camera and audio backends. Rendering-only control
requests report their unavailable state; input does not enable unsupported movement.

`dev/scripts/check-core-boundaries.sh` builds an isolated consumer and rejects
renderer, window, audio, install-parser and transport dependencies in portable
assets/world/game. CI also checks the browser target. Contract tests belong to
the crate that owns the behavior, including world replacement, explicit ticks,
preference failure handling and TCP framing.

Verification covers the isolated portable dependency graph, native MCP
state/input/fidelity/quit, rendered START/PALACE loading, invalid-world retention
and camera/player independence. Browser release compilation is checked separately
from live browser behavior. Matched original-game comparisons, audio parity and
browser interaction checks remain required; extraction does not establish them.

The split does not provide automatic animation eligibility, a simulation clock,
locomotion, script execution, combat or saves. Build-time gains and fidelity
improvements are not claimed. See the
[implementation plan](plans/2026-10-11-gitnexus-plan-game-crate-split.md)
for the ownership decisions and migration constraints.

## 1. Complete world playback

**In progress.** Static loading works. Complete playback is still open.

- [x] Load and export worlds with their terrain, textures, and palettes.
- [ ] Compare placement, scale, lighting, sky, transparency, and visibility with the original.
- [x] Apply faithful ambient/directional terrain lighting to dry and displaced-water meshes, with screen-linear vertex RGB and encoded texture modulation before fog. Lighting is independent of Water/Fog/Sky; Modern lighting is only a placeholder reusing Faithful.
- [ ] Resolve original water-edge dirty-normal admission and retention, then replace full-grid recomputation where required. Keep the established coordinate and arithmetic contracts.
- [ ] Recover model normal/coordinate transforms, material shading rules and local-light selection/attenuation inputs in preservation; wire supported paths into native/browser model rendering. Fixed Bevy model lighting is still a placeholder.
- [ ] Validate terrain filtering and combiner precision against original draws, then run matched native/browser comparisons. Do not infer pixel parity from arithmetic tests or shader smoke; modern lighting remains outside the current implementation scope.
- [x] Faithful sky plane, tilt, sun and synchronized scrolling; original fog/range defaults; independent Water, Fog and Sky fidelity buttons and F1 render-distance override.
- [x] Modern presentation keeps installed artwork: camera-centered sky, extended distance with distant fog, and smoother water waves, texture ripples and sheen. These are optional artistic effects, not original-game parity claims.
- [ ] **In progress: sky filtering verification.** Sun bilinear selection is published in [preservation PR #40](https://github.com/michidk/redguard-preservation/pull/40). Initial START cloud filtering state and software Voodoo interpolation are published in [preservation’s sky documentation](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/sky.md). The Faithful cloud correction is implemented and included in the preview. **Verified:** isolated GPU output checks cover interpolation, wrapping and sRGB conversion, including a deliberately incorrect variant that fails; the browser release build passes. Full integration gates and rendered comparison remain open.
- [ ] Capture the original sun's active blend state and translucent edge output, then implement any warranted alpha-rounding correction. **Unknown:** exact sun-edge parity; entering the sun routine without a visible draw does not establish it.
- [ ] Check sky/sun filtering after menu and world transitions, plus other texture filtering and physical-hardware color precision. The initial START observation does not establish every draw state.
- [ ] Compare native and browser sky tilt, scroll direction/phase, sun motion, above-plane flycam behavior, fog and clipping against the same original-game view. Isolated shader checks cover plane crossing, Modern camera-translation invariance and fog/dither arithmetic; they do not establish integrated visual parity.
- [ ] Verify Water/Fog/Sky switching and F1 render-distance editing/reset in the current integrated browser build. Retain original defaults in Faithful and confirm the Modern sky stays camera-centered during translation.
- [x] Retain actor animation metadata, original section payloads and model frames in native/WASM assets. **Verified:** all 29 installed worlds export without unresolved animation models; native asset tests and WASM compilation pass.
- [x] Apply explicit i32 model poses and forward groups using supplied simulation ticks. **Verified:** 17 native animation contracts, installed FLAG2 GLB deformation/restoration, and WASM compilation.
- [x] Implement forward type-0/type-1 requests, numeric priority, pending/default transitions, forced replacement, finite task loop counts, delay precedence and type-1 exit tails. Public-interface contracts and the 13-update original walking-exit comparison cover core identifiers, groups and markers.
- [x] Connect literal `PushAnimation`/`End` programs to explicit mesh playback: preserve request/start/wait phases, same-stage continuation, End restart, synchronous sound handlers and sticky contextual failures. Installed FLAG2 contracts cover source mapping and mesh restoration; 499 captured states match supplied-tick core output, and a wrong schema is rejected.
- [x] Dispatch authored animation sound markers through the native/browser adapter during supplied-tick updates. Thirteen focused animation/sound contracts pass, including shared-channel capacity and queued Start/Stop. Audible original parity remains unverified.
- [ ] Play actor and mesh-frame animations, including loops, exit sequences, queued transitions, and animation event cues.
  - [ ] Resolve the original clock entry and transition-8 discrepancy, then wire a supported production tick source. Supplied ticks remain explicit; no fixed-rate substitute is authorized.
  - [ ] Integrate actor activation and spatial/stage eligibility, including the selected-player caller; preserve separate invocations in the same update.
  - [ ] Implement evidence-backed state-dependent defaults, remaining group types, cancellation and authored event commands.
  - [x] Support explicitly supplied reconstructed i16 poses; installed CYRSA001 frames verify export and projection. Old raw-delta caches require re-export.
  - [ ] Support model replacement across frame sources for all actors. Explicit player pose binding now loads and replaces textured source models; automatic player activation remains pending.
  - [ ] Verify world replacement and actor removal with active mesh, script and sound state.
  - [ ] Compare an actual rendered and audible authored sequence in both games, then repeat in the browser and across required worlds.

  **Verified provenance:** frame/countdown/walking-marker facts are published in
  preservation `8c3856e`, `9b4206b` and `92b9933`; request/default rules in
  `84cc21d`; initialization/spatial membership/forced requests in `d1e8e46`
  and corrected view-centered activation in `64eca10`; ordinary/script stage
  ordering in `ea35fca`; task-start and End continuation in `a1ffba6`.
  Delay, type-1 and sound contracts are documented in
  [PR #30](https://github.com/michidk/redguard-preservation/pull/30), with
  caller/eligibility evidence in PRs #32 and #39. These facts and core comparisons
  establish supported increments, not automatic world playback or full fidelity.
- [x] Play documented terrain water waves in native and browser builds.
- [ ] Compare water with the original; play other animated textures and effects.
- [ ] Play positional and looping effects, ambient sound, speech, and music with original timing and volume behavior.
  - [x] Portable `SoundCue` (flat and animation cues) plays through 16 shared channels on native and browser, with original start-index, volume and rate clamps, per-actor last-sound Stop/Forget, `SYSTEM.INI sound=0`, and browser cues retained until the effects bank loads.
  - [x] Deferred sound spawns retain their channel reservation within a cue burst; the existing 16-channel limit contract and all nine sound tests pass after the integration fix in `f2f7a12`.
  - [x] Explicit animation playback emits authored sound markers through `SoundCue`; same-update starts/stops and the 16-channel limit pass runtime contracts.
  - [x] Startup book and menu feedback cues emit `SoundCue`; native MCP/PCM, cue contracts and menu-only bank export are verified. Browser audio remains unchecked.
  - [ ] Parse SURFACE.INI's sound remap in rgpre and remap animation sounds by surface; animation sounds currently play only on surface 0.
  - [ ] Ground/collision sets each actor's `SoundSurface`.
  - [ ] Record each channel's owner and requested effect for `EndSound`, `EndMySound(s)` and `KillMe`.
  - [ ] Script sound calls (`Sound`, `FlatSound`, `AmbientSound`, `AmbientRTX`, `EndSound`, `EndMySound(s)`, `StopAllSounds`, `KillMe`/`Resurrect`) once the script engine exposes them.
  - [ ] Rope sounds from MPRP thresholds and the torch loop (effect 25), once rope and torch objects exist at runtime.
  - [ ] Refresh positional volume and pan while a sound plays.
- [x] **Verified:** portable SOUP cache and execution core, independent actor locals, shared flags, six task slots, pause/resume/stop, animation migration and bounded MCP stepping. Explicit hosts support researched flag helpers, attribute reads/writes and wait commands through explicit frame-scale task passes. Attribute-44 model coupling remains unsupported. Explicit routing supports Me, standalone player/camera receivers and names ordered by signed runtime identifiers, with receiver locals, slots and foreign operands. Variable references, scriptless receivers and later allocation/recycling remain unsupported; this does not enable ordinary gameplay.
- [ ] Run SOUP automatically with actor eligibility/timing, object references and receiver-owned tasks, main/interrupt execution, waits, hooks and synchronization.
- [ ] Activate every required object script; report unsupported operations rather than silently excluding objects.
- [ ] Support scripted object groups, attachments, movement, lights, visibility, and world-required NPC navigation and perception.
- [ ] Synchronize scripts, animation events, speech/subtitles, and sound; resume waiting scripts at the correct time.
- [ ] HUD script inputs: `Activate`/`TorchActivate`/`Deactivate` prompts, `ShowBitmap`/`UnShowBitmap` power-up slots, `LockoutPlayer` lockout and logbook entries feed `HudState`. **To do:** no caller yet; catalogue which shipped scripts call them.
- [ ] Execute script-requested world loads and reinitialize playback state; player-facing transition UI follows in step 3.
- [ ] Carry the required playback data into both native and browser world views.
- [ ] Verify every world and palette variant with repeatable timed comparisons.

**Verified:** preservation published corrected twenty-bit ShowFrame references
and exported source vertex/face mapping in
[`8c3856e`](https://github.com/michidk/redguard-preservation/commit/8c3856e),
and forward frame-countdown semantics in
[`9b4206b`](https://github.com/michidk/redguard-preservation/commit/9b4206b).
Explicit pose, group and literal task playback are committed. Automatic actor
activation, host timing and full playback fidelity remain unsupported.

**Done when:** every world plays its required visuals, animations, sound, and
scripts correctly against the original. Missing behavior remains a blocker.
Use the inspection camera and controlled events until the player controller exists.

**Next tasks:**

1. Capture the same scene in cyrus and the original.
2. Run and review the preload classifier, including unsupported and failed data.
3. Choose reference sequences that exercise animation, water, sound, and scripts.
4. Verify reference audio now that the retro runner enables it.
5. Inventory runtime data and SOUP bindings; record operations actually called by the reference sequences and unsupported paths.
6. Resolve the animation clock discrepancy and eligible callers, then connect production playback and compare mesh/audio outcomes. Coordinate surface identity with player collision and SURFACE.INI parsing with the sound owner.

## 2. Player controller

**Parked at user request. The controller is incomplete; the remaining acceptance scope is listed below.**

Published prerequisites:

- Marker-array decoding corrected in preservation ([c7aea52](https://github.com/michidk/redguard-preservation/commit/c7aea52)); validated against 215 markers in 27 installed maps, with parser regression tests.
- Player coordinate units and ordinary ground-turn arithmetic documented ([1058051](https://github.com/michidk/redguard-preservation/commit/1058051)). Original walking and turning coordinates are captured, including the right-turn heading sign. These observations do not establish controller parity.

FPS-derived frame-scale smoothing is now documented and checked against a native
original-game recording ([16703d1](https://github.com/michidk/redguard-preservation/commit/16703d1)).
Forward-walking startup advances while animation can suppress translation.
Live original-game observations confirm the first displacement can use a later
startup phase ([e839ed8](https://github.com/michidk/redguard-preservation/commit/e839ed8)).
The recording lacks the settled fractional starting position, so its two-unit
endpoint difference is not an established mismatch. Ordinary release resets
startup after an update without accepted forward movement; marker entry and
ground settling are separate ([fad0b00](https://github.com/michidk/redguard-preservation/commit/fad0b00)).
A live player trace verifies the forward obstacle, slope, and terrain-support
checks ([7d1fe5d](https://github.com/michidk/redguard-preservation/commit/7d1fe5d)).
Actor bounds layout is corrected and checked across 806 installed records
([9e93d4e](https://github.com/michidk/redguard-preservation/commit/9e93d4e));
its parser correction is published in [29ac86b](https://github.com/michidk/redguard-preservation/commit/29ac86b).
Upright collision spheres and the FPS measurement window are documented in
[cd65049](https://github.com/michidk/redguard-preservation/commit/cd65049).
Terrain support sampling and surface-to-player height conversion are documented
in [054369b](https://github.com/michidk/redguard-preservation/commit/054369b) and
[1d68050](https://github.com/michidk/redguard-preservation/commit/1d68050).
Original flat and sloped contact samples match the documented heights and normals;
65,280 captured terrain height entries match installed data. An original
four-stop walking trace verifies the subsequent ordinary height assignment while
preserving X/Z and the previous position
([d7de505](https://github.com/michidk/redguard-preservation/commit/d7de505)).
Obstacle rollback, airborne responses and complete movement eligibility remain
unverified.
Automatic animation admission and complete collision integration remain unresolved;
keyboard-driven locomotion is not enabled.

Input foundation: eight held directional/action bindings from
`KEYS.INI`, Modern WASD/Space/held-Shift input, an independent debug flycam
toggle, and input/camera-isolation tests. A deterministic ground-turn core
matches captured original heading changes, including a frame-scale change and
release. It is not connected to live gameplay while timing and state eligibility
remain unresolved. Workspace tests, strict Clippy, formatting, publication checks,
and an MCP smoke test pass using isolated worktree artifacts. Player tests cover
installed bindings, Modern input, camera isolation, ground-turn arithmetic, and
animation-gated walking startup and release reset. The walking core supports step
type 1 and rejects other step types. The explicit supplied-tick adapter connects
supported translation to player pose,
render transform and animation pose. Automatic gameplay translation and the original
follow camera remain unsupported.

Initial marker placement uses exact exported map coordinates and retains the
requested heading modulo 2048. A headless ISLAND load matches a fresh original
marker-zero start-position query; debug flycam input leaves the player unchanged.
Missing and unavailable markers remain explicit errors. Native workspace tests,
strict Clippy, WASM compilation and MCP smoke passed for that foundation.
Scene entry now also waits for matching player inputs and settles against dry
terrain/static geometry. The installed ISLAND result matches the captured original
startup height. Focused contracts cover strict endpoint admission, truncation,
startup rank scales and earlier-equal retention. Dynamic/transient support,
rendered heading and automatic locomotion remain incomplete.

**Verified:** `PlayerPlugin` accepts explicit `GroundedTick` messages in the shared
native/browser path. It preserves separate animation invocations, selects collision
candidates after ordinary animation, retains walking state, and emits authored sound
cues. Headless ECS contracts cover multiple updates, zero ticks, stale entries,
errors and world re-entry. The installed ISLAND comparison covers twelve supplied-tick
walking-release updates, including movement suppression and return to idle. This
is a bounded static-ground comparison, not full controller or host-cadence parity.
Strict workspace Clippy, installed workspace tests with zero skips, formatting,
publication checks and WASM compilation pass for this increment.
No production system supplies these messages yet. Native/browser visual movement,
automatic timing/admission, full collision eligibility and both complete control
modes remain unchecked or unimplemented. Research and comparison artifacts remain
ignored and recoverable after the dedicated player worktree is removed.

- [x] Land and verify the keyboard-input and independent debug-flycam foundation.
- [x] Apply marker placement and static/dry-terrain startup settling; scripted spawn selection and dynamic support remain pending.
- [ ] Implement quick actions and the walk-mode toggle, alongside the held bindings. The HUD Tab/F7 toggles and notices exist; movement must consume `HudState::walk_mode` and `auto_defend`.
- [ ] Feed the HUD from the player: `set_health`, `set_sword_drawn` (show timer, sword-lock item cycling), `attr_player_type`, heading and world ID (compass).
- [ ] Establish a repeatable simulation-timing oracle and animation movement gates; compare the same walking/turning sequence in both games.
- [ ] Reproduce movement, turning, camera behavior, jumping, and gravity.
- [ ] Add Modern controls: camera-relative WASD, mouse camera, Space jump, and held Shift to run. Keep the original controls in Faithful mode and the debug flycam independent of both modes.
- [ ] Match collision on walls, slopes, steps, and animated geometry; reproduce slope sliding.
- [ ] Match ledge hanging, sideways movement, dropping, and pull-up clearance.
- [ ] Carry the player on moving/rotating platforms and expose standing/contact state to scripts.
- [ ] Support swimming, climbing, falls, and scene exits where the original does.
- [ ] Connect player actions to world scripts, animations, and sound; hand control to scripted animations and restore it afterward.

The independent debug flycam leaves the player in place. Player tests verify
binding semantics, input suppression and camera switching; the MCP smoke test
verifies camera movement without player movement. These checks do not establish
locomotion or original follow-camera parity.

**Done when:** recorded input routes match the original and interact with the
world correctly, including collision and scene transitions.

## 3. Interaction and persistence

**Later. Requires the player controller.**

These are concrete gaps found in RGUnity's source, not a goal to copy RGUnity.
Implement their original-game behavior, including the original UI.

- [ ] Interaction prompts and one eligible target at a time, including overlapping pickups; collected objects stay removed on revisits.
- [ ] One inventory shared by scripts, quantities/icons, active-item selection, item-use rules, and saves. The HUD still reads the exported starting items: selection must follow count changes, using item 0 toggles the compass flag, and the inventory screen pins the candle (show timer 50, 0 on exit).
- [ ] Save and restore the HUD show timer, power-up slots and compass flag.
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

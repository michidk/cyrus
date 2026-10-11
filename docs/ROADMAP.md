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

Checklist items show three statuses: **Done**, **In progress**, and **To do**.
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
| Step 1: actor and mesh animation | Load All World Animations Correctly, `animation-playback` | Native cue wiring, actor eligibility, clock, and full script scheduling; type-1 core integrated |
| Step 2: player | Investigate Player Controller Design, `player-parity` | Installed collision model/list export and runtime queries; admission, dynamic eligibility, v4 extents, grounded movement, timing and follow camera |
| Step 1: sky | Prior sky snapshot integrated; remaining work unassigned | Runtime filtering state and sun alpha rounding |
| Step 1: terrain lighting | Match Faithful Rendering and Lighting, `t3/faithful-rendering-analysis` | Verify coordinates/live inputs, then implement the user-resumed terrain-only scope |
| Step 1: sound | Implement Game Sound Effects, resumed | Script, animation and ambient triggers; per-call volume/rate evidence |
| HUD | Implement the Game Overlay UI, resumed | Current user request: shared text rendering and documentation cleanup; overlay timers, sword lock, compass, power-ups, logbook, prompts and F8 remain incomplete |
| Startup and book menu | Match Startup Screen Appearance, `t3/faithful-startup-sequence` | Mouse interaction research first, then published book-cue integration; lighting remains unresolved |
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
- [ ] Verify type-1 mesh transitions, native cues, eligibility, host timing and matched rendering. The five new transition/sound contracts pass. Preservation PR #32 documents static actor/stage eligibility gates. Scale and nominal-tick recurrence match 659 archived transitions given observed FPS; the full BIOS/FPS replay fails at transition 8, so host timing remains unresolved.
- [x] The ordered-contact regression retries a first-sphere miss, suppresses successful duplicates, and keeps omitted faces omitted. This tests a caller-supplied candidate list, not gameplay collision.
- [x] Polygon-segment contacts and ground-support selection pass integrated gates and WASM compilation. Four new contracts cover the model fraction scale, crossing/containment, terrain truncation, different rank scales and later-equal replacement. These cores still require caller-supplied admission, transforms and candidate order; live walking is not enabled.
- [ ] Compare complete player updates for walking startup, steady movement, release, walls and ground; verify native/browser controls and flycam independence after runtime integration. Preservation PR #27 now retains authored model subsets and documents subgroup admission and static spatial ordering. Its 1,691-placement reconstruction matches 223 original active static nodes, including per-cell order; segment subgroup admission, dynamic/transient eligibility and v4 extents remain open. Published support ranking retains different terrain/model scales and later-equal replacement; full support integration remains open. The newer preservation parser is not yet wired into Cyrus collision export.
- [ ] Verify sky tilt, sun, synchronized scrolling and fog in the integrated Bevy renderer and browser. Retain full original-game comparisons for filtering, alpha, lighting and hardware color precision. Capture filter/blend state after menu and world transitions, translucent sun edges and dither phases. World lighting still uses a fixed light; recover terrain/model lighting rules before replacing it.
- **Verified, bounded terrain research:** [preservation PR #33](https://github.com/michidk/redguard-preservation/pull/33) documents the Glide terrain ambient/directional RGB contract. Original-machine-code checks cover 192 inputs; a reversed light sign fails 88 cases. **Unknown:** original-to-export coordinate mapping and live terrain input/output agreement. No terrain shader change or visual parity claim follows from this arithmetic check.
- [x] Native START screenshot shows the candle above health 100; retained frame handles fix unloading during flicker. The equal-count item icon regression passes.
- [ ] Compare native/browser HUD layout with the original. The candle targets the documented 60 Hz Glide retrace rate independently of rendering; original in-world cadence, transparency and starting inventory still need comparison. Health, pickups and inventory integration remain separate work.
- [x] All eleven installed movie definitions decode through their final video frame with the in-process decoder; their first audio stream decodes when present.
- [ ] Compare picture, audio, skip points, subtitles, interlace and menu return against the original with the new decoder. Check cancellation and skip responsiveness.
- [ ] Compare effects, positional gain/pan, speech/subtitle preferences and CD music by ear with the original; verify triggers once implemented. Resolve pan handedness, listener position per camera mode, subtitle font and text-only timing, multitask cadence, rope speed source and attached emitter eligibility. Sound ownership and ambient/rope/torch/surface triggers are documented in preservation PR #31 but still require script and world integration.
- [ ] Exercise startup and animated book transitions in the browser, saved settings after restart, and native audible playback. Verify lighting, book cues and mouse selection after implementation.
- [x] The integrated native F1 inspector opens Entities, Resources and Assets, checked through the live renderer.
- [ ] Repeat inspector/HUD/sky runtime checks in the browser; confirm served browser assets are current and cache reuse/invalidation works.

## Done

- [x] Rust/Bevy workspace, install discovery, settings, and fidelity switch.
- [x] Startup preload, progress display, and parse-failure logging.
- [x] Installed 3dfx splash and title playback through the book menu; CLI and MCP skip stop at the menu.
- [x] MENU.INI-defined book pages, keyboard selection, Options edits, Controls rebinding, Movies browsing, and Quit, using installed geometry and rgpre fonts. Settings persist and affect native audio; remaining actions are pending.
- [x] Native installed Smacker playback with original skip points and subtitles; menu selection survives playback.
- [x] Authored book page turns, submenu camera moves, startup camera entry, and closing Quit sequence; input waits for settling.
- [x] Native scene loading and headless scene export.
- [x] WORLD.INI map, terrain, and palette selection; numeric IDs and map aliases.
- [x] Export all 29 installed worlds, with structural checks.
- [x] Browser scene viewer with download progress, persistent GLB/engine caching, and cache-write failure fallback.
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
| `3D`, `3DC`, `ROB` | Static export, retained animation frames, and explicit source-mapped i32 mesh poses; authored book frames and runtime menu motion | Automatic actor playback, other frame encodings, and model replacement |
| `BSI`, `COL` | Textures and palettes; documented terrain waves in both graphics modes | Other animated textures and visual comparison with the original |
| `GXA` | Startup title artwork, book checkboxes/sliders, movie previews, HUD candle/item artwork, and preload parsing | Software sky projection |
| Glide splash overlay | Installed authored meshes, matrices and textures rendered into a local startup cache | Exact hardware edge coverage, mip selection and shadow interpolation |
| Glide sky BSI/COL | Faithful tilted sky plane, sun, fog and synchronized scroll inputs; camera-centered Modern sky | Full original-game projection/motion, filtering, alpha and lighting comparison |
| `SFX` | Every effect decoded at its stored rate and channel layout; playback by index with the file's loop flag and base volume, master volume, documented distance falloff and stereo pan, via `play_sound` | Script, animation, ambient and surface triggers; per-call volume and rate; audible comparison with the original |
| `RTX` | All voice clips load in the background; playback by label via `play_speech` with dialogue subtitles; voice and subtitle preferences honoured | Dialogue and combat triggers, Cyrus' subtitle colour, dialogue choices, and quest integration |
| `FNT` | Startup labels, book-menu glyphs, dialogue subtitles, HUD numbers, and status font | Remaining prompts and menus |
| `CHT`, `PVO` | Preload parsing | Runtime use and behavior verification |
| SOUP scripts | Original payloads and typed literal Task/End descriptors retained; explicit authored animation-task playback passes native contracts | Automatic activation, full control flow, events, other native bindings, interrupts, and synchronization |
| `MENU.INI` | Main, Options, Display, Sound, Controls, and Movies definitions, navigation, widgets, and action IDs | Save/load, New Game, browser cinematic playback, and progression unlocks |
| `KEYS.INI` | Menu activation, installed key names/capture prompt, fifteen saved bindings, and held player-input foundation | Gameplay locomotion and remaining quick-action handlers |
| `COMBAT.INI`, `ITEM.INI` | Starting-item HUD icons and counts from ITEM.INI | Combat, item use, pickups and shared inventory |
| `SMK` | Native video and first embedded audio stream via the in-process Smacker decoder, installed skip points and subtitle definitions; CD music pauses while a movie plays | Browser playback, audible output and A/V synchronization comparison |
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
camera-plane clipping, book sound cues, mouse selection, gameplay snapshots, saves,
and New Game remain unsupported. Native MCP checks cover all five browsable movies, forward skips,
held-key handling, natural EOF and unsupported-path recovery in the prior decoder,
Quit during playback, and decoder cleanup on world entry. Video and subtitle
screenshots were inspected; the first audio stream decodes to a seekable WAV.
Audible output and A/V synchronization remain unchecked. Subtitle/interlace
preferences apply to cinematics; saved Sound preferences apply to native effects,
speech, and CD music.
Browser runtime verification of this menu slice remains pending.

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
- [ ] **In progress** Split `cyrus-game` into crates by concern in the separate `crate-split` worktree. See the [crate split proposal](#crate-split-proposal).
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

## Crate split proposal

**In progress in the separate `crate-split` worktree. Not landed on main.**

The split now moves existing committed responsibilities. Future stage-1 script
and playback systems follow these boundaries when they exist; the refactor
does not complete world playback. Reconcile the proposal with landed APIs
before each migration step.

Keep `cyrus-game` as the gameplay core. Extract four substantial crates for
assets, world simulation, presentation, and the native control server. Keep the
existing protocol/client crate, `cyrus-control`. The result should let native,
headless, and browser applications share world and gameplay code without
bringing install access, a renderer, or TCP into the core.

This needs more than moving files. `scene.rs` currently resolves worlds,
exports assets, and spawns rendered scenes. `loading.rs` combines install
scanning with progress UI. `player.rs` combines player state with the inspection
camera. `control.rs` combines transport, gameplay commands, and screenshots.
Native and browser entry points also duplicate scene spawning.

### Responsibilities and public interfaces

Names below are proposed destinations. Reuse existing types where their
contracts fit; keep implementation helpers private.

| Owner | Move here | Public contract |
| --- | --- | --- |
| New `cyrus-assets` | `install`, preload scanning, export preparation from `scene`, `animation_export`, `environment_export`, `sound_export`; retained animation/environment/sound records and their loaders | Portable `Animations`, `Environment`, sound metadata and an asset-loading plugin. Native-only `GameInstall`, preload reporting, `export_scene` and `export_all_worlds`. Validate sidecars with file/world/field context. Keep `rgpre` conversion private. |
| New `cyrus-world` | `SceneRequest`, `CurrentScene`, world replacement/lifecycle from `scene`; stage-1 actor, script and animation scheduling once implemented | A `WorldPlugin`, a world request and observable load state. Own simulation entities, playback state, and unload cleanup. Read loaded data from assets; publish state and requests for gameplay and presentation without importing either. |
| Existing `cyrus-game` | `AppState`, fidelity, player state, bindings and action rules; later locomotion and gameplay | `PlayerPlugin`, player/action state, `Fidelity` and its run conditions. Use world queries for collision and object state. Player rules must run without spawning a camera, reading the install, or opening a socket. |
| New `cyrus-render` | GLB spawning, materials/lights, `ViewCamera`, `FlycamSettings`, `FrameScene`, framing and mouse-look; progress UI and sound playback | A presentation plugin, inspection-camera controls and frame-scene requests. Consume world lifecycle and animation state. Keep `AudioSource`, audio handles, mesh updates and screenshot rendering here. Verified sky/water rendering belongs here when available. |
| New `cyrus-control-server` | TCP listener, connection handling and request/reply queue from `control` | A native-only server plugin and typed pending requests/replies using `cyrus-control`. No world, gameplay, camera or screenshot dependencies. Keep socket/thread internals private. |
| Existing `cyrus`, `cyrus-web` | Plugin assembly and platform adapters | Native install/export startup, settings-file access and command application live in `cyrus`. Browser URL, DOM and cache-URL handling live in `cyrus-web`. Both use the same world and presentation plugins. |
| Existing `cyrus-debug`, `cyrus-control`, `cyrus-mcp` | Keep their existing roles | Debug uses public game/world/presentation APIs and a frontend-supplied world list. Protocol/client and MCP remain independent of Bevy gameplay. Preserve the control wire format. |

For example, a native world request resolves its WORLD.INI selection, prepares
the existing cache assets, and hands their references to the world plugin. The
browser starts from those exported references. Neither world simulation nor
rendering knows an install path. An invalid selection must report a contextual
error without partially replacing the current world.

Retain the current request pattern, such as `AnimationRequest(Handle<Animations>)`,
when extracting asset loaders. Separate sound metadata from its audio handles.
Separate native KEYS.INI reading from portable binding/action sampling. Replace
the input sampler's dependency on `FlycamSettings` with an explicit input-lock
state that the camera/debug adapter sets. Enabling inspection must still leave
the player in place and suppress gameplay input.

Split `Settings` data from disk loading. Pass cache paths and the control address
from the native host so gameplay no longer needs filesystem operations or a
protocol dependency merely to obtain defaults. Keep serialized settings and
their current defaults compatible.

### Dependency rules

Each row names dependencies allowed by this proposal. Dependencies point in
one direction; lower-level crates never import their consumers.

| Crate | Allowed project dependencies |
| --- | --- |
| `cyrus-assets` | `rgpre` for native install parsing/export only |
| `cyrus-world` | `cyrus-assets` |
| `cyrus-game` | `cyrus-world`, `cyrus-assets` where gameplay consumes retained data |
| `cyrus-render` | `cyrus-world`, `cyrus-game`, `cyrus-assets` |
| `cyrus-debug` | Public APIs of world, game and render |
| `cyrus-control-server` | `cyrus-control` |
| `cyrus-mcp` | `cyrus-control` |
| `cyrus`, `cyrus-web` | Their required plugins and platform adapters; the browser excludes the native server and install/export APIs |

World scripts may emit typed gameplay requests that `cyrus-game` handles and
answers through the world's contract. They must not import player/controller
implementations. The native app's command dispatcher applies TCP requests to
game, world and render APIs, then returns protocol replies. Screenshots must
not make the transport crate depend on graphics.

Use Rust privacy and Cargo dependencies to enforce these rules. Select minimal
Bevy 0.20 component crates or explicit features for assets, world and gameplay.
The current workspace Bevy defaults enable rendering; moving files alone will
not remove it. Check the dependency graph for each core crate independently,
because a full-workspace build can unify features enabled by the renderer.
Guard native dependencies and APIs by target as well as any export feature.
The browser build must remain valid even when native CI tests all features.

### Migration order

1. **Agree ownership and freeze contracts.** Work from committed `origin/main`
   in the separate worktree. Record native, headless and WASM baselines. Recheck
   this proposal against the landed APIs and reconcile concurrent changes when
   rebasing. Do not move another agent's uncommitted work.
2. **Extract assets.** Move validated records/loaders and native install,
   preload and export code into `crates/cyrus-assets`. Separate scan results
   from their progress UI. Migrate both frontends and asset/install tests in
   the same commit, preserving cache paths and formats.
3. **Extract world lifecycle.** Move world requests, state and cleanup into
   `crates/cyrus-world`. Separate simulation lifetime from rendered entities.
   Use exported assets in both frontends. Migrate stage-1 script and animation
   scheduling with their behavior tests; do not implement missing behavior as
   part of the move.
4. **Extract presentation and narrow gameplay.** Move scene spawning, cameras,
   progress UI and audio playback into `crates/cyrus-render`. Remove duplicate
   browser scene spawning. Separate player input from install reads and camera
   state. Keep the controller in `cyrus-game` until its size and contracts
   justify a separate `cyrus-player` crate.
5. **Extract transport.** Move TCP handling into `crates/cyrus-control-server`
   and command application into the native app. Keep state, teleport, input,
   fidelity, world loading, screenshots and quit compatible through MCP.
6. **Finish assembly and documentation.** Replace the broad native
   `GamePlugin` assembler with frontend composition and a gameplay-only plugin.
   Delete the old modules and temporary re-exports as callers migrate. Update
   workspace/default members, manifests, lockfile, crate READMEs, AGENTS.md,
   SETUP.md, build scripts and CI wherever crate ownership changes.

Each extraction is a separate verified commit. Move implementation, callers
and tests together. Revert the failing unit before continuing if its contracts
cannot be preserved. Do not create empty crates or maintain a second legacy API.
Until this work starts, improve boundaries within existing modules where needed
for stage 1; crate moves must not delay world playback.

### Verification and completion

Run `cargo test -p <changed-crate>` for quick feedback after each move, then
`just gates` before its commit. Move the existing public-interface tests to
their owners. Add only missing boundary checks needed to prove the split.

| Check | Required result |
| --- | --- |
| Assets and exports | All 29 installed worlds, WORLD.INI numeric selections, aliases and palette variants resolve as before. GLB content and sidecar data remain equivalent, with unchanged cache paths/formats. Invalid assets report context; install-backed tests announce skips when absent. |
| World/game core | A minimal headless Bevy app can load retained data, change worlds and run existing simulation/input contracts without window, GPU, audio playback or TCP plugins. Old world entities, waits and playback state are cleaned up as specified by the stage-1 baseline. |
| Player and fidelity | Held/released actions, unsupported bindings, input lockout, independent flycam and Faithful defaults retain their tested behavior. No gameplay feature is marked complete by a crate move. |
| Dependency graph | `cargo tree -p cyrus-world -e features` and `cargo tree -p cyrus-game -e features` show no renderer/window/socket path. Check portable assets without native export features too. |
| Native control | `just smoke-mcp` and the native screenshot/load-scene checks retain the public JSON protocol and observable results. Record supported headless commands separately from rendering-only requests before migration. |
| Browser | `cargo check -p cyrus-web --target wasm32-unknown-unknown` and `just web-build` succeed. Drive scene switching, sidecar loading, flycam and F1 controls in the browser. No install or native transport API enters WASM. |
| End-to-end baseline | `cargo run -p cyrus -- --export-all-worlds`, native scene loading, headless startup and the browser viewer still work. Record the revision, commands and any unavailable verification in the roadmap. |

**Done when:** both frontends share world and presentation code, the core can
run headless without graphics or network dependencies, public contracts pass,
and the old mixed modules are gone. Original-game comparisons remain required
for fidelity; preserving today's behavior during the split does not prove it
matches Redguard.

The main risks are Cargo feature unification, changes to world-unload ordering,
and confusing retained asset data with live playback. Build-time improvements
are not promised without measurements. Keep Bevy 0.20, cache formats, protocol
formats and gameplay scope unchanged during this refactor.

## 1. Complete world playback

**In progress.** Static loading works. Complete playback is still open.

- [x] Load and export worlds with their terrain, textures, and palettes.
- [ ] Compare placement, scale, lighting, sky, transparency, and visibility with the original.
- [x] Retain actor animation metadata, original section payloads and model frames in native/WASM assets. **Verified:** all 29 installed worlds export without unresolved animation models; native asset tests and WASM compilation pass.
- [x] Apply explicit i32 model poses and forward groups using supplied simulation ticks. **Verified:** 17 native animation contracts, installed FLAG2 GLB deformation/restoration, and WASM compilation.
- [ ] Play actor and mesh-frame animations, including loops, exit sequences, queued transitions, and animation event cues.
  Explicit forward groups and source-mapped mesh poses are supported.
  A separate forward type-zero request core handles ordinary numeric priority,
  forced replacement, finite task loop counts, and explicit default fallback
  in the same update. Literal PushAnimation/End programs are now connected to
  explicit rendered playback. Native contracts verify their stage ordering and
  installed FLAG2 transitions; WASM compilation and final workspace gates pass. Automatic activation,
  the host-clock driver, state-dependent defaults, other group types, complete
  script scheduling and event cues remain unsupported. Original-game visual comparison is pending.
  Frame reference, countdown and walking-marker facts are published in preservation
  commits `8c3856e`, `9b4206b` and `92b9933`. Request priority and default fallback
  are documented in `84cc21d`. Placed-actor initialization, spatial update
  membership and forced requests are documented in `d1e8e46`; `64eca10` corrects
  the activation-window center to the current view coordinates. `ea35fca`
  documents advancement before the main script stage and immediate script
  request advancement. Preservation `a1ffba6` documents the separate task-start
  phase and authored End continuation; it corrects the permanent-stop assumption.
  All 34 native animation
  contracts pass in final workspace tests, including installed FLAG2 task
  transitions, End restart, GLB deformation and restoration without skipping.
  Final workspace formatting, strict all-target Clippy and tests pass.
  The integrated core also handles delay precedence, forward type-1 exits and
  explicit sound handlers, documented in preservation [PR #30](https://github.com/michidk/redguard-preservation/pull/30).
  Native sound dispatch is not wired. A narrow 13-update original walking exit
  comparison covers identifiers, groups and markers only. Automatic rendered
  playback, mesh and audio parity remain unchecked.
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
Complete animation transitions and collision integration remain unresolved;
locomotion is not enabled.

Input foundation: eight held directional/action bindings from
`KEYS.INI`, Modern WASD/Space/held-Shift input, an independent debug flycam
toggle, and input/camera-isolation tests. A deterministic ground-turn core
matches captured original heading changes, including a frame-scale change and
release. It is not connected to live gameplay while timing and state eligibility
remain unresolved. Workspace tests, strict Clippy, formatting, publication checks,
and an MCP smoke test pass using isolated worktree artifacts. Player tests cover
installed bindings, Modern input, camera isolation, ground-turn arithmetic, and
animation-gated walking startup and release reset. The walking core supports step
type 1 and rejects other step types. It is not connected to gameplay. Translation
and the original follow camera remain unsupported.

Initial marker placement uses exact exported map coordinates and retains the
requested heading modulo 2048. A headless ISLAND load matches a fresh original
marker-zero start-position query; debug flycam input leaves the player unchanged.
Missing and unavailable markers remain explicit errors. Native workspace tests,
strict Clippy, WASM compilation and MCP smoke pass. Ground settling and rendered
heading are separate and remain unsupported.

- [x] Land and verify the keyboard-input and independent debug-flycam foundation.
- [x] Apply explicit marker-based initial placement; scripted spawn selection and ground settling remain pending.
- [ ] Implement quick actions and the walk-mode toggle, alongside the held bindings.
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

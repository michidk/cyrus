# ROADMAP

A delivery plan for a faithful rebuild of Redguard in Rust and Bevy. The original game, run from the player's own GOG install, is the specification. Scope is limited to the original single-player game: no multiplayer, new content, or functionality the original did not have.

## Current baseline

Documented baseline at commit 1df0990. These capabilities were not revalidated by the planning task.

- Workspace, fidelity settings, install discovery, startup preload, scene export/load, and WORLD.INI world selection exist.
- TCP control, MCP control, DOSBox-X reference runner, and browser scene viewer exist.
- Controllers remain placeholders. The first native virtual-display comparison and preload coverage report remain acceptance work.

## Delivery sequence

Stages 01 through 08 describe dependency order, not dates or completion percentages. After stage 05, full-content coverage can proceed alongside stage 06; both must finish before stage 07 is accepted.

## 01. Establish the reference

Status: Next. Depends on: None.

Capture reproducible evidence before expanding gameplay.

- [ ] Run cyrus under docker/dev and the original under DOSBox-X. Capture the same scene, position, camera, and input sequence.
- [ ] Inventory preload results by file type. Separate parsed, unsupported, and failed inputs; send format gaps to rgpre.
- [ ] Define a comparison matrix for every WORLD.INI entry, including shared maps with different palettes. Keep captures and research under magic/ or cyrus-config/.

**Exit condition.** A repeatable first side-by-side comparison exists, and every install file type has an explicit coverage status.

## 02. Make the worlds trustworthy

Status: Planned. Depends on: 01.

Load the right world and reproduce its appearance.

- [ ] Validate model placement, terrain, coordinate conversion, scale, textures, palettes, transparency, and world transitions against the original.
- [ ] Reproduce sky, water, lighting, and visibility rules from evidence. Track visual differences per world instead of treating a successful export as fidelity.
- [ ] Keep native scene loads and browser exports consistent. Verify numeric world IDs and map-stem aliases preserve the documented selection rules.

**Exit condition.** Every WORLD.INI entry loads or has an explicit blocker; representative viewpoints and all palette variants match an agreed reference comparison.

## 03. Make movement playable

Status: Planned. Depends on: 01, 02.

Replace placeholder control with observed movement and collision.

- [ ] Resolve the player start from game data. Read the keyboard mapping from KEYS.INI and verify it in the original.
- [ ] Measure movement, turning, camera behavior, jumps, gravity, slopes, steps, walls, and ledges. Implement only supported behavior in Faithful mode.
- [ ] Investigate swimming, climbing, falls, and scene exits where the original requires them. Exercise collision at boundaries and across frame rates.
- [ ] Verify that controls and camera behavior reproduce the original. Alternative behavior is not a delivery goal.

**Exit condition.** Recorded input routes reproduce movement and camera behavior, block solid geometry, and cross supported scene boundaries without trapping the player.

## 04. Bring actors and scripts to life

Status: Planned. Depends on: 02, 03.

Give world objects their original behavior.

- [ ] Load actors from RGM and validate animation timing, transforms, idle states, and movement against reference scenes.
- [ ] Inventory SOUP operations used by shipped content. Build execution state, scheduling, variables, events, and native bindings from preservation facts.
- [ ] Begin with an interpreter that reports unsupported operations. Expand it through observed interactions; never silently guess an operation.
- [ ] Implement triggers, doors, object activation, and actor behavior required by the first playable route.

**Exit condition.** A selected interaction sequence runs from trigger to visible result; unsupported operations identify the blocked content and fail explicitly.

## 05. Complete one gameplay route

Status: Planned. Depends on: 03, 04.

Prove the systems together before broad content coverage.

- [ ] Implement RTX dialogue, choices, conditions, and quest state for a bounded route observed in the original.
- [ ] Implement inventory and item behavior from ITEM.INI, plus combat rules from COMBAT.INI and observed timing.
- [ ] Connect damage, death, restart, pickups, and quest consequences to script and actor state.
- [ ] Choose a route that includes a conversation, an item, combat, and a scene transition. Record its exact steps and expected outcomes.

**Exit condition.** The same route completes in both games with matching observable outcomes and no unsupported operation on that route.

## 06. Complete the player experience

Status: Planned. Depends on: 05.

Make progress persistent and the game usable outside a test session.

- [ ] Reproduce menus, fonts, and interface artwork through MENU.INI, GXA, and supported font formats. Cover input, settings, pause, and feedback.
- [ ] Implement save and load for world, player, script, inventory, and quest state. Verify interruption-safe writes into cyrus-config/, never the install.
- [ ] Decide original-save compatibility only after format support is established. Document the supported boundary explicitly.
- [ ] Add effects, speech, and music from the owned install and CD image. Establish an audio-enabled reference because the current retro container disables sound.
- [ ] Verify animation, dialogue, and audio synchronization, volume controls, and missing-data errors.

**Exit condition.** A player can start, play, save, restart, and resume the route with equivalent state, working menus, and reference-checked audio.

## 07. Reach full-game fidelity

Status: Planned. Depends on: 05, 06.

Expand from the proven route to the whole original game.

- [ ] Enumerate reachable worlds, quests, actors, script operations, items, combat encounters, and endings from the install and reference playthroughs.
- [ ] Close coverage gaps in small verified changes. Exercise quest order, revisits, failure paths, death, and save/load across transitions.
- [ ] Run a complete new-game-to-ending playthrough with checkpoint comparisons. Fix blockers and review every remaining unverified assumption.
- [ ] Classify remaining differences by fidelity area and evidence. Resolve deviations from original behavior before claiming fidelity.

**Exit condition.** A complete playthrough finishes without blockers; the content matrix has no unexplained omissions, and known differences have explicit evidence and disposition.

## 08. Ship a reproducible release

Status: Planned. Depends on: 07.

Let an owner of Redguard install and play cyrus reliably.

- [ ] Choose supported operating systems and hardware explicitly. Validate a clean setup, install discovery, error messages, and packaged binaries on that matrix.
- [ ] Measure loading, memory, and frame pacing on representative worlds and declared hardware. Set budgets from those measurements and fix observed bottlenecks.
- [ ] Run release gates, dependency/license review, and an artifact audit that excludes game data, research, caches, and credentials.
- [ ] Publish player setup instructions, controls, supported features, known limitations, and recovery steps. Keep the browser scene viewer scope explicit unless gameplay parity is separately approved.

**Exit condition.** A clean machine with a purchased GOG install can run the release, finish the validated route, and save/resume; packages contain no game assets.

## Rules for every stage

- Observe the original, specify behavior, implement one piece, and compare the same inputs. Run `just gates` before each commit.
- `rgpre` owns formats and published engine facts. cyrus owns Bevy behavior, integration, fidelity modes, and player experience. Contribute discoveries upstream.
- Read the install only. Keep saves and settings in `cyrus-config/`. Keep research and copyrighted captures in `magic/` or `cyrus-config/`, untracked.
- Default to Faithful. The existing fidelity switch does not authorize new functionality or expand the scope beyond the original game.
- Keep native filesystem, socket, and thread access out of the browser build.

## Unknowns and decisions

- Preload coverage determines required parser work. Unsupported data blocks the affected feature.
- Inventory shipped SOUP operations before estimating gameplay coverage.
- Standardize reference scene, camera, input timing, and capture conditions. The current retro container disables sound and cannot validate audio.
- Decide original-save compatibility after format support is established.
- Choose release platforms and hardware before setting performance budgets or release dates.

## Immediate next tasks

1. Produce the first native scene capture beside the original and record reproduction steps locally.
2. Report preload coverage by file type; assign failures to cyrus integration or rgpre format support.
3. Create the world comparison matrix and verify START plus a shared-map palette variant.
4. Resolve player spawn and input mapping from data, then measure a short movement route in both games.

## Out of scope

Multiplayer, new content, and functionality absent from the original are excluded, not deferred. Decoders derived from third-party GPL sources are also excluded. The browser scene viewer does not commit the project to full browser gameplay.

## Maintaining this plan

This file is the source of truth. Change stage status only when its exit evidence exists. Later stages describe proposed work, not existing capabilities. Keep research and captures outside tracked documentation. Follow [METHOD.md](METHOD.md) for evidence and [SETUP.md](SETUP.md) to run both games.

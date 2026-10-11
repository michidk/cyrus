# Split cyrus-game by ownership


Evidence baseline: `0d6116bca7eeecceea2280b69eaf23e201cd373b`. GitNexus 1.6.12 index refreshed
with PDG; source-content and analyzer provenance checks passed. Evidence schema 2;
57 cited paths; dirty digest `0a9c85780067d9afcd0764f307b60891e3cee927ee11eaeb5ec7826d10fd82cd`.

## 1. Objective

Make world simulation and player rules usable without rendering, install access,
audio playback, or TCP. Native and browser applications should assemble the same
world and presentation plugins. Keep Bevy 0.20, existing cache formats, settings,
control protocol, and supported behavior intact.

This is an implementation plan, not a claim that the split or missing gameplay
is complete. The roadmap's old `crate-split` worktree is not the starting point.
Start each migration unit from current main and reconcile active domain work.

## 2. Current behavior

[verified, source-derived] `crates/cyrus-game/src/lib.rs:61` composes preload,
scene loading, player/camera, control, sound, music, and subtitles in a native-only
`GamePlugin`. `crates/cyrus/src/main.rs:69` separately installs animation,
environment, HUD, startup, and debug plugins. Browser assembly and scene spawning
are separate in `crates/cyrus-web/src/main.rs:37` and `:101`.

[verified, source-derived] `scene.rs:70` exports before removing the old scene,
then requests animation/player sidecars, optionally spawns graphics, and updates
`CurrentScene`. Equal request names return early. Asset readiness is asynchronous;
`CurrentScene` does not mean every sidecar and rendered mesh has loaded.

[verified, source-derived] `player.rs:57` registers both input and camera systems.
`player/input.rs:126` reads installed bindings and saved menu preferences.
`animation.rs:155` registers asset loading, playback, sound ordering, and mesh
updates together. `sound_bank.rs:40` mixes serialized metadata with audio handles.

## 3. Target architecture

Proposed ownership, not existing crates. Use three extra crates inspired by the roadmap,
with explicit submodules rather than a crate for every plugin.

| Owner | Responsibilities | Allowed project dependencies |
| --- | --- | --- |
| `cyrus-assets` | Validated portable cache records and loaders; native install discovery, export, preload parsing and installed media extraction | `cyrus-text`; optional native `rgpre` |
| `cyrus-world` | World identity and lifetime; existing explicit animation execution; geometry and terrain queries independent of the player | `cyrus-assets` |
| `cyrus-game` | App state, fidelity policy, player state/input/controller rules, runtime preferences | `cyrus-world`, `cyrus-assets` |
| `cyrus-render` | Scene rendering, environment, mesh poses, cameras, startup/book menu, HUD, effects/music/speech/subtitles and cinematic presentation | `cyrus-game`, `cyrus-world`, `cyrus-assets`, `cyrus-text` |
| `cyrus-control::server` | Native TCP framing and pending request/reply ownership, as a module in the existing crate | Standard library and existing protocol types |
| `cyrus-debug` | Inspector, flycam/fidelity controls, host-supplied world picker | Public game/world/render APIs |
| `cyrus`, `cyrus-web` | Platform setup, storage and source adapters, plugin assembly; native command dispatcher | Required crates above |
| `cyrus-control`, `cyrus-mcp`, `cyrus-text` | Keep protocol/client, MCP adapter and font rendering roles | No new dependency on game or presentation |

World never imports game. `AppState` and `Fidelity` remain game-owned; world
systems operate on world requests and their own resources, not `AppState`.
Host/game assembly supplies any application-state run conditions on public world
system sets. Assets never import world, game, or render.

Within render use `scene`, `environment`, `camera`, `animation`, `audio`, `startup`,
`hud`, and `subtitles` modules. Rendering and audio remain distinct plugins so a
headless app need not install them. Within world use `lifecycle`, `animation`,
and `collision`; within game retain `player`, `fidelity`, and `preferences`.
Do not introduce `common`, `shared`, or a generic service registry.

## 4. GitNexus findings

The requested index was built with GitNexus 1.6.12 using
`GITNEXUS_WORKER_POOL_SIZE=2 gitnexus analyze --index-only --pdg`.
`gitnexus status` reports matching analyzer provenance and covered source content.
The first attempt terminated before publication; the retry succeeded. No
instruction/skill files were injected. Main advanced during planning; the index
and source evidence were refreshed for the final commit pin. This includes
portable movie playback, browser sidecar bindings and prepared static collision
queries. Earlier candidate sketches were reconciled against those changes.

[graph, source-confirmed] `context(export_scene)` and upstream
`impact(export_scene, maxDepth=3, includeTests=true)` report seven direct
callers: `load_requested`, `export_all_worlds`, the installed animation mesh
contract, three installed world/sky/water export contracts, and the player export
contract. All are accounted for in units 2/3/5 and the test migration. The native
main function is a transitive caller.

[graph, source-confirmed] Upstream impact on the `Animations` struct identifies
`animation_export::export` directly and the scene export chain transitively.
The constructor and its consumer must move together. Source reads additionally
identify runtime readers and tests that a constructor-call traversal does not
fully enumerate.

[graph] The cluster/process resources identify startup, animation and player
cohesion, with export flows crossing those areas. They are navigation hints,
not proposed crate boundaries. `context(PlayerPlugin)` reports no incoming
references despite visible plugin registration in both frontends. Therefore the
plan does not treat empty graph results or its `exact` marker as proof of
complete Bevy registration coverage.

[graph] PDG controls for `load_requested` corroborate early returns and the
headless split. PDG flows for `animation/playback.rs`, variable `ticks`, confirm
its reuse through group and scripted advancement. Preference PDG includes both
conditional compilation arms and does not distinguish all match cases; native
versus browser storage behavior is verified from source. Some functions emitted
CDG construction warnings, and process enumeration hit limits. Neither the
fresh index nor an absent edge proves those paths unused.

## 5. Ordering and failure constraints

[verified, source-derived and PDG corroborated] `scene.rs:81–103` has an
unchanged-name guard, an export-error return before despawn, and a headless
branch before rendering. Preserve those distinct outcomes. Do not silently turn
this extraction into asynchronous export, automatic reload, or transactional
replacement of all cache files.

[verified, source-derived] The current cross-system order is menu editing before
sound preference application, animation advancement after preference application,
then cue consumption in the same Update. Pose application is PostUpdate
(`startup/menu.rs:134`, `animation.rs:155`). Replace references to private sibling
functions with public system sets owned by their domains and order those sets in
composition. Do not substitute next-frame messages for the synchronous sound
callback used by the deterministic animation cores. Keep a single ordered cue
queue for flat and animation effects rather than forwarding between two queues.
The sound consumer registers `Messages<SoundCue>`; world only defines the type.
A core-only app without that consumer must still reject sound-bearing commands,
not silently enqueue and discard them (`animation/playback.rs:100`). Native
headless assembly currently installs SoundPlugin even without Bevy AudioPlugin;
retain its control-visible state/unsupported outcomes without creating an audio
backend. Core-only tests and the native headless application's plugin set are
different contracts.

[verified, source-derived and PDG corroborated] `animation/playback.rs:65` passes
the same supplied ticks through ordinary advancement, default requests and script
advancement. A zero tick is still a supplied update. Do not derive ticks from
render frames or add a production clock in this migration.

[verified, source-derived] `startup/menu.rs:397` saves preferences before replacing
runtime values. `startup/menu_preferences.rs:36` distinguishes missing local state
from malformed or inaccessible state. Storage failure must leave the previous
values active; malformed files must remain intact. Direct scene launches must
load saved bindings and sound preferences without constructing a menu.
Keep one loaded `Result<Option<MenuPreferences>, String>`, not a default that
swallows errors. Menu shows book defaults, records the persistence error and disables edits
on load failure. On missing state each consumer resolves its own defaults: MENU
for menu, KEYS.INI for input, SYSTEM.INI for sound and exported HUD bindings for HUD.
Never resolve `Ok(None)` centrally. Input currently leaves bindings disabled on a malformed menu
file; sound logs and falls back to installed volumes; HUD falls back to its
installed bindings. Preserve those policies when centralizing IO
(`player/input.rs:142`, `sound.rs:206`, `hud.rs:300`).

[verified, source-derived] `control.rs:49` releases taps and handles commands in
PreUpdate after Bevy input, in that order. Preserve response IDs, JSON fields,
request order and the current meaning of screenshot acknowledgement. Splitting
transport must not move command execution into a socket thread.

## 6. Proposed changes

### Portable assets must stop constructing runtime objects

Move serialized animation/environment/collision/menu/HUD/sound records and their
validation into domain modules under assets. Include all native exporters,
including `player/export*`, `hud_export`, `startup/export`, and `startup/menu_export`.
Separate preload scanning/results from progress UI and application transitions.
Keep export paths explicit parameters instead of calls to game `Settings`.

[verified, source-derived] Existing factory methods cross the intended boundary:
`Animations::sequence`, `actor_animation`, and `animation_script` return runtime
objects (`animation/sequence.rs:119`, `actor.rs:19`, `script.rs:52`). Move these to
constructors on world-owned runtime types, taking `&Animations`. Move renderer
conversion `AnimationModel::frame_geometry` to `FrameGeometry::from_model` in
render. Migrate callers and remove old factories in the same unit; foreign
inherent implementations and a compatibility trait are not solutions.

Keep data-only frame reference resolution with the asset if it needs only cache
records. Expose narrow validated lookup methods needed by runtime constructors,
not the entire parser implementation. Preserve contextual unsupported errors.

[verified, source-derived] `PlayerScene` currently contains `TerrainGround`,
`PlayerDefinition`, and prepared `StaticCollision`, and collision records construct `CollisionFace`
(`player/data.rs:24`, `player/collision_data.rs:58`). Keep serialized heights,
water masks, placement order, flags, and definition fields in assets. Move
terrain sampling, polygon construction/intersection, and support selection into
world, including the newly landed `collision_mesh.rs` and `collision_world.rs`.
Game owns the prepared `PlayerScene` asset and its asynchronous loader, which
combines validated asset records with world query constructors and game-owned
`PlayerDefinition`. Preserve loading error context and asynchronous preparation.
Build `StaticCollision` once per loaded record, not per query. Keep its model
versions, face textures, surface assignments and candidate ordering. Move
player-specific forward-admission policy out of query result methods into game
when the result types become foreign types. Keep player body/contact response, turn/walk and action rules in game.
Replace data-owned constructors that return world/game types with constructors
on the consuming type. Retain schema-2 validation and unavailable reasons; do
not decode malformed data as empty collision geometry.

[verified, source-derived] `MenuAsset` embeds `MenuItem` and `MenuPreferences`
(`startup/menu_data.rs:9`). Move cache entry records and installed preference
default records together into assets. Reuse the validated serialized preference value in a game-owned live resource,
with one writer after successful persistence; render owns selection, animation and readiness. Do not make assets depend on
the menu implementation or game preferences merely to deserialize defaults.
`HudAsset` and item/font/layout records also belong in assets. HUD drawing,
candle presentation state and control snapshots remain render-owned; game-owned
toggles and input intent must not require importing HUD drawing code.

Split sound bank metadata from `Handle<AudioSource>` storage. Audio handles,
channel allocation and decoders implementing Bevy audio traits stay in render.
The current `SoundCue`, `SoundSurface`, and `AnimationSound` contracts live with
world runtime so game/render can produce or consume them without a reverse
edge. Assets owns the installed sound settings data and PCM/WAV conversion that
native export needs; render owns mutable mixer settings and gain application.

### Native IO is an adapter, not a core dependency

Use an opt-in, target-gated `native` feature in assets for install readers,
exports, preload and installed media extraction. Native app enables it; world/game and the
browser consume the portable default. Move installed movie preparation/export from `startup/movie_native.rs` to
assets/native. Move `movie_data.rs` metadata, validation and compressed bytes to
portable assets, separating its `Prepared.audio: AudioSource` into raw WAV bytes.
Keep Smacker decoding portable: browser playback already exists and `smk` is now
an unconditional dependency. A portable media decoder produces RGBA frames;
render owns native worker/browser cooperative scheduling and its existing seek
budget, plus `Image`, `AudioSource`, playback progress and menu return. Retain
cancellation on receiver drop. Keep speech/CD reads behind the same boundary.
Hosts initiate native work and hand results to presentation through concrete
resources/messages; do not create a pluggable backend framework.

[verified, source-derived] `cyrus-text/Cargo.toml` currently pulls `rgpre` into
every native build. Gate its native font conversion behind an opt-in feature
that assets/native enables, or a supposedly portable assets build still inherits
native preservation dependencies. Font records/rasterization remain portable.

Split `Settings` into native host configuration and game-owned fidelity/runtime
values. Keep the existing TOML/JSON keys and localStorage names. Hosts own the
concrete filesystem/localStorage persistence adapters. Define a small game-owned
`PreferenceStore` interface for loading/saving the existing menu and HUD records;
implement it in each host, with errors preserved. A missing adapter is an error,
never `Ok(None)`; host assembly must install it before consumers run. Expose synchronous save
operations to menu editing so saving still precedes runtime application in the
same update; use explicit native/browser adapters, not an eventual save queue.
Keep separate menu and HUD persisted files, rather than merging schemas.

### World and presentation share one lifecycle

Move `SceneRequest`, `CurrentScene`, and lifetime ownership into world. A host
resolves the native world selection and prepares cache references; the browser
prepares equivalent URLs. World accepts one prepared request containing identity
and retained asset references. Render derives its GLB/environment requests from
that same request instead of reconstructing filenames independently.

Assets owns validated `SceneData`; game owns the prepared `PlayerScene` asset.
World carries the player sidecar path, so it never names the game-owned asset.
Graphics paths remain paths, not render asset handles.
Preserve requested name and resolved WORLD.INI identity separately, because map
aliases and numeric palette variants are different concerns. Preserve the public
scene string used by control and the current same-name no-op. Represent loading
and failure separately from current identity; do not claim ready when just the
export finished. This status is internal unless a separate protocol change is
approved.

Make cleanup ownership explicit: world owns live simulation/playback entities;
render owns GLB descendants, private meshes, environment and lights. Both react
to the same committed world change. Keep the present explicit playback entity
attachment while moving it; a new full actor/entity hierarchy is separate work.
Preserve per-session water/sky clock behavior, same-world rebinding tests and
player marker invalidation while assets load. Headless requests must not wait
for graphics assets, create a camera, or install an audio backend.

Separate `ViewCamera`, `FlycamSettings`, `FrameScene`, framing and mouse motion
from `PlayerPlugin`. Game input consumes an explicit input-suppression resource;
the inspection/debug adapters set it before input sampling. Player pose must not
move when flycam moves. Debug receives world options and submits the common
request; URL/DOM and native install lookups belong to the hosts.

### Keep the public interface small

Proposed signatures, with names finalized during the owning extraction:

```rust
// assets/native: preparation, not scene mutation
pub fn export_scene(install: &GameInstall, name: &str, out: &Path) -> Result<()>;
// world/lifecycle: no Handle<Scene>, Mesh, camera or install path
pub struct PreparedWorld {
    pub requested_name: String,
    pub world: u32,
    pub animations: Handle<Animations>,
    pub player_path: AssetPath<'static>,
    pub scene_path: AssetPath<'static>,
    pub environment_path: AssetPath<'static>,
}
// world/animation: constructors replace factories on foreign asset types
pub fn from_asset(data: &Animations, actor: &str, default: u16) -> Result<ActorAnimation>;
// render/animation: exporter mapping to render geometry
pub fn from_model(model: &AnimationModel, frame: usize, binding: &MeshFrameBinding)
    -> Result<FrameGeometry>;
// cyrus-control::server: host drains the std channel in frame order and replies once
pub fn listen(addr: &str) -> std::io::Result<std::sync::mpsc::Receiver<PendingRequest>>;
impl PendingRequest {
    pub fn respond(self, response: cyrus_control::Response);
}
```

The caller should assemble `AssetDataPlugin`, `WorldPlugin`, and a gameplay-only
`GamePlugin` for headless contracts. Native/windowed and browser hosts additionally
install the same presentation plugins. The native-only `cyrus-control::server` module exposes requests;
`cyrus/src/control.rs` applies them using public game/world/render APIs.
No transport dependency on `cyrus-game` or screenshot types is needed.

### Alternatives and synthesis

Use the layered candidate as the base. A domain-first candidate extracts
animation and frontend now, leaving scene rendering, camera/player coupling and
native IO in game. It offers smaller initial moves but leaves the roadmap's core
isolation and shared world-loading goal unfinished. Do not choose crate count
alone over completing those boundaries.

Adopt the layered candidate's in-place cycle refactors and std-only transport
module. Retain its warning about different preference error policies. Adopt the
domain candidate's explicit sound-consumer availability contract and install-test
skip-marker check. Reject an extra animation cue queue because preserving one
queue avoids accidental reordering between flat and animation effects.

Keep portable records and their native exporters together in assets. Reject
keeping native exporters in world/game solely to retain existing inherent
factory calls: moving the constructors onto runtime types is an intentional,
bounded API refactor that makes the dependency direction enforceable. The
serialized record and its exporter still have one owner. Do not expose a broad
`WorldSource` full of preservation parser internals across runtime crates.

The large presentation crate is deliberate but must have private domain modules
and separate plugins. Do not extract more crates just to reduce its line count.
An audio or startup crate is a later option if independent consumers or build
measurements justify that boundary.

The independent review favored this design after resolving prepared-player ownership,
consumer-specific preference fallbacks and mixed animation/audio test placement.
Those corrections are included above and in the migration checks below.

## 7. Implementation sequence

Each numbered unit ends with migrated callers/tests, a coherent build and a
reviewable commit. Refactor inside current modules first where needed to avoid
extracting circular dependencies. Do not add empty destination crates.

1. **Capture contracts and claim ownership.** Rebase onto current main, agree
   file ownership with active player/render/sound work, and replace the obsolete
   roadmap split status with the current task. Record native, headless, browser,
   exports and preference baselines under ignored artifacts. Locate every public
   factory caller. Do not import the unfinished branch wholesale.
2. **Separate portable records and extract assets.** Cut foreign factory methods,
   collision record/query coupling, menu defaults/state coupling and audio
   metadata/handles. Add assets with current formats and native feature gating,
   including text conversion. Move pure loader/export tests with it. Keep native
   adapters in their current consumers until their later extraction; all readers
   already use the new records. No assets-to-game dependency is allowed even
   temporarily at the end of this unit.
3. **Extract world execution and lifetime.** Move animation cores and explicit
   playback, collision/terrain queries, sound intents and prepared world request
   state. Introduce only the system sets needed for existing ordering. Adapt
   game-owned player logic to world queries. Preserve no-tick cleanup, zero-tick
   advancement and caller-supplied eligibility. Both frontends submit the shared
   lifecycle request; existing rendering consumes it until unit 5.
4. **Remove platform/presentation work from player and preferences.** Separate
   camera systems, binding IO and settings storage within the current crate.
   Move host adapters to native/browser modules, pass runtime values into game,
   and replace flycam-dependent sampling with explicit suppression. Share one
   accepted preference state across menu, input and sound consumers.
5. **Extract presentation and unify frontend assembly.** Move environment,
   terrain/sky/water/fog, mesh application, camera, startup, HUD and audio into
   render. Move remaining native media IO behind assets/native and host adapters.
   Delete browser `spawn_scene` duplication. Reduce GamePlugin to gameplay;
   migrate debug imports and mixed rendering tests. Preserve menu/movie/audio
   scheduling and make both frontends use the same plugin composition contracts.
6. **Extract TCP transport.** Move listener/framing/reply ownership to the existing
   `cyrus-control` crate's native `server` module and command application/tap scheduling to the native app. Move transport
   tests and retain end-to-end MCP coverage. Do not change the protocol or give
   the server a dependency on renderer/game.
7. **Enforce and document boundaries.** Remove obsolete modules/re-exports and
   duplicate plugin setup. Add per-package dependency checks to CI, finish
   READMEs/MAP/SETUP/AGENTS references, and replace the roadmap proposal with the
   landed ownership and remaining feature work. Verify the combined tip once.

Units 2 and 5 are the broadest moves. They may be divided into domain-complete
commits after the internal seams exist, but never leave two supported APIs or
one frontend on the old implementation. Stop and redesign if an extraction
needs a reverse dependency or a generic catch-all crate.

## 8. Verification strategy

Move tests by the contract they exercise, not merely their current filename.
Existing `crates/cyrus-game/tests/animation.rs` mixes loading, deterministic
execution, mesh restoration and installed export; split those tests among
assets, world and render. `animation_script.rs`, `animation_transitions.rs`,
follow world execution. The first three pure tests in `animation_script_sounds.rs`
move to world; `group_and_scripted_playback_start_and_stop_sound_in_the_pose_update`
installs SoundPlugin and belongs in render with its installed-data dependencies. `player.rs` divides
input behavior from camera behavior. `player_ground.rs`, `player_support.rs`, `player_collision_mesh.rs`, and
`player_collision_world.rs` follow world queries, with player admission policy
assertions retained in game; `player_collision_data.rs` divides record validation from
geometry conversion. `install.rs`, `player_export.rs`, and `menu.rs` follow
native assets; `sound.rs` follows render except its data-only export contracts.
Keep mixed end-to-end assertions in the upper consumer, not as a reverse
production dependency from assets to rendering.

Add only missing boundary contracts:

- Minimal assets/world/game app, no renderer/audio/TCP plugins: synthetic sidecars,
  player input and explicit animation updates work; no camera entities appear.
- Failed world preparation leaves the active scene intact; a superseded pending
  sidecar cannot apply to a newer world. Missing/corrupt sidecars retain context.
- World replacement clears playback/poses without requiring a simulation tick;
  removing a pose restores a private mesh without changing another instance.
- Missing sound consumer rejects a sound-bearing animation; installing the
  consumer preserves ordered flat/animation cues without adding an update delay.
- Same-update menu sound edits affect that update's cue; Start/Stop/Forget,
  multiple ticks including zero, and the existing channel-limit tests still pass.
- Malformed saved preferences remain untouched; failed save leaves runtime values
  unchanged; direct world launches use edited bindings and audio preferences. Browser world
  binding initialization still waits for the player sidecar, clears prior
  bindings during a load and applies local overrides to exported keyboard defaults.
- Flycam movement changes only the camera; input suppression clears held gameplay
  actions and respects debug text capture.
- Transport handles malformed JSON, request IDs, sequential replies and dropped
  replies; MCP retains state, taps, scene changes, fidelity and screenshots.

For each changed code unit run the narrowest relevant test first, then one
`just gates` after its final code edit. Run `just genmap` before gates when module
headers change. Normalize migrated install-test skip messages to the existing gate
marker `skipped: no install`; current tests use several spellings, so a zero
match alone does not prove every installed test ran. Use the repository
toolchain/cargo-hauler wrappers and a separate
worktree target directory; await a build ticket instead of resubmitting it.

Add isolated dependency checks for assets without native features, world and game.
Use `cargo tree -p <crate> -e features` and standalone package checks; inspect
normal/build dependencies rather than counting test-only dependencies. Reject
renderer/window/audio-backend/TCP dependencies and `rgpre` in those portable
builds. Avoid workspace feature unification hiding an accidental dependency.
Use minimal Bevy 0.20 subcrates or disable defaults at the workspace dependency
and explicitly enable frontend features. A per-crate `default-features = false`
is insufficient while inherited workspace defaults remain enabled.

Run `cargo check -p cyrus-web --target wasm32-unknown-unknown`, `just web-build`,
`just smoke-mcp`, native scene/menu/movie checks and browser scene/menu/movie/flycam/F1
checks. Preserve cooperative browser seeking, exported movie header/metadata
validation, download failure, skip/menu-return and replay behavior. Compare installed export outputs for every WORLD.INI entry, including
numeric palette variants, to the baseline using GLB content and parsed sidecars.
Keep cache paths/versions and local state files unchanged. Existing protocol
fixtures under `dev/fixtures/` should require no semantic changes.

These checks establish refactor equivalence. Original-game visual/audio/timing
parity remains a separate roadmap requirement. No Cargo or game execution is
needed to validate this planning document itself.

## 9. Risks and impact

The largest risks are accidental dependency cycles, feature unification,
changed Bevy deferred-command/system ordering, and world cleanup happening at a
different stage. Preserve current behavior first; document any intentional fix
as its own tested unit rather than hiding it in file moves.

Consumers include both host binaries, the debug crate, the native dispatcher,
all game tests and dev scripts/docs naming moved APIs. MCP and protocol crates
should remain behaviorally unchanged. Exports and saved preferences are user
compatibility boundaries. Never regenerate or overwrite local preferences as a
migration mechanism. Native media cancellation and same-update audio channel
reservation must survive moving their owning resources.

No build-time or frame-rate improvement is promised. Isolated dependency builds
prove separation; a measured performance claim would require a separate benchmark.

## 10. Files expected to change during implementation

| Existing paths | Destination/change |
| --- | --- |
| `crates/cyrus-game/src/scene.rs`, `loading.rs`, `install.rs`, all `*_export.rs`, `player/export/`, startup export/raster helpers | Assets native preparation; world lifecycle; render progress/scene modules |
| `animation.rs`, `animation/` | Assets records, world constructors/execution, render mesh binding/application |
| `player.rs`, `player/` | Game controller/input; assets records; world geometry/support; render camera |
| `environment.rs`, `lighting.rs`, `sky.rs`, `water.rs`, `fog.rs` | Assets records and render algorithms/plugins |
| `sound.rs`, `sound_bank.rs`, `music.rs`, `subtitles.rs` | Assets native readers/metadata; world intents; render playback |
| `startup/`, `hud.rs`, `settings.rs` | Assets cache records; game runtime preferences; render presentation; host storage |
| `control.rs` | `crates/cyrus-control/src/server.rs` transport and `crates/cyrus/src/control.rs` dispatcher |
| `crates/cyrus-game/src/lib.rs`, `crates/cyrus/src/main.rs`, `crates/cyrus-web/src/main.rs`, `crates/cyrus-debug/src/lib.rs` | Narrow gameplay plugin and common lifecycle/presentation composition |
| Workspace/crate manifests, `Cargo.lock`, `crates/cyrus-text/Cargo.toml` and native font conversion | New crates, minimal Bevy dependencies, opt-in native data conversion |
| Existing tests, crate READMEs, `docs/MAP.md`, `docs/ROADMAP.md`, `docs/SETUP.md`, `AGENTS.md`, `.github/workflows/ci.yml`, affected `dev/scripts/` | Move contract tests, enforce isolated builds, document actual ownership |

## 11. Reusable implementation context

```json
{"implementation_context":{"task_summary":"Split cyrus-game into portable assets, world execution, gameplay and presentation; move native TCP into cyrus-control::server. Plan only.","acceptance_criteria":["Core independent of renderer/audio backend/install parser/sockets","Shared native/browser world and presentation paths","Unchanged cache/settings/protocol behavior","Migrated tests and isolated dependency checks"],"evidence_provenance":{"schema_version":2,"head_commit":"0d6116bca7eeecceea2280b69eaf23e201cd373b","generated_plan_path":"docs/plans/2026-10-11-gitnexus-plan-game-crate-split.md","global_dirty_digest":{"algorithm":"sha256","canonicalization":"gitnexus-evidence-provenance-v2 NUL-framed UTF-8 records","value":"0a9c85780067d9afcd0764f307b60891e3cee927ee11eaeb5ec7826d10fd82cd"},"cited_path_manifest":[{"path":".github/workflows/ci.yml","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:ef1cdfc365bba5dbc1faedbedad5bf263a671c048801ad2d74c183de47d826bd","index_digest":"sha256:ef1cdfc365bba5dbc1faedbedad5bf263a671c048801ad2d74c183de47d826bd","worktree_digest":"sha256:ef1cdfc365bba5dbc1faedbedad5bf263a671c048801ad2d74c183de47d826bd","untracked_digest":"absent"},{"path":"Cargo.toml","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:62706151fd5c55f2f5a8f17ab0732582fe6180fb000167098bd5247902b853b8","index_digest":"sha256:62706151fd5c55f2f5a8f17ab0732582fe6180fb000167098bd5247902b853b8","worktree_digest":"sha256:62706151fd5c55f2f5a8f17ab0732582fe6180fb000167098bd5247902b853b8","untracked_digest":"absent"},{"path":"crates/cyrus-control/Cargo.toml","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:7b80e6976fe20a9b9358534c30dd7f5ae2230849565a9c68c37d392cada11e44","index_digest":"sha256:7b80e6976fe20a9b9358534c30dd7f5ae2230849565a9c68c37d392cada11e44","worktree_digest":"sha256:7b80e6976fe20a9b9358534c30dd7f5ae2230849565a9c68c37d392cada11e44","untracked_digest":"absent"},{"path":"crates/cyrus-control/src/lib.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:0254ae07b7df0d48d4e1edd2b85ddc3b43288d4a140dd73c2d13575887cee9fb","index_digest":"sha256:0254ae07b7df0d48d4e1edd2b85ddc3b43288d4a140dd73c2d13575887cee9fb","worktree_digest":"sha256:0254ae07b7df0d48d4e1edd2b85ddc3b43288d4a140dd73c2d13575887cee9fb","untracked_digest":"absent"},{"path":"crates/cyrus-debug/src/lib.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:7489e30056e6123db14bf814d796bab29842c10a0bb82038f5b16a0e280b1d0e","index_digest":"sha256:7489e30056e6123db14bf814d796bab29842c10a0bb82038f5b16a0e280b1d0e","worktree_digest":"sha256:7489e30056e6123db14bf814d796bab29842c10a0bb82038f5b16a0e280b1d0e","untracked_digest":"absent"},{"path":"crates/cyrus-game/Cargo.toml","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:17821b00d48c29b7572ec7373d41f0a4f5953d4ec4baaa528ac089591a9e836d","index_digest":"sha256:17821b00d48c29b7572ec7373d41f0a4f5953d4ec4baaa528ac089591a9e836d","worktree_digest":"sha256:17821b00d48c29b7572ec7373d41f0a4f5953d4ec4baaa528ac089591a9e836d","untracked_digest":"absent"},{"path":"crates/cyrus-game/src/animation.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:3e7ccc67f4eb0ca10c8b912aa4bdab43c15f21e81a2185a647de4e0d2c074c55","index_digest":"sha256:3e7ccc67f4eb0ca10c8b912aa4bdab43c15f21e81a2185a647de4e0d2c074c55","worktree_digest":"sha256:3e7ccc67f4eb0ca10c8b912aa4bdab43c15f21e81a2185a647de4e0d2c074c55","untracked_digest":"absent"},{"path":"crates/cyrus-game/src/animation/actor.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:9d3f7d5788d5bb663b49773aa904aafb9ce5c4d84f858d2da7ccde1e74ecee24","index_digest":"sha256:9d3f7d5788d5bb663b49773aa904aafb9ce5c4d84f858d2da7ccde1e74ecee24","worktree_digest":"sha256:9d3f7d5788d5bb663b49773aa904aafb9ce5c4d84f858d2da7ccde1e74ecee24","untracked_digest":"absent"},{"path":"crates/cyrus-game/src/animation/frame.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:82a991d6265c53ccc411cc6698d4c658a53234d40294771668dcb1a0984c62c7","index_digest":"sha256:82a991d6265c53ccc411cc6698d4c658a53234d40294771668dcb1a0984c62c7","worktree_digest":"sha256:82a991d6265c53ccc411cc6698d4c658a53234d40294771668dcb1a0984c62c7","untracked_digest":"absent"},{"path":"crates/cyrus-game/src/animation/playback.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:b77792a50751cadd1258bd38646a011f2eadc6ca407689fab2b0daa97266b851","index_digest":"sha256:b77792a50751cadd1258bd38646a011f2eadc6ca407689fab2b0daa97266b851","worktree_digest":"sha256:b77792a50751cadd1258bd38646a011f2eadc6ca407689fab2b0daa97266b851","untracked_digest":"absent"},{"path":"crates/cyrus-game/src/animation/script.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:6a24dd2bac320ca8a46f3ef22e3a468fca105bc7367bccf38c0e9d33ea78a374","index_digest":"sha256:6a24dd2bac320ca8a46f3ef22e3a468fca105bc7367bccf38c0e9d33ea78a374","worktree_digest":"sha256:6a24dd2bac320ca8a46f3ef22e3a468fca105bc7367bccf38c0e9d33ea78a374","untracked_digest":"absent"},{"path":"crates/cyrus-game/src/animation/sequence.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:2d4a5096516a7e4ce85b3c3903cf512f62ab85f911e6452b6191148616601ab5","index_digest":"sha256:2d4a5096516a7e4ce85b3c3903cf512f62ab85f911e6452b6191148616601ab5","worktree_digest":"sha256:2d4a5096516a7e4ce85b3c3903cf512f62ab85f911e6452b6191148616601ab5","untracked_digest":"absent"},{"path":"crates/cyrus-game/src/animation_export.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:08a481d62dc368f375ab645424b8fd94876afec9d41c02cf261433c8cdba06c7","index_digest":"sha256:08a481d62dc368f375ab645424b8fd94876afec9d41c02cf261433c8cdba06c7","worktree_digest":"sha256:08a481d62dc368f375ab645424b8fd94876afec9d41c02cf261433c8cdba06c7","untracked_digest":"absent"},{"path":"crates/cyrus-game/src/control.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:0fce6c533629263479eb24c2578c74dcc8745e93f77ae7e41e9bc14e9e639eb1","index_digest":"sha256:0fce6c533629263479eb24c2578c74dcc8745e93f77ae7e41e9bc14e9e639eb1","worktree_digest":"sha256:0fce6c533629263479eb24c2578c74dcc8745e93f77ae7e41e9bc14e9e639eb1","untracked_digest":"absent"},{"path":"crates/cyrus-game/src/environment.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:b493ac63e5fb294ff5852a8036730015949d26116aaa0f3183ca7b04a36b504c","index_digest":"sha256:b493ac63e5fb294ff5852a8036730015949d26116aaa0f3183ca7b04a36b504c","worktree_digest":"sha256:b493ac63e5fb294ff5852a8036730015949d26116aaa0f3183ca7b04a36b504c","untracked_digest":"absent"},{"path":"crates/cyrus-game/src/hud.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:0006a02ceebef11f402800b17c15b07d5e7090997065e714c7f7704c3919916a","index_digest":"sha256:0006a02ceebef11f402800b17c15b07d5e7090997065e714c7f7704c3919916a","worktree_digest":"sha256:0006a02ceebef11f402800b17c15b07d5e7090997065e714c7f7704c3919916a","untracked_digest":"absent"},{"path":"crates/cyrus-game/src/lib.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:7ae2bcc76bc077fe3428e01c06916ca3cc42c5c86a7e6c8d390f3c43bce4f0bc","index_digest":"sha256:7ae2bcc76bc077fe3428e01c06916ca3cc42c5c86a7e6c8d390f3c43bce4f0bc","worktree_digest":"sha256:7ae2bcc76bc077fe3428e01c06916ca3cc42c5c86a7e6c8d390f3c43bce4f0bc","untracked_digest":"absent"},{"path":"crates/cyrus-game/src/lighting.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:0ff93c0b4ea525dfdd68856913081cd15626ebe60a231ec0bd5e174a19e5156e","index_digest":"sha256:0ff93c0b4ea525dfdd68856913081cd15626ebe60a231ec0bd5e174a19e5156e","worktree_digest":"sha256:0ff93c0b4ea525dfdd68856913081cd15626ebe60a231ec0bd5e174a19e5156e","untracked_digest":"absent"},{"path":"crates/cyrus-game/src/loading.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:7aaac3335ef99159eb4923929d3f805a53263e13c33019275b1d0503b7f34893","index_digest":"sha256:7aaac3335ef99159eb4923929d3f805a53263e13c33019275b1d0503b7f34893","worktree_digest":"sha256:7aaac3335ef99159eb4923929d3f805a53263e13c33019275b1d0503b7f34893","untracked_digest":"absent"},{"path":"crates/cyrus-game/src/player.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:280542a8b131704b29b588dcabccd26961251b43353455247951ba23a38a43ee","index_digest":"sha256:280542a8b131704b29b588dcabccd26961251b43353455247951ba23a38a43ee","worktree_digest":"sha256:280542a8b131704b29b588dcabccd26961251b43353455247951ba23a38a43ee","untracked_digest":"absent"},{"path":"crates/cyrus-game/src/player/collision_data.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:9cb90b238d95a2affa29b5c48ab59421666c69ba63f5f698ece6bee7652eee5b","index_digest":"sha256:9cb90b238d95a2affa29b5c48ab59421666c69ba63f5f698ece6bee7652eee5b","worktree_digest":"sha256:9cb90b238d95a2affa29b5c48ab59421666c69ba63f5f698ece6bee7652eee5b","untracked_digest":"absent"},{"path":"crates/cyrus-game/src/player/collision_mesh.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:bc758b8a1d9244499d9ab3a6c72208ca3c4d35f3215f1f1ef8c01d3c04836f4e","index_digest":"sha256:bc758b8a1d9244499d9ab3a6c72208ca3c4d35f3215f1f1ef8c01d3c04836f4e","worktree_digest":"sha256:bc758b8a1d9244499d9ab3a6c72208ca3c4d35f3215f1f1ef8c01d3c04836f4e","untracked_digest":"absent"},{"path":"crates/cyrus-game/src/player/collision_world.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:cf441aac76bbabce20d660e14421b32cd35fcdb09cd4a97bd38e9bceb62f6a7a","index_digest":"sha256:cf441aac76bbabce20d660e14421b32cd35fcdb09cd4a97bd38e9bceb62f6a7a","worktree_digest":"sha256:cf441aac76bbabce20d660e14421b32cd35fcdb09cd4a97bd38e9bceb62f6a7a","untracked_digest":"absent"},{"path":"crates/cyrus-game/src/player/data.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:d873765d9f4e5703e8dc1ba1c8d598f62a54e030e685735d375af3ba0223ef33","index_digest":"sha256:d873765d9f4e5703e8dc1ba1c8d598f62a54e030e685735d375af3ba0223ef33","worktree_digest":"sha256:d873765d9f4e5703e8dc1ba1c8d598f62a54e030e685735d375af3ba0223ef33","untracked_digest":"absent"},{"path":"crates/cyrus-game/src/player/ground.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:fd63e23dc01992b9efea2473fdfe8ae9d2ffc3d563cfe68ce0060bf5daaa50fc","index_digest":"sha256:fd63e23dc01992b9efea2473fdfe8ae9d2ffc3d563cfe68ce0060bf5daaa50fc","worktree_digest":"sha256:fd63e23dc01992b9efea2473fdfe8ae9d2ffc3d563cfe68ce0060bf5daaa50fc","untracked_digest":"absent"},{"path":"crates/cyrus-game/src/player/input.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:e7dc5db29ba3d6e82196a41c2baf769db9a4c63a9581d2ba9127817444bb088c","index_digest":"sha256:e7dc5db29ba3d6e82196a41c2baf769db9a4c63a9581d2ba9127817444bb088c","worktree_digest":"sha256:e7dc5db29ba3d6e82196a41c2baf769db9a4c63a9581d2ba9127817444bb088c","untracked_digest":"absent"},{"path":"crates/cyrus-game/src/player/placement.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:d64026fee82c4aa3ba7645196c7bb88ed3cebe0986ce49c195ca62754c0285d3","index_digest":"sha256:d64026fee82c4aa3ba7645196c7bb88ed3cebe0986ce49c195ca62754c0285d3","worktree_digest":"sha256:d64026fee82c4aa3ba7645196c7bb88ed3cebe0986ce49c195ca62754c0285d3","untracked_digest":"absent"},{"path":"crates/cyrus-game/src/scene.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:4190e392b86b07efc6ae926b0704d806352c9ca4424d48888f6d43f2eb98545f","index_digest":"sha256:4190e392b86b07efc6ae926b0704d806352c9ca4424d48888f6d43f2eb98545f","worktree_digest":"sha256:4190e392b86b07efc6ae926b0704d806352c9ca4424d48888f6d43f2eb98545f","untracked_digest":"absent"},{"path":"crates/cyrus-game/src/settings.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:b300675e1e1aaca71ef5934197aef9007e001a9e73c5d6ea80d0bf9456c6965e","index_digest":"sha256:b300675e1e1aaca71ef5934197aef9007e001a9e73c5d6ea80d0bf9456c6965e","worktree_digest":"sha256:b300675e1e1aaca71ef5934197aef9007e001a9e73c5d6ea80d0bf9456c6965e","untracked_digest":"absent"},{"path":"crates/cyrus-game/src/sound.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:b144aa911d667315615f5e302e5f23e67486f5ef64a6e57c8d06185ba9fdb976","index_digest":"sha256:b144aa911d667315615f5e302e5f23e67486f5ef64a6e57c8d06185ba9fdb976","worktree_digest":"sha256:b144aa911d667315615f5e302e5f23e67486f5ef64a6e57c8d06185ba9fdb976","untracked_digest":"absent"},{"path":"crates/cyrus-game/src/sound_bank.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:a74ebcf03874a7578f0b052741dc290b303d671b7c7c66c38e7a7e3b572f8ebb","index_digest":"sha256:a74ebcf03874a7578f0b052741dc290b303d671b7c7c66c38e7a7e3b572f8ebb","worktree_digest":"sha256:a74ebcf03874a7578f0b052741dc290b303d671b7c7c66c38e7a7e3b572f8ebb","untracked_digest":"absent"},{"path":"crates/cyrus-game/src/startup/menu.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:9f680bb07c1b5fc42a2bf88a1a8f922df524386a80109104de8d008c5358c370","index_digest":"sha256:9f680bb07c1b5fc42a2bf88a1a8f922df524386a80109104de8d008c5358c370","worktree_digest":"sha256:9f680bb07c1b5fc42a2bf88a1a8f922df524386a80109104de8d008c5358c370","untracked_digest":"absent"},{"path":"crates/cyrus-game/src/startup/menu_data.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:c810215b5c50f2cd81170e7a78e4cb90020513c313530f11fc7031829184cf14","index_digest":"sha256:c810215b5c50f2cd81170e7a78e4cb90020513c313530f11fc7031829184cf14","worktree_digest":"sha256:c810215b5c50f2cd81170e7a78e4cb90020513c313530f11fc7031829184cf14","untracked_digest":"absent"},{"path":"crates/cyrus-game/src/startup/menu_preferences.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:37468c77c50d21323042828d7cf7d0ce040b64ac103eb5b46315b8f9a6b8dd1b","index_digest":"sha256:37468c77c50d21323042828d7cf7d0ce040b64ac103eb5b46315b8f9a6b8dd1b","worktree_digest":"sha256:37468c77c50d21323042828d7cf7d0ce040b64ac103eb5b46315b8f9a6b8dd1b","untracked_digest":"absent"},{"path":"crates/cyrus-game/src/startup/mod.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:7e91f982531d0b89c4e32ad025b9a05924d0fdd8a68ac3f4a498200dca6e2615","index_digest":"sha256:7e91f982531d0b89c4e32ad025b9a05924d0fdd8a68ac3f4a498200dca6e2615","worktree_digest":"sha256:7e91f982531d0b89c4e32ad025b9a05924d0fdd8a68ac3f4a498200dca6e2615","untracked_digest":"absent"},{"path":"crates/cyrus-game/src/startup/movie.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:d65e95f5783c47580dd5025e80da71686e6244dc403191e95d99cca105ec66da","index_digest":"sha256:d65e95f5783c47580dd5025e80da71686e6244dc403191e95d99cca105ec66da","worktree_digest":"sha256:d65e95f5783c47580dd5025e80da71686e6244dc403191e95d99cca105ec66da","untracked_digest":"absent"},{"path":"crates/cyrus-game/src/startup/movie_data.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:73db7d27b484b918440829cb09d50694ed05375469177a468b7b6ab177e39642","index_digest":"sha256:73db7d27b484b918440829cb09d50694ed05375469177a468b7b6ab177e39642","worktree_digest":"sha256:73db7d27b484b918440829cb09d50694ed05375469177a468b7b6ab177e39642","untracked_digest":"absent"},{"path":"crates/cyrus-game/src/startup/movie_native.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:d7fe443009b94d46d8d07145036d758cc1ab76c3b01612af42e5d347973bed20","index_digest":"sha256:d7fe443009b94d46d8d07145036d758cc1ab76c3b01612af42e5d347973bed20","worktree_digest":"sha256:d7fe443009b94d46d8d07145036d758cc1ab76c3b01612af42e5d347973bed20","untracked_digest":"absent"},{"path":"crates/cyrus-game/tests/animation.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:f4294e2966ad8be66d85b63d7cb87756297280faf82f8c0afbf177ff3cf8379c","index_digest":"sha256:f4294e2966ad8be66d85b63d7cb87756297280faf82f8c0afbf177ff3cf8379c","worktree_digest":"sha256:f4294e2966ad8be66d85b63d7cb87756297280faf82f8c0afbf177ff3cf8379c","untracked_digest":"absent"},{"path":"crates/cyrus-game/tests/animation_script_sounds.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:8e12e1891043b6d0ceb4e187a84d905882c426cb6d08c5316ef7ce3a4e597f66","index_digest":"sha256:8e12e1891043b6d0ceb4e187a84d905882c426cb6d08c5316ef7ce3a4e597f66","worktree_digest":"sha256:8e12e1891043b6d0ceb4e187a84d905882c426cb6d08c5316ef7ce3a4e597f66","untracked_digest":"absent"},{"path":"crates/cyrus-game/tests/install.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:d618beda2cd0b9ca772e5ce5ccc9ba564997813545332ac81f637431c5ba1db5","index_digest":"sha256:d618beda2cd0b9ca772e5ce5ccc9ba564997813545332ac81f637431c5ba1db5","worktree_digest":"sha256:d618beda2cd0b9ca772e5ce5ccc9ba564997813545332ac81f637431c5ba1db5","untracked_digest":"absent"},{"path":"crates/cyrus-game/tests/menu.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:e9937795e61cd4362a0528a1ac04049722bddf995cdb3fbed99dfb5a36c614f9","index_digest":"sha256:e9937795e61cd4362a0528a1ac04049722bddf995cdb3fbed99dfb5a36c614f9","worktree_digest":"sha256:e9937795e61cd4362a0528a1ac04049722bddf995cdb3fbed99dfb5a36c614f9","untracked_digest":"absent"},{"path":"crates/cyrus-game/tests/player.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:06f772f3981c48691dd9ce84e2f697d02baed8923062c89fb063e744a2f8c926","index_digest":"sha256:06f772f3981c48691dd9ce84e2f697d02baed8923062c89fb063e744a2f8c926","worktree_digest":"sha256:06f772f3981c48691dd9ce84e2f697d02baed8923062c89fb063e744a2f8c926","untracked_digest":"absent"},{"path":"crates/cyrus-game/tests/player_collision_mesh.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:c77e724ef6a3b3eafe06c45b4ba863891a69a11927043c684adfa0b889c197b2","index_digest":"sha256:c77e724ef6a3b3eafe06c45b4ba863891a69a11927043c684adfa0b889c197b2","worktree_digest":"sha256:c77e724ef6a3b3eafe06c45b4ba863891a69a11927043c684adfa0b889c197b2","untracked_digest":"absent"},{"path":"crates/cyrus-game/tests/player_collision_world.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:fd76cea4cb2191d5559969feb39116d6654deddc7ee43db2605fa087b4e93570","index_digest":"sha256:fd76cea4cb2191d5559969feb39116d6654deddc7ee43db2605fa087b4e93570","worktree_digest":"sha256:fd76cea4cb2191d5559969feb39116d6654deddc7ee43db2605fa087b4e93570","untracked_digest":"absent"},{"path":"crates/cyrus-game/tests/player_export.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:5fc1647ff81b7bc310515b838205b04ac7942bc480ec99f345aece1c6eecfe65","index_digest":"sha256:5fc1647ff81b7bc310515b838205b04ac7942bc480ec99f345aece1c6eecfe65","worktree_digest":"sha256:5fc1647ff81b7bc310515b838205b04ac7942bc480ec99f345aece1c6eecfe65","untracked_digest":"absent"},{"path":"crates/cyrus-text/Cargo.toml","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:c2622fa68dd025a365cb778b748bb48319b911303a617e1ac2357f435baf5cf4","index_digest":"sha256:c2622fa68dd025a365cb778b748bb48319b911303a617e1ac2357f435baf5cf4","worktree_digest":"sha256:c2622fa68dd025a365cb778b748bb48319b911303a617e1ac2357f435baf5cf4","untracked_digest":"absent"},{"path":"crates/cyrus-web/src/main.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:4e726933091324696507a9cae9f9dd9c005df4b0530377197fc79c11a4d95403","index_digest":"sha256:4e726933091324696507a9cae9f9dd9c005df4b0530377197fc79c11a4d95403","worktree_digest":"sha256:4e726933091324696507a9cae9f9dd9c005df4b0530377197fc79c11a4d95403","untracked_digest":"absent"},{"path":"crates/cyrus/src/main.rs","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:03d42193a720999df02d848db5537304c2aad87552f088533e95c1f7da511f0d","index_digest":"sha256:03d42193a720999df02d848db5537304c2aad87552f088533e95c1f7da511f0d","worktree_digest":"sha256:03d42193a720999df02d848db5537304c2aad87552f088533e95c1f7da511f0d","untracked_digest":"absent"},{"path":"dev/scripts/build-web.sh","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:7c7777927d73e79ed5dd6d246b33e49f7aad08047759114e9b59949ffd9fc08d","index_digest":"sha256:7c7777927d73e79ed5dd6d246b33e49f7aad08047759114e9b59949ffd9fc08d","worktree_digest":"sha256:7c7777927d73e79ed5dd6d246b33e49f7aad08047759114e9b59949ffd9fc08d","untracked_digest":"absent"},{"path":"dev/scripts/gates.sh","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:8525bfd96bf50afa6de7d87c587e90a911a8458383fdf96eb3b476cf9b60c3e5","index_digest":"sha256:8525bfd96bf50afa6de7d87c587e90a911a8458383fdf96eb3b476cf9b60c3e5","worktree_digest":"sha256:8525bfd96bf50afa6de7d87c587e90a911a8458383fdf96eb3b476cf9b60c3e5","untracked_digest":"absent"},{"path":"docs/MAP.md","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:f917d882125923f048d535fc7344f01971615b003be674d3779f2b3bd1068dda","index_digest":"sha256:f917d882125923f048d535fc7344f01971615b003be674d3779f2b3bd1068dda","worktree_digest":"sha256:f917d882125923f048d535fc7344f01971615b003be674d3779f2b3bd1068dda","untracked_digest":"absent"},{"path":"docs/METHOD.md","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:d082d45be98c12f8ad2f4b1211e2336a5be502d5bed09512e291ecdfca020d48","index_digest":"sha256:d082d45be98c12f8ad2f4b1211e2336a5be502d5bed09512e291ecdfca020d48","worktree_digest":"sha256:d082d45be98c12f8ad2f4b1211e2336a5be502d5bed09512e291ecdfca020d48","untracked_digest":"absent"},{"path":"docs/ROADMAP.md","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:6aac02687551b7d23ac36c0f66b58e42ff6156959a3ca85b61bd97ef965f6d21","index_digest":"sha256:6aac02687551b7d23ac36c0f66b58e42ff6156959a3ca85b61bd97ef965f6d21","worktree_digest":"sha256:6aac02687551b7d23ac36c0f66b58e42ff6156959a3ca85b61bd97ef965f6d21","untracked_digest":"absent"},{"path":"docs/SETUP.md","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:99cb892d2117129c85ce51059eed2d0ace0a521474128595e0c2ac31a777a4f5","index_digest":"sha256:99cb892d2117129c85ce51059eed2d0ace0a521474128595e0c2ac31a777a4f5","worktree_digest":"sha256:99cb892d2117129c85ce51059eed2d0ace0a521474128595e0c2ac31a777a4f5","untracked_digest":"absent"},{"path":"docs/WORKFLOW.md","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:340ec7e3c064f8fae4c72dc5e36c164d5a6fd6c47b50b362e7bb5e7d7555731f","index_digest":"sha256:340ec7e3c064f8fae4c72dc5e36c164d5a6fd6c47b50b362e7bb5e7d7555731f","worktree_digest":"sha256:340ec7e3c064f8fae4c72dc5e36c164d5a6fd6c47b50b362e7bb5e7d7555731f","untracked_digest":"absent"},{"path":"justfile","object_kind":{"head":"regular","index":"regular","worktree":"regular","untracked":"absent"},"state":"clean","rename_from":null,"rename_to":null,"head_digest":"sha256:88a4e70ee6ad59836bd2da070b49a7aaad298a92fb09c91f2afdf74e4bc55a9e","index_digest":"sha256:88a4e70ee6ad59836bd2da070b49a7aaad298a92fb09c91f2afdf74e4bc55a9e","worktree_digest":"sha256:88a4e70ee6ad59836bd2da070b49a7aaad298a92fb09c91f2afdf74e4bc55a9e","untracked_digest":"absent"}]},"primary_symbols":[{"symbol":"GamePlugin","file":"crates/cyrus-game/src/lib.rs","lines":"52-76","role":"native assembly to narrow"},{"symbol":"load_requested","file":"crates/cyrus-game/src/scene.rs","lines":"70-124","role":"split preparation, lifetime and rendering"},{"symbol":"Animations","file":"crates/cyrus-game/src/animation.rs","lines":"20-38","role":"portable record separated from runtime constructors"},{"symbol":"PlayerPlugin","file":"crates/cyrus-game/src/player.rs","lines":"57-90","role":"separate camera and binding IO"},{"symbol":"MenuPreferences","file":"crates/cyrus-game/src/startup/menu_preferences.rs","lines":"9-93","role":"preserve storage and runtime commit semantics"}],"related_symbols":[{"symbol":"AnimationPlayback","relationship":"consumes Animations; emits pose and sound","relevance":"supplied ticks and same-update dispatch"},{"symbol":"SoundBank","relationship":"serialized metadata plus runtime audio handles","relevance":"remove renderer dependency from records"},{"symbol":"PlayerScene","relationship":"embeds terrain/definition/collision objects","relevance":"split data validation from queries"},{"symbol":"MenuAsset","relationship":"embeds MenuItem and MenuPreferences","relevance":"defaults must not import gameplay"},{"symbol":"handle","relationship":"control dispatcher consumes most domains","relevance":"move to native host"}],"execution_path":["Native resolves/export selection before scene replacement","World accepts retained animation references and player/graphics paths; game prepares PlayerScene asynchronously","Gameplay consumes player data; presentation spawns shared scene/camera","Menu edits persist before apply; animation emits cues before mixer consumes them"],"pdg_constraints":[{"description":"load_requested returns for unchanged name and export failure; headless exits before graphics","affected_statements":["crates/cyrus-game/src/scene.rs:81","crates/cyrus-game/src/scene.rs:85","crates/cyrus-game/src/scene.rs:101"],"implementation_consequence":"keep failure/no-op/headless outcomes distinct"},{"description":"Playback reuses supplied ticks in each execution stage","affected_statements":["crates/cyrus-game/src/animation/playback.rs:65"],"implementation_consequence":"no inferred clock or dropped zero-tick updates"}],"architectural_patterns":[{"pattern":"Bevy plugin/resource/message composition","example_location":"crates/cyrus-game/src/animation.rs:155","usage_guidance":"separate loader, execution and presentation plugins; use public sets for ordering"},{"pattern":"preserve state on persistence failure","example_location":"crates/cyrus-game/src/startup/menu.rs:397","usage_guidance":"synchronous storage before runtime apply"}],"files_to_modify":[{"file":"crates/cyrus-game/src/","symbols":["GamePlugin","Animations","PlayerPlugin","MenuPreferences"],"intended_change":"extract owners described in sections 6 and 10"},{"file":"crates/cyrus-assets/","symbols":[],"intended_change":"new portable records/native IO"},{"file":"crates/cyrus-world/","symbols":[],"intended_change":"new lifetime, animation and collision execution"},{"file":"crates/cyrus-render/","symbols":[],"intended_change":"new shared presentation"},{"file":"crates/cyrus-control/src/server.rs","symbols":[],"intended_change":"new std-only native server module"},{"file":"crates/cyrus/src/main.rs","symbols":["main"],"intended_change":"native composition and adapters"},{"file":"crates/cyrus-web/src/main.rs","symbols":["main","spawn_scene"],"intended_change":"browser composition; remove duplicate spawning"},{"file":"crates/cyrus-debug/src/lib.rs","symbols":["DebugMenuPlugin"],"intended_change":"use public domain APIs and host world options"}],"tests":[{"file":"crates/cyrus-game/tests/animation.rs","scenarios":["sidecar load","explicit tick playback","mesh restoration","world replacement"]},{"file":"crates/cyrus-game/tests/player.rs","scenarios":["held opposing inputs","flycam isolation"]},{"file":"crates/cyrus-game/tests/player_export.rs","scenarios":["installed schema and ground values"]},{"file":"crates/cyrus-game/tests/menu.rs","scenarios":["data-defined menu entries and exported sound bank"]},{"file":"crates/cyrus-world/tests/lifecycle.rs","scenarios":["new test: failed preparation retains active world","new test: stale sidecar cannot overwrite newer request"]}],"verification_commands":["just genmap","just gates","cargo check -p cyrus-web --target wasm32-unknown-unknown","just web-build","just smoke-mcp","cargo run -p cyrus -- --export-all-worlds","cargo run -p cyrus -- --export-startup"],"risks":["Bevy feature unification","deferred commands and same-update ordering","native/browser lifecycle drift","consumer-specific malformed preference policies"],"assumptions":["Recheck HEAD and cited bytes before implementation","Resolve exact minimal Bevy dependencies through isolated builds","Capture existing headless control and browser reload behavior before changing adapters","Preserve existing browser movie playback and sidecar binding initialization"],"open_questions":["Which active domain increments land before each move?","Do baseline exports have nondeterministic byte ordering? Compare parsed content where necessary."],"avoid":["Do not implement gameplay clock or actor activation","Do not change cache/settings/protocol formats","Do not revive archived split wholesale","Do not create a catch-all common crate","Do not retain old API re-exports after caller migration","Do not interpret missing graph edges as absent Bevy consumers"]}}
```

## 12. Assumptions and open questions

- [assumed] Current main remains the migration baseline. Recheck commit and
  cited-file digests before execution; active domain branches may move these
  exact files. Coordinate ownership before moving them.
- [assumed] Three new crates are the appropriate current scale. Their internal
  modules must have enforced visibility; crate count alone does not fix coupling.
- Exact minimal Bevy 0.20 feature/subcrate lists require isolated compile checks
  during implementation. No build was run to prove the proposed manifests.
- Current headless rendering-only control responses and browser scene-picker
  reload behavior need baseline capture before choosing a common host adapter.
  Preserve them during extraction; in-page browser switching is separate work.
- A fully transactional asset/world swap, production simulation clock, complete
  actor activation/SOUP VM, player locomotion, combat, inventory and saves
  are deferred. Existing browser movies are part of the migration baseline. Roadmap stages identify their eventual owners, but this
  split must not create stubs or mark those features complete.
- Retain the current JSON metadata where it is adequate. A complete typed rewrite
  of animation/script schemas is not required to separate cache ownership from
  execution. Format discoveries still belong in preservation.

## 13. Definition of done

The split is complete when portable assets/world/game compile and pass their
contracts independently without renderer, audio backend, native install parser
or sockets; both frontends consume one world/presentation path; all current
exports, saved settings and protocol behavior remain compatible; existing and
new boundary checks pass; and mixed legacy modules/re-exports are removed.

Roadmap ownership, generated module map and crate documentation describe the
actual tree. Missing gameplay and unverified fidelity remain explicit. This
plan is complete once source/graph evidence, ownership, migration order and
checks are recorded; implementation is a subsequent task.

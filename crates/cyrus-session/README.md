# cyrus-session

Shared assembly of gameplay, audio and presentation replacements for native and browser hosts.
Storage adapters supply bytes and prepare installed assets; this crate does no I/O.
It depends on the state owners, which never depend on this assembly crate.

The runtime snapshot composes current and visited logical worlds, HUD, clock,
RNG, toggles, fidelity modes, player placement and explicit-update state, sample playback and
retained dialogue preparation, CD source transport, optional rendered HUD state, presentation-clock epoch, camera/framing state, sky continuation, texture deadlines/holds, terrain history/private meshes and per-world environment edits. Reconstruction uses a logical identity plan before
binding audio and player presentation. Authored type-six light state belongs to each
logical world; staged scene materials use the saved current world's enabled lights. No scripts, initialization or playback run
during construction. Settled lifecycle capture validates prepared script bytes against
the logical source, retains request/current identity and any scene-load error, and
rejects in-flight preparation. Player replacements also retain the displayed
model/frame and sound surface before the first gameplay update.

This draft provides an exclusive commit and document encoding for its represented
owners and diagnostic host load handlers, but does not yet provide a complete accepted save.
Host preparation, complete source identity and lifecycle admission still need
native/browser publication and restart/revisit acceptance. Source-decoder snapshots
do not retain downstream resampler or device queues.

`encode_document()` returns one UTF-8 byte document. `decode_document()` checks
the format version and exact payload digest before decoding. Hosts can measure
the returned byte length directly and publish it using existing opaque storage.
Decoding performs no I/O and is not source or gameplay validation. The returned
snapshot must still pass `plan`, `finish` and the exclusive `commit` preflight.
The digest detects corruption, not malicious modification. Hosts own input-size
limits. This format is a cyrus document, not the original game's save format.

`SessionWorldAssets::request` accepts host-resolved cache handles for exactly the
saved world census and retains player sidecar requests for each world. It does
not insert `PreparedWorld`, configure HUD state or initialize actors. `sources`
reports pending loads separately from contextual failures and supplies the
decoded current/visited sources to `plan` only when every owner is available.
Native hosts must finish cache export first; browser hosts resolve the same
installed world IDs against their cache service. Render and audio staging remain
separate preparation steps before `finish`.
`SessionWorldAssets::for_capture` prepares the same sources from the live
registry's current/visited census when no saved document exists yet. Capture
accepts `&World`, so its source references can borrow that same world's asset
stores. The host still pauses simulation and drains deferred work before capture;
read-only ECS access alone does not freeze audio decoder threads.

`SessionEntityStage::prepare` matches saved map bindings against the hidden
prepared scene and allocates unbound standalone actor targets. Headless sessions
receive named map targets without loading geometry. `bindings()` and `player()`
feed the existing preparation contract. `discard()` removes only standalone
targets still owned by that stage; commit releases those markers. Prepared scene
and asset cleanup remain with their owner. No script, animation or player update
runs as part of entity staging.

`SessionSceneInstances::request` loads hidden world/model instances and creates an
inactive staging camera without inserting entry resources. `ready()` checks
instance completion and loader errors; `discard()` removes only unpublished
roots owned by that stage. `prepare_sky` uses the normal renderer's CPU asset
builder, allocates private sky/sun images and a hidden triangle, and exposes them
for `SceneReplacement` and `SessionServices`. `prepare_textures` binds an independent timeline owner under the hidden world
and model roots through the ordinary renderer binding rules. Staging roots
exclude their descendants from ordinary texture binding, fog conversion and
terrain binding until publication. This prevents the live environment from
preparing replacement geometry.
Terrain preparation joins separately. `cyrus_render::fog::prepare_scene_materials`
then validates and builds private destination materials using the ordinary model,
sampling and fog rules. Supply the saved camera position, fidelity and clipping;
run texture binding first because material conversion removes standard materials.
`SessionSceneInstances::prepare_materials` takes these values directly from the
saved session. Render playback retains both original clipping and the optional
Modern far-distance override and publishes them together.
Ordinary lighting refresh excludes staging roots and adopts their lighting only
after coordinated publication. Candidate assembly and commit both recheck the
complete mesh/material/image census and reject unconverted source materials. Camera publication restores the captured active
state, and aggregate preflight rejects an already-active staging camera.
`prepare_playback` resolves saved terrain paths, prepares flat geometry, binds
textures, prepares destination materials and binds private terrain in that order,
returning `PreparedRenderPlayback`. Discard the hidden stage after
a preparation failure; live session resources are unchanged.
`prepare_terrain` binds a complete explicit target census under the hidden root
using the ordinary renderer's mesh preparation. It allocates private meshes only
after all sources validate and does not advance terrain history. Use `RuntimeSessionSnapshot::terrain_bindings` to resolve saved child-ordinal
paths beneath the exact-source replacement root. This preserves terrain order
across fresh entity generations. Candidate assembly rejects supplied bindings
that differ from those paths; the terrain constructor also checks source and
mapping fingerprints.

`SessionServices` supplies prepared audio sources, dialogue assets, identity bindings
and HUD images. A saved rendered HUD requires its replacement image census; a
headless snapshot requires that census to be absent. Construction does not discard
saved presentation or synthesize fresh HUD timers. The render dependency belongs
here so neither rendering nor audio needs to depend on the other owner.

Rendered candidates require a `SceneCandidate` whose world instance has finished
preparation and whose asset handles match the chosen lifecycle. Scene adoption
suppresses first-update recreation; it does not provide storage or a live load
handler by itself. Publication must also bind every retained actor-backed entity
before audio owners become live.

`entity_bindings()` exposes the saved actor-backed entity census for host
preparation. Bindings validate actor identity, role and exported map-node names;
standalone alternate-player bindings retain their lifecycle marker. Audio owners
without a retained actor binding reject capture instead of being rebound by guess.

The candidate owns private replacement fields; callers cannot exchange validated
owners between preparation and publication. Its publication preflight rechecks fresh actor generations, reserved
entity existence and ownership, sample allocation freshness, scene readiness and
CD asset storage before mutation. Host-supplied entity bindings must exactly
match the saved census. The exclusive commit reruns preflight before retiring old actor and playback entities, then applies all represented replacements without an intervening system update.

Prepared scene roots remain hidden until publication. The scene candidate rejects
replacement roots, sky or camera entities beneath any retiring scene owner. Map
actor bindings must descend from the prepared replacement root, including when
rechecked at publication. This prevents retiring the old hierarchy from deleting
newly restored owners.

`RuntimeSessionCandidate::commit(&mut World)` publishes the logical registry,
player placement/runtime and sidecar adoption, actor presentations, sample and CD
playback, dialogue, lifecycle and prepared presentation owners. Capture requires an active gameplay boundary. Commit also accepts Menu with no
pending state transition, marks restored entry and requests InGame after publication. A rejected preflight preserves the
live session. Host load/menu admission and rendered preparation acceptance
remain required before this is a complete user-facing save/load path.

Explicit inspection `ScriptedPlayback` components are not part of the logical
registry snapshot. Capture rejects their presence rather than losing their
continuation. Normal actor animation remains owned by the retained registry.

`PreparedRenderPlayback` supplies independently bound texture and terrain owners
and explicit saved-ordinal terrain entity bindings. Reconstruction preserves the
shared presentation epoch. Publication restores private mesh contents, visibility
and texture admission together; preflight rejects shared live terrain handles.
Animated frame bytes and decoded dimensions/format are fingerprinted from the
bound scene and rechecked against prepared assets before commit. The world scene source is fingerprinted by `SceneIdentityPlugin` during the
actual GLTF decode, including embedded binary and loader settings. External
references reject identity capture. The separate player-model candidate retains
its scene identity, displayed pose, transform and visibility. Capture checks its
source path and loaded handle against the prepared world; publication adopts the
prepared model without another load. Complete rendered host preparation still
requires acceptance testing.

Camera preparation uses a fresh `Camera3d` entity without a live `ViewCamera`
marker. Commit retires old view cameras and marks the prepared target live.
Candidate ancestry under retiring cameras rejects before publication.

CPU-only preparation hosts can register `cyrus_render::fog::SceneMaterialAssetsPlugin`
to provide the same material stores without renderer systems. Windowed hosts
already provide those stores through their material plugins.

`RuntimeSessionSnapshot::prepare_environments` accepts owned installed environments
for the exact saved current/visited census and applies validated retained edits
to those off-live values. Use these values for sky, terrain and material preparation
and pass the same values to candidate assembly. Never apply edits to live assets
as part of load preparation.

Use `SceneReplacement::prepare_with_edits` when publishing a private edited
environment. It compares that asset against the installed source with validated
saved edits applied, then rechecks its exact contents before commit. The ordinary
`prepare` path continues requiring the original installed asset handle.

Both game hosts register `SessionBoundaryPlugin`. Host adapters submit loaded
`SessionWorldAssets` and a HUD handle through `SessionBoundary::capture`, or a
fully assembled candidate through `SessionBoundary::commit`. The exclusive Last
system executes after ordinary Update/PostUpdate work. `take_result` returns the
captured snapshot or commit result; an unread result prevents another submission.
Capture rejects pending sources rather than silently waiting while gameplay
changes. Hosts encode the returned snapshot and publish through their existing
storage helper. This boundary alone does not freeze audio threads, implement
original save eligibility, or provide installed current-and-visited acceptance.

`SessionBoundary::capture_live()` selects the renderer-bound installed HUD and
uses `SessionSourceCatalog`, which
retains cache references and the loaded player sidecar handle observed at settled frame boundaries. It selects exactly
the registry's current and visited world census and rejects missing historical
sources. A host preparing a saved session can retain its resolved world handles
through `SessionSourceCatalog::retain`, supplying the corresponding player handle. Live capture clones these retained handles without starting new sidecar loads. This catalog owns source references only;
it neither initializes a world nor retains dormant audio/dialogue services.

Menu-to-game restoration marks `RestoredSessionEntry` through the exit/entry
schedules. Menu cleanup discards its old timing checkpoint rather than applying it
to the replacement clock. Player and camera entry preserve already-published
owners. The marker is removed at the end of the first gameplay frame. Host load
admission and complete rendered startup-transition acceptance remain outstanding.

`SessionSceneInstances::prepare_rendered` assembles private sky assets, validates
the scene against installed sources plus saved edits, then prepares flat,
texture, material and terrain playback. It returns the scene candidate and playback
owners for `SessionServices`; actor and audio preparation remain separate. Supply
the resolved current world ID and the same private environment used for aggregate
assembly. On failure discard the stage rather than reusing partial staging work.

Cheat snapshots retain every decoded raw slot, including unnamed slots, and validate
the slot layout before replacement. Publication preserves the host's existing CHT
store without loading or saving it. Failed cheat initialization rejects capture or
replacement rather than silently substituting fresh state.

Native control exposes `session_capture`, followed on a later frame by
`session_capture_result`. Successful retrieval returns `document` as UTF-8 and
`bytes` as its encoded byte length. Retrieval consumes the result. No file is
written. With browser `?capture`, write a JSON string to
`window.__cyrusSessionRequest`: `{"type":"capture","id":1}`, then
`{"type":"result","id":2}` on a later frame. Read
`window.__cyrusSessionResult`. These are diagnostic capture hooks, not original
menu save handlers or evidence of complete installed restoration.

`SessionServices::sources` owns the complete `SessionWorldAssets` prepared for the
load. `from_loaded` accepts a complete already-loaded census; `request` remains
available for cache paths. Assembly compares every world's complete registry source identity, including
collision and player content, with the logical plan, and publication rechecks those
assets before mutation. Successful commit replaces the source catalog with this
same complete census so subsequent captures retain restored visited-world sources.

Worlds with bound environmental effect pools retain their installed environment
handles alongside player sources. Captures save slot order, descriptors, positions
and lifetime frames. Replacement derives image metadata and lifetime targets from
the exact prepared source; missing or changed sources and malformed slots reject
before publication. `SessionWorldAssets::from_loaded` requires environment handles
for exactly the saved pool census. Pool-free documents retain their prior source
identity. This retention does not prepare effect presentation assets.
The current prepared world and player handle come from that census.

Capture and commit reject `WorldPreparationPending`. Completed loading-bar
`PreparationProgress` may remain present and does not block a settled capture.

## Unfinished session checkpoint

The shared session implementation is committed as work in progress. It includes
current/visited-world documents, source validation, replacement candidates,
render/audio staging, boundary capture/commit and asynchronous load preparation.
Native and browser diagnostic capture hooks are present. This does not enable
accepted end-to-end save/load.

The shared-load integration test passes after separating its fixture from the
late plugin-registration check, which reinitializes source asset storage. Focused
public tests cover delayed-asset waiting, commit, malformed-candidate rejection
without live replacement, and direct-candidate/audio continuation. These synthetic
contracts do not establish complete installed rendered load acceptance. The
full workspace gate for runtime checkpoint `66340ca` passed 947 Rust test
executions, with zero failures, ignored tests or installed-data skips.

The native diagnostic load caller prepares installed current and visited sources
off-live. Its malformed-document host test passes without live replacement.
Terminal native load results remain available until explicitly acknowledged.
An installed diagnostic load reached ready ISLAND gameplay from START and
recaptured current ISLAND plus visited START. Retained START matches after
actor-generation rebinding. Complete rendered continuation and revisit execution
remain unverified. The separately owned browser storage caller is integrated.
Remaining work includes installed live restoration, restart/reload and
visited-world continuation. The revisit draft validates retained actors and clears shared sound services
before traversal; hosts without that restoration still reject the update.
Installed ISLAND-to-START revisit preserves retained task identities and player
records while invocation counters continue; departed ISLAND remains unchanged.
Pre-existing controller/combat update errors remain, so complete gameplay revisit
acceptance is still open. The return to ISLAND preserves all 975 actor records
exactly under fresh generation binding and leaves dormant START unchanged;
pre-existing update failures persist. Audio snapshots cover source-decoder continuation, not downstream
converter/device queues. Complete gameplay and controller acceptance remain open.

The current draft also captures ordinary actor model replacements by saved actor
entity. Hidden staging loads each selected source before texture/material preparation.
`SessionServices::actor_model_roots` supplies those roots for direct assembly; the
shared asynchronous load path supplies them automatically. Missing displayed models,
aliased roots and source/pose mismatches reject before publication. Model candidates
publish prepared frame geometry in the same commit. This closes a represented-owner
gap. Installed native load and recapture retain all 173 ordinary model replacements
with matching displayed poses and unique roots; dormant START matches after fresh
actor-generation binding. Installed missing-model and mismatched-frame candidates
reject without replacing the logical session, entity identities or captured
non-presentation owners. Presentation state continues advancing. Pixel comparison
and complete installed revisit acceptance remain pending.

Capture compares shared immutable source references without serializing the same
asset twice. Separately decoded sources still require complete content comparison;
saved source fingerprints and document compatibility are unchanged.

Logical load planning validates each represented world's source fingerprint once
and retains immutable source borrows through reconstruction. Aggregate planning
reuses those checked fingerprints. Direct world restores still validate sources,
and assembly/publication independently recheck live asset content before mutation.

An eligible book owns `PausedGameplay` while its gameplay camera is suspended.
Capture admits this menu context, retains the gameplay clock from book entry,
and records each camera's pre-book activation instead of its temporary hidden state.
Startup and terminal death books have no capture admission.

`SessionBoundary::load_saved_game` applies the original saved-world audio cleanup
at publication. It clears RTX/dialogue owners and stops sample transports while
retaining native task counters and remembered handles. The next sound maintenance
sample clears the restoration reset flag. A load into the current world preserves
its scene request and live CD transport. A load into another world lets ordinary
world music selection act on the live track. These rules follow preservation's
`docs/engine/sound.md` and `docs/engine/music.md`. Named diagnostic `load` and
candidate `commit` continue restoring exact source-decoder checkpoints.

`SessionBoundaryInspection` provides an opt-in read-only staging census: pending
operation, private entity/render-root/camera counts, active live camera count, and
existing staged asset paths/load states. Calling `snapshot(false)` performs no
census. Hidden render roots identify source-ready render staging before logical
planning, not a publishable replacement. Inspection never consumes a result.

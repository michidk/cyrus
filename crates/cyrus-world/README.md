# cyrus-world

World lifecycle, explicit animation execution and terrain/static collision queries,
with a one-way dependency on `cyrus-assets`. No renderer, camera, audio backend,
install parser or sockets are required.

`PreparedWorld` retains host-selected cache references. `WorldPlugin` commits
identity and animation inputs in `WorldChange`; game and presentation consume the
same request. `CurrentScene.world` retains the resolved WORLD.INI ID separately from the
request name. `WorldMetadataStatus` reports animation metadata loading and
errors; neither identity nor metadata readiness implies that graphics or player
sidecars are ready.

`AnimationPlugin` loads animation data and advances explicitly attached playback
only for supplied `AnimationTick` messages. `FrameSequence::from_asset`,
`ActorAnimation::from_asset` construct runtime
state from records. No production clock, actor activation or unsupported script
operation is inferred. Pose application belongs to `cyrus-render`.

World defines `SoundCue` and `SoundSurface`; the audio consumer registers the cue
queue. Without that consumer, sound-bearing animation remains explicitly
unsupported. `AnimationSystems` lets composition preserve same-update ordering.

Collision queries retain original units, traversal order and contextual errors.
Player-specific forward admission is in `cyrus-game::player::support`.
Run `cargo test -p cyrus-world` for deterministic execution and geometry contracts.

`AnimationPlayback` consumes an explicitly selected forward group and finite
loop count. `AnimationTick` supplies the original whole nominal-tick increment;
render updates alone do not advance playback. The interpreter follows
preservation's [animation facts](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/animation.md).
BreakPoint markers retain the documented walking-translation gate until the
cursor advances or a taken loop/exit jump clears it. DelayCounter holds the prior
pose while its byte delay consumes supplied nominal ticks. Zero continues
immediately and byte 255 waits until replacement. A waiting group remains current
and does not complete its PushAnimation task. These rules are
[published in preservation](https://github.com/michidk/redguard-preservation/blob/515e62f/docs/engine/animation.md#forward-animation-delays).
Unsupported cues stop playback with a contextual error.

`ActorAnimation` is the separate request core for forward types zero and one. The caller supplies the default group
and nominal ticks. Ordinary requests
replace one pending group by the original numeric priority; forced requests
bypass that priority. End selects a pending or default group in the same call,
reusing its tick input. `set_loop_count` supplies the finite count when a
PushAnimation task becomes current. Returning to the same default keeps the
group identity, so a finite count alone does not prove the waiting task completed.
Type-one groups keep their exit tail when the pending group is the default.
After an exit marker, a pending non-default can replace them on a later advance,
with delays checked first. These rules are
[published in preservation](https://github.com/michidk/redguard-preservation/blob/75cba23/docs/engine/animation.md#forward-type-one-transitions).
`request_with_sounds`, `force_request_with_sounds`, and `advance_with_sounds` accept a synchronous handler for
signed effect requests, rate offsets in hertz, End stops and replacement handle
resets. The handler owns the original surface remapping, positional gains and
channel allocation. Without it, sound commands still fail explicitly. Audible
original walking cues and automatic runtime scheduling remain unverified. The
[sound facts](https://github.com/michidk/redguard-preservation/blob/a0e7b16/docs/engine/sound.md#animation-playsound)
define this boundary.

SOUP execution belongs to `cyrus-script`; game-owned hosts and explicit scripted
playback belong to `cyrus-game::script`. `cyrus-world` supplies animation and
sound services without depending on either executor or gameplay handlers.
`PreparedWorld` also retains the independently exported `.scripts` asset.

Animated i16 frame
interpretation, mesh replacement across models, automatic actor activation,
complete script scheduling, state-dependent default selection, other group types,
cancellation requests and the host-clock driver remain
unsupported. These APIs do not establish matched original-game playback.

**Verified:** the independent polygon-segment and ground-support selection cores
follow [preservation PR #27](https://github.com/michidk/redguard-preservation/pull/27).
They retain distinct terrain/model rank scales and replace earlier equal-ranked
contacts. Callers still supply admission, transforms and traversal order; these
cores do not activate gameplay collision. Nearest-even segment conversion follows
the documented default rounding evidence; a live halfway-boundary comparison
remains unchecked.

World exports also write a `.player` sidecar containing installed movement
settings, named CYRUS attributes and bounds, decoded terrain heights, and the
water mask.
The optional terrain texture grid retains packed image/rotation bytes for material
queries using the [published coordinate rules](https://github.com/michidk/redguard-preservation/blob/b2ddb1b/docs/engine/player.md#terrain-material-lookup).
Old caches require re-export for these queries. Explicit selected-support surface
queries apply the [generated terrain blend mask and water overrides](https://github.com/michidk/redguard-preservation/blob/94ea3ae/docs/engine/player.md#terrain-material-blend-coverage).
They do not assign actor surface state or the shared movement veto. Terrain water
height and buoyancy remain unsupported. Schema 2 also retains original static
placement order, model polygons, ROB collision flags and ordered subobject face
lists. Per-face texture set/image
identity is retained for surface lookup. The cache includes the installed
SURFACE.INI assignments, and textured contacts resolve their surface code.
Version-4/5 solid-color contacts resolve to surface zero.
Older caches without face textures or assignments cannot supply surface-dependent
behavior. Missing model-version metadata requires re-export.
Models resolve from the WORLD.INI-selected world archive in authored segment order. Missing models remain
explicitly unsupported. Authored version-4 subobjects are retained in the cache
but discarded during runtime preparation, matching the original loader.
Native, headless, and browser entry load the same validated asset.
Missing or unsupported actor definitions retain a contextual unavailable reason.
Loading also prepares static collision meshes and their spatial registry.
Explicit queries preserve view-centered cell order, separate actor-style
candidates, transform original polygons, and retain segment contact order.
Explicit static body probes test actor spheres against authored model faces and
return ordered contacts for response. Coincident subgroup centers and saturated
body-contact buffers remain unsupported.
Archive selection follows [preservation PR #38](https://github.com/michidk/redguard-preservation/pull/38);
segment admission and traversal follow [PR #41](https://github.com/michidk/redguard-preservation/pull/41).
Sphere model admission and forward slope/support selection follow the
[published player facts](https://github.com/michidk/redguard-preservation/blob/2810574/docs/engine/player.md).
The caller supplies the retained surface veto after support-state updates;
geometric support alone does not authorize forward movement.
When primary support is absent, the selected-player fallback uses the caller's
support-history state and retains the actor's runtime Y at the first accepted
non-side contact. This follows the [published fallback rules](https://github.com/michidk/redguard-preservation/blob/3c5be4a/docs/engine/player.md#selected-player-fallback-support).
Explicit selected-player queries also commit shared support history. Misses retain
metadata, so they can reset the fallback timer; temporary forward probes retain
those changes when position is restored. The caller supplies initial state and
nominal ticks, following the [support-state rules](https://github.com/michidk/redguard-preservation/blob/ed296ed/docs/engine/player.md#support-query-misses).
Ground support reads only face-model candidates. Actor-style obstacle response
remains unsupported.
Startup support uses an actor-centered window and radius 1000, with a vertical
half-length of 500. Its distinct terrain/model ranking retains earlier equal
contacts, following [preservation PR50](https://github.com/michidk/redguard-preservation/pull/50).
The game uses this static query for inspection placement. Dynamic/transient support
and automatic locomotion remain unsupported. Re-export cached worlds after updating.

`transform::Matrix` retains original single-precision actor matrices. Its products
and sums use software x87 extended intermediates and nearest-even stores and
integer conversion, following preservation PR55. The shared rotation table also
supplies player heading projection. Matrix arithmetic requires no renderer or
host floating-point-mode changes.

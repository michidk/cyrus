# cyrus-world

World lifecycle, explicit animation execution and actor transforms,
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

`ActorAnimation::initialize` performs the animation-table loading sequence with
two group-zero requests and retains the selected frame. The caller supplies
nominal ticks and synchronous sound dispatch; this does not grant update eligibility.

World defines `SoundCue` and `SoundSurface`; the audio consumer registers the cue
queue. Animation cues capture the actor surface when the command runs; the
mixer does not read a later component value. Without that consumer, sound-bearing animation remains explicitly
unsupported. `AnimationSystems` lets composition preserve same-update ordering.

Terrain and static collision queries live in [cyrus-collision](../cyrus-collision/README.md).
Run `cargo test -p cyrus-world` for animation, lifecycle and transform contracts.

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
`select_default_with_sounds` applies a caller-selected ordinary default outside
shell execution. A changed default immediately replaces the current old default
before its exit marker and retargets a pending old default through ordinary
request priority. It retains synchronous sound dispatch and shared nominal ticks,
following [preservation's default selection rules](https://github.com/michidk/redguard-preservation/blob/d6e59584fb0fb5bc750e1c881cb386d4c9c690cc/docs/engine/animation.md#living-actor-default-selection).
After an exit marker, a pending non-default can replace them on a later advance,
with delays checked first. These rules are
[published in preservation](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/animation.md#forward-type-one-transitions).
`request_with_sounds`, `force_request_with_sounds`, and `advance_with_sounds` accept a synchronous handler for
signed effect requests, rate offsets in hertz, End stops and replacement handle
resets. The handler owns the original surface remapping, positional gains and
channel allocation. Without it, sound commands still fail explicitly. Audible
original walking cues and automatic runtime scheduling remain unverified. The
[sound facts](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/sound.md#animation-playsound)
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

`transform::Matrix` retains original single-precision actor matrices. Its products
and sums use software x87 extended intermediates and nearest-even stores and
integer conversion, following preservation PR55. The shared rotation table also
supplies player heading projection. Matrix arithmetic requires no renderer or
host floating-point-mode changes.

`Matrix::collision_orientation` rebuilds shape orientation from local angles,
using the published collision matrix rather than retained presentation history.
It shares table samples and extended arithmetic, while retaining the collision
matrix's distinct yaw direction. Model presentation matrices remain independent.

Stored-coordinate matrix transforms retain fractional sphere centers with the
same extended dot-product arithmetic as integer actor transforms. Integer
conversion remains a separate operation for queries that require it.

`ActorAnimation::checkpoint` saves the current receiver and pending request for
ordinary collision rollback. Restoration retains the separately selected default
and sticky failure. The game host validates actor generation and restores the
selected frame; the audio host restores its remembered animation sample slot.

`ActorAnimation::snapshot` captures complete playback state, including the default,
pending request, counters, delays, exit/translation/sound latches and failures.
`from_snapshot` builds a replacement after matching actor context and exact installed
groups, validating selected groups and command cursors. It decodes immutable commands
without executing them. Restoration makes no initialization requests or sound calls.
The versioned snapshot is an internal format, not an original save record.

**Verified:** headless serialization/resume contracts retain pending/default requests,
finite loops, wrapped byte countdowns, delays, exit tails and sticky failures.
Subsequent Stop/Forget cues retain the prior sound latch without replaying Play.
Malformed state and changed sources reject without changing live playback. This does
not restore a game world: the host must validate actor/world generation and bind its
saved model frame and native sound slot before committing the replacement.

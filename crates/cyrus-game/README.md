# cyrus-game

Portable gameplay state, fidelity policy, player input and deterministic controller
rules. Depends on `cyrus-world` and `cyrus-assets`; it does not load an install,
open sockets, spawn a camera, or play audio.

`GamePlugin` installs app state, fidelity and `PlayerPlugin`. The host supplies
Bevy's asset/input/state infrastructure and `Preferences` with a concrete
`PreferenceStore`. Menu load errors remain errors; each consumer owns its fallback.
Successful saves precede runtime application.

Player scene loading prepares `PlayerDefinition`, terrain and static collision
queries asynchronously from validated asset records. The world request carries a
sidecar path rather than depending on this game-owned asset. Inspection adapters
set `InputSuppressed` before `InputSample`; player transforms remain independent
of camera transforms.

Ground-turn, walking startup, wall response and forward support admission are
explicit deterministic APIs. Automatic locomotion, animation eligibility and a
production simulation clock remain unsupported. See
[SETUP](../../docs/SETUP.md) and [ROADMAP](../../docs/ROADMAP.md).

Run `cargo test -p cyrus-game`. Installed-definition tests require `redguard/` and
report when they skip. `dev/scripts/check-core-boundaries.sh` checks the production
dependency graph without workspace or development feature unification.

The debug flycam toggle moves only the view camera. Turning it off returns the
view to the player pose; it does not enable gameplay. Faithful
keyboard actions preserve held and opposing inputs. Modern input samples WASD,
Space, and held Shift run. Camera-relative movement and the mouse camera remain
pending. The deterministic `player::turn` and `player::walk` cores are
tested against original heading and animation-gated startup observations.
Walking supports actor step type 1, advances startup during suppressed
translation, and resets after an update without accepted forward movement.
An explicit ordinary grounded update now composes animation advancement/default
requests, forward support admission, walking translation, support-height assignment,
ordered wall response, residual separation, and startup reset. It preserves
separate animation invocations with the same caller-supplied nominal ticks.
The ordinary idle default comes from installed `attr_panic_group`, following
[preservation](https://github.com/michidk/redguard-preservation/blob/976422b/docs/engine/player.md#ordinary-idle-animation-default).
The forward probe uses installed bounds and walk distance with nearest-even
integer rotation, following [preservation PR48](https://github.com/michidk/redguard-preservation/pull/48).
Player caches retain authored frame-source model radii; candidate selection can
resolve the radius by active model name. Missing named radii require re-export.
The upright orientation helper reuses the original movement rotation table.
Callers still supply eligibility, actor orientation, candidates,
surface policy, and shared support state. Falling, water/lava transitions and
special movement states remain unsupported. Gameplay does not yet drive this
update; production timing, actor initialization and player mesh binding remain
incomplete.

Scene inspection places the player at available marker zero once the world's
`.animations` and `.player` data match, using preservation's exact map coordinates. `MarkerEntry`
selects a different marker and heading for explicit entries. `PlayerPose` retains
runtime coordinates and the wrapped heading; the transform uses the existing
scene export coordinates. Entry then settles against dry terrain and static models,
using the actor-centered startup query documented in
[preservation PR50](https://github.com/michidk/redguard-preservation/pull/50).
Startup selection preserves earlier equal-ranked contacts, independently of ordinary
walking support. Dynamic/transient support and heading-to-render rotation remain
unsupported; this inspection placement does not enable gameplay. Unavailable terrain
or player definitions produce an entry error. Re-export older assets that lack
`map_position`.

`placement::initial_animation` applies the selected-player entry requests with
caller-supplied nominal ticks and a synchronous sound handler. It starts with
default group zero, makes both ordinary setup requests, then forces the default.
It retains the selected file frame for presentation, following
[preservation PR50](https://github.com/michidk/redguard-preservation/pull/50).
This explicit entry API does not choose a production clock or enable locomotion.

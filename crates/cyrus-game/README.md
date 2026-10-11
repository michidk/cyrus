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
explicit deterministic APIs. Automatic locomotion, animation eligibility, ground
settling and a production simulation clock remain unsupported. See
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
Callers still supply eligibility, default group, oriented probe, candidates,
surface policy, and shared support state. Falling, water/lava transitions and
special movement states remain unsupported. Gameplay does not yet drive this
update; production timing, actor initialization and player mesh binding remain
incomplete.

Scene inspection places the player at available marker zero once the world's
`.animations` data loads, using preservation's exact map coordinates. `MarkerEntry`
selects a different marker and heading for explicit entries. `PlayerPose` retains
runtime coordinates and the wrapped heading; the transform uses the existing
scene export coordinates. Heading-to-render rotation and ground settling remain
unsupported. The marker height is the initial height, not the settled ground
height. Re-export older assets that lack `map_position`.

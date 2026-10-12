# cyrus-control

The control protocol between a running game and anything that drives it:
one JSON object per line over TCP, default `127.0.0.1:7117`.

```text
{"id":1,"cmd":{"type":"state"}}
{"id":1,"ok":true,"result":{"state":"InGame","scene":"START","player":{...},"fidelity":{...},"frame":120}}
```

Commands: `ping`, `state`, `screenshot {path}`, `teleport {x,y,z}`,
`input {key, action: press|release|tap}` (Bevy `KeyCode` names),
`set_fidelity {area, mode}`, `skip_startup`, `load_scene {name}`, `play_sound {effect}`,
`play_speech {label}`, `stop_sounds`, `quit`.

`skip_startup` goes to the book menu. `load_scene` skips presentation or
leaves the menu and enters a world. `state` includes `Loading`, `Menu`, or `InGame`.

`Client` is a blocking helper for tests and tools. `server::listen` owns native
TCP framing and sends pending requests to the host. `cyrus/src/control.rs` applies
them on the game thread and replies; `cyrus-mcp` supplies the agent adapter.
This crate has no Bevy dependency.

In `Menu`, `state.menu` reports the page, installed entries, current selection,
preferences, and Controls capture action. Wait for `ready` before sending menu
input and after each animated page change. Quit closes the book before exiting.
Drive Options and Movies with `input`; the same saved bindings and values
apply after a native restart. Native Movies playback uses the saved subtitle and
interlace preferences and reports progress in `state.menu.movie`. A new key press
advances to the next installed skip point, then returns to the menu. Saved Sound
preferences apply to native effects, speech, and CD music. Browser Movies shares
playback rules using locally exported cinematic assets; it has no control port.

`set_sky_scroll {direction, offset}` applies a captured original sky direction
from 0 through 2047 and two texture offsets in texels from -256 through 256.
Use it after loading the reference world. It synchronizes sky motion for
comparison; it does not synchronize other users of the original random stream.

`scripts {offset, limit}` returns bounded logical actor state. `step_script
{generation, index, stage, frame_scale, nominal_ticks}` selects `animation`,
`entry`, `main`, `interrupt`, or `tasks`.
`tasks` requires an explicit unsigned `frame_scale` with eight fractional bits.
For example, 256 supplies scale one; zero is accepted. Main and interrupt body
steps do not poll existing tasks. These inspection tools do not enable gameplay.

`bind_script_animation {generation, index, default_group, node?}` prepares an
actor once for this world generation. The caller supplies the established default
group. An optional exact scene node name transfers its animation presentation to
the logical actor. Without a node, stepping captures sound intents in the response's `captured_sounds` array instead of playing them. Once any actor is animation-bound,
all steps require explicit `nominal_ticks`, including zero, before execution.
These tools do not determine production eligibility or pacing.

Spatial inspection uses `step_script` stages `prepare_spatial`, `snapshot`,
`transform`, and `families`. Preparation is explicit and transactional. Snapshot
copies the selected actor's local angles; transform refreshes an unattached
matrix; families composes prepared attachments. These operations grant no actor
eligibility. Once spatial state is prepared, executable stages need both
`frame_scale` and `nominal_ticks`, including zero.

Player queries use the current `PlayerPose`, not the inspection camera.
`distance_override` must explicitly establish the PlayerDistance override flag;
`fallback_position` supplies original Q8 coordinates when no player exists.
Missing required inputs fail. Attached queries read the last composed pose.
Unattached linked-position selection remains unsupported in the script host.

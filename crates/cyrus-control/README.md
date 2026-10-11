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

`Client` is a blocking helper for tests and tools. The game side lives in
`cyrus-game::control`; the agent side in `cyrus-mcp`. This crate has no Bevy
dependency so drivers stay small.

In `Menu`, `state.menu` reports the page, installed entries, current selection,
preferences, and Controls capture action. Wait for `ready` before sending menu
input and after each animated page change. Quit closes the book before exiting.
Drive Options and Movies with `input`; the same saved bindings and values
apply after a native restart. Native Movies playback uses the saved subtitle and
interlace preferences and reports progress in `state.menu.movie`. A new key press
advances to the next installed skip point, then returns to the menu. Saved Sound
preferences apply to native effects, speech, and CD music. Browser cinematic
playback remains unavailable.

`set_sky_scroll {direction, offset}` applies a captured original sky direction
from 0 through 2047 and two texture offsets in texels from -256 through 256.
Use it after loading the reference world. It synchronizes sky motion for
comparison; it does not synchronize other users of the original random stream.

# cyrus-control

The control protocol between a running game and anything that drives it:
one JSON object per line over TCP, default `127.0.0.1:7117`.

```text
{"id":1,"cmd":{"type":"state"}}
{"id":1,"ok":true,"result":{"state":"InGame","scene":"START","player":{...},"fidelity":{...},"frame":120}}
```

Commands: `ping`, `state`, `screenshot {path}`, `teleport {x,y,z}`,
`input {key, action: press|release|tap}` (Bevy `KeyCode` names),
`set_fidelity {area, mode}`, `skip_startup`, `load_scene {name}`, `quit`.

`skip_startup` goes to the book menu. `load_scene` skips presentation or
leaves the menu and enters a world. `state` includes `Loading`, `Menu`, or `InGame`.

`Client` is a blocking helper for tests and tools. The game side lives in
`cyrus-game::control`; the agent side in `cyrus-mcp`. This crate has no Bevy
dependency so drivers stay small.

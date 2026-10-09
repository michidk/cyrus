# cyrus-control

The control protocol between a running game and anything that drives it:
one JSON object per line over TCP, default `127.0.0.1:7117`.

```text
{"id":1,"cmd":{"type":"state"}}
{"id":1,"ok":true,"result":{"scene":"START","player":{...},"fidelity":{...},"frame":120}}
```

Commands: `ping`, `state`, `screenshot {path}`, `teleport {x,y,z}`,
`input {key, action: press|release|tap}` (Bevy `KeyCode` names),
`set_fidelity {area, mode}`, `load_scene {name}`, `quit`.

`Client` is a blocking helper for tests and tools. The game side lives in
`cyrus-game::control`; the agent side in `cyrus-mcp`. This crate has no Bevy
dependency so drivers stay small.

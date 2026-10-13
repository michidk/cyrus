# cyrus-console

Portable editor, keyboard focus and command dispatch. This crate depends on
Bevy's ECS/input layer, serde and anyhow. It does not depend on gameplay,
rendering, installed assets, transport or host storage.

Add `ConsolePlugin`, then register commands with `ConsoleApp::console_command`.
Each handler is a normal Bevy system taking `In<Invocation>` and returning
`anyhow::Result<Vec<String>>`. Plugins retain ownership of the state they change.
Names and aliases are unique. Programmatic dispatch accepts canonical names;
interactive dispatch also accepts exact aliases. Hidden gameplay input only
accepts commands marked as cheats and does not append console output.

Hosts set `Console.enabled` and `Console.blocked` to control input availability.
Keyboard submissions execute during `ConsoleInput` before the next keyboard
event. Other queued submissions run in `ConsoleDispatch` after input. An
exclusive host adapter can use `submit` for synchronous execution and reports.

The shared `number` parser accepts decimal, hexadecimal, percent-prefixed and
packed four-byte values. It rejects unresolved symbols and short quoted inputs.
**Verified:** percent-prefixed input returns zero under the
[preservation console rules](https://github.com/michidk/redguard-preservation/blob/feat/console-presentation/docs/engine/console.md).

`cargo test -p cyrus-console` verifies a consumer without the game or renderer.

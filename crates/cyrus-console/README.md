# cyrus-console

Portable console editor, keyboard focus and command dispatch. Subsystems
register their own commands; the game hosts and `cyrus-debug` use it. It depends
only on Bevy's ECS/input layer, serde and anyhow, plus redguard-preservation
behind the optional `cpf` feature. It must not depend on gameplay, rendering,
installed assets, transport or host storage.

## Modules

| Module | Responsibility |
| --- | --- |
| [lib](src/lib.rs) | `ConsolePlugin`, `ConsoleApp`, `Command`, `Invocation`, `submit`, the `ConsoleInput`/`ConsoleDispatch` sets |
| [editor](src/editor.rs) | Console line editor and hidden gameplay cheat entry |
| [input](src/input.rs) | Keyboard focus; each Enter dispatches before the next key event |
| [catalog](src/catalog.rs) | The original command names and aliases |
| [number](src/number.rs) | Numeric literals: decimal, hexadecimal, percent-prefixed, packed four-byte |
| [preferences](src/preferences.rs) | Console preferences (`font`, `paper`, `col0`–`col3`, ...) and the host `Store` |

## Usage

```rust
app.add_plugins(cyrus_console::ConsolePlugin)
    .console_command(command, handler); // command: cyrus_console::Command
```

A handler is an ordinary Bevy system taking `In<Invocation>` and returning
`anyhow::Result<Vec<String>>`; the registering plugin keeps owning the state it
changes. `console_symbols` supplies live game values to numeric arguments.
Hosts toggle `Console.enabled` / `Console.blocked`, install `Console::with_store`
for persistence and log export, and may call `submit` from an exclusive system
for synchronous execution.

## Notes

- Names and aliases are unique. Programmatic dispatch takes canonical names;
  aliases work only in interactive dispatch. Hidden gameplay input accepts only
  commands marked as cheats and writes no console output.
- The original catalog stays visible in `commands`; entries without an owner
  report unsupported rather than disappearing.
- Number parsing follows preservation's
  [console rules](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/console.md)
  (percent-prefixed input returns zero). Short quoted inputs are unsupported.
- Malformed stored preferences block edits and saves instead of being overwritten.

## Testing

```sh
cargo test -p cyrus-console
cargo test -p cyrus-console --features cpf
just verify console
```

Tests build a consumer without the game or renderer and need no install.

# cyrus-game

Gameplay state shared by every host, independent of presentation and
scripting: the fidelity policy, accepted menu preferences, named cheat state,
the session random stream, `AppState`, movie unlocks and ropes. It builds on
`cyrus-assets` and `cyrus-world` and has no dependency on the script host;
`cyrus-script` builds on it.

## Modules

| Module | Responsibility |
| --- | --- |
| [fidelity](src/fidelity.rs) | Faithful/Modern policy by gameplay `Area` |
| [preferences](src/preferences.rs) | Accepted menu settings with host-provided persistence |
| [cheats](src/cheats.rs) | Named cheat state with host-provided CHT persistence |
| [random](src/random.rs) | Session random stream shared by scripts and gameplay services |
| [rope](src/rope.rs) | Rope records and attachment rules |
| [lib](src/lib.rs) | `AppState`, `MovieUnlocks`, gameplay overlay and pause-book state |

## Notes

- Every fidelity area defaults to `Faithful`; a deviation runs only in a
  `Modern` branch (see the hard rules in [AGENTS.md](../../AGENTS.md) and
  [docs/DIFFERENCES.md](../../docs/DIFFERENCES.md)).
- Preferences are stored in the original menu's action units, and are applied
  at runtime only after they were persisted successfully.
- File access belongs to the hosts; this crate holds state and rules only.

## Testing

```sh
cargo test -p cyrus-game
just verify game
```

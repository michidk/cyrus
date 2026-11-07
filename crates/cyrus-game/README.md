# cyrus-game

Gameplay state shared by every host, independent of presentation and scripting:
the fidelity policy, accepted menu preferences with host-provided persistence,
named cheat state, the session random stream, `AppState`, movie unlocks and
ropes. It has no dependency on the script host; `cyrus-script` builds on it.

## Module guide

| Module | Responsibility |
| --- | --- |
| [fidelity](src/fidelity.rs) | Faithful/Modern policy by gameplay area |
| [preferences](src/preferences.rs) | Accepted menu settings and host-provided persistence |
| [cheats](src/cheats.rs) | Named cheat state and host-provided CHT persistence |
| [random](src/random.rs) | Session random stream shared by scripts and gameplay services |
| [rope](src/rope.rs) | Rope records and attachment rules |

Preferences use original action units; successful persistence precedes runtime
application. Fidelity areas default to Faithful.

Run `cargo test -p cyrus-game`.

# cyrus-soup

Portable SOUP script virtual machine: program records, invocation stages,
actor task slots, values and VM snapshots. The game supplies every effect
through the `Host` trait; this crate supplies no clock, activation or
eligibility. `cyrus-assets` builds `program::Program` records from the
install, and `cyrus-script` runs them and implements the native handlers. It
depends only on `anyhow` and `serde`: no Bevy, asset loader, install parser or
platform I/O. The behaviour follows preservation's
[SOUP contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md).

## Modules

| Module | Responsibility |
| --- | --- |
| [lib](src/lib.rs) | `Runtime`, `Host`, `ScriptValue`, `CallResult`; entry, main and interrupt invocation |
| [program](src/program.rs) | Instructions, definitions and serialized schema; [validation](src/program/validation.rs) before any host effect |
| [tasks](src/tasks.rs) | Six actor-owned task slots, blocking and timed task passes, operand reevaluation |
| [routing](src/routing.rs) | Object calls to other actors, `receive`, native hooks |
| [values](src/values.rs) | Integer formulas and conditions, typed actor references |
| [persistence](src/persistence.rs) | Versioned VM `Snapshot`, validated restore, actor-reference rebinding |
| [attribute](src/attribute.rs) | `Attribute` slots and an actor's 256 `Attributes` bytes |
| [animation_group](src/animation_group.rs) | `AnimationGroup` numbers with the named DEF groups |
| [native](src/native.rs) | `Native`: every native function in DEF order plus engine-only natives |
| [property](src/property.rs) | `Property`: every object property in DEF `[refs]` order |

## Usage

A host builds a `Runtime` for an actor and calls one explicit stage per update:
`invoke_entry`, `invoke_main_stage` (entry, timed task pass and main body),
`invoke_body`, `invoke_tasks_timed`, `invoke_native_hook` or `receive`. Native
calls reach `Host::call` as a typed `Native` with arguments in authored order.
`Runtime::snapshot` and `Runtime::from_snapshot` capture and rebuild the VM.

## Notes

- `Attribute`, `AnimationGroup`, `Native` and `Property` are named after the
  installed `SOUP386.DEF` and serialize as its spellings. Convert installed
  names or numbers to them once at the boundary; use `from_index` or raw
  attribute bytes only where the number is data (a script argument, an
  installed record, a control request).
- The caller chooses eligibility and timing. Task timers take an explicit
  frame scale with eight fractional bits.
- Failures (unsupported operations, budget exhaustion, consumed
  `CallResult::CompleteWithoutValue`) are sticky and retain
  world/actor/instruction context, so an effect is never repeated.
- Program records must be schema version 4; VM snapshots are version 2 and
  still restore version 1. A snapshot is a VM restore boundary, not a game
  save: the game host restores globals, RNG, animation and native task state
  and rebinds actor references and task tokens.
- Actor references are logical generation/index identities, never pointers.
  Arithmetic and ordering on references are unsupported.
- New native handlers belong in `cyrus-script`; format and engine discoveries
  belong in redguard-preservation.

## Testing

```sh
cargo test -p cyrus-soup
dev/scripts/check-core-boundaries.sh   # builds cyrus-soup as an isolated consumer
```

The tests use synthetic programs through the public interface and need no
install. They do not establish complete game fidelity.

# cyrus-script

Portable SOUP execution from its own `program::Program`. The `program` module
owns instructions, definitions and validation. Asset preparation constructs these
records without changing their serialized schema. This crate depends only on
`anyhow` and `serde`, with no Bevy, asset loader, install parser or platform I/O.
`dev/scripts/check-core-boundaries.sh` verifies it as an isolated consumer.

`Runtime::invoke` runs an explicitly eligible main or interrupt stage through an
authored yield or blocking task. The host supplies synchronous native services,
completion tokens, End notifications, shared global flags, and a diagnostic
instruction budget. Budget exhaustion and unsupported operations retain a sticky
world/actor/instruction error. The core supplies no production clock or activation.

**Verified:** operand structure, cooperative invocation, End continuation, separate
invocation cursors and actor return continuation, and integer operators follow preservation's
[SOUP contract](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/SOUP.md).
Synthetic public-interface tests cover immediate statements, subroutines,
shared locals, blocking completion, arithmetic, branches, and sticky failures.
These tests do not establish complete game fidelity.

`CallResult::CompleteWithoutValue` lets a host complete an effect when no
portable numeric result is established. Statements discard that result;
formulas, conditions and arguments fail contextually when consuming it.
The failure is retained so the effect is not repeated. Routing and native hooks
preserve the absent value. This does not define a new result for the original
handler or admit any unsupported native service.

Six actor-owned task slots support blocking tasks, multitasks, operand
reevaluation, explicit task passes, pause/resume, stop and teardown cancellation.
Native operands evaluate from last to first; handlers receive authored argument order.
`invoke_body` excludes the existing-task pass; `invoke` combines a main task pass
with body processing for the existing animation adapter. The caller supplies
eligibility and task timing. `invoke_tasks_timed` takes an explicit frame scale
with eight fractional bits. **Verified:** each slot starts at counter one;
elapsed uses the low 32-bit counter/scale product shifted right by eight.
Counters wrap at 16 bits and paused slots do not advance. Initial native
completion does not skip allocation or the blocking yield. Hosts must retain a
task token for completion on a later task pass. Hosts receive elapsed
zero for initial task calls, and unpaused total/blocking slot counts that reflect
earlier completions in the same pass. Untimed animation passes and Function calls
have no established elapsed input. `Yield`, Anchor and ScriptRV retain actor state.
Exceeding task capacity fails before a seventh host effect. Full save restoration
and complete native command services remain unsupported.

`Host::route` resolves object calls before evaluating their operands. `Runtime::receive`
uses receiver locals and task slots without running its body. Initial native strings
use the executing actor's table; later ordinary receiver task passes use the receiver's.
`CodeOrigin` and `TaskSource` retain the physical RASC operand identity across routing.
`Host::task_call` supplies foreign instructions for reevaluation. Duplicate suppression
and task controls compare physical operands. Object calls continue the caller, including
explicit Me tasks. Hosts restore receiver context and runtime ownership on errors.
The default host rejects non-Me routing. `State` is serializable for inspection;
serialization alone is not a save/load implementation. Production task tokens
must remain bound to their owning actor and world generation.

`Runtime::snapshot` captures complete VM state and the exact owning/foreign
programs used by its tasks. `Runtime::from_snapshot` builds a replacement VM
without invoking native services or scripts. It validates program content,
continuation and return cursors, local counts, task source/operand identities,
blocking kinds, and unique saved tokens. Every saved token requires an explicit,
unique replacement token prepared by the host. `Snapshot::task_tokens` exposes
those saved tokens in slot order. Slot holes, pause state, counters and sticky
failures are retained. Source programs are included for exact compatibility,
including their strings and initial locals, without an asset or hashing dependency.
`Snapshot::task_calls` validates saved structure and exposes each token's
unevaluated source call, including paused and foreign slots. Game hosts use it
to validate native continuation variants. It does not replace the installed-source
validation in `Runtime::from_snapshot`.

**Verified:** headless public-interface tests resume waits and subroutines without
reissuing native calls, retain both continuations and saved return values, poll
foreign tasks after rebinding, preserve paused/wrapping counters, and reject
malformed state or changed code without altering the live VM. This is a VM
restore boundary, not a complete game save. The owning game host must validate
actor/world identity and restore globals, RNG, animation, native task state and
all other gameplay state before committing the replacement world. Native/browser
storage, original-save import and full game restoration remain unsupported.

Run `cargo test -p cyrus-script`. New handlers belong to `cyrus-game`; format and
engine discoveries belong to preservation.

`invoke_entry` applies the ordinary entry gate before running the interrupt
continuation. An unpaused blocking task suppresses entry when its physical
operand identity is at or before that cursor, including foreign task sources.
`invoke_body(Thread::Interrupt)` remains the separately admitted direct interrupt
path. Neither entry API runs existing tasks or chooses update eligibility.

`invoke_native_hook` dispatches the validated RAHK call with the actor's locals
and strings, without advancing either continuation or polling tasks. It retains
native failures so a failed effect is not retried. Hook eligibility remains the
game scheduler's responsibility.

`invoke_main_stage` combines entry, timed task polling, and main continuation for
an admitted ordinary update. Its task-bypass input skips polling and the main
blocking-slot gate, while retaining entry's operand gate. It does not choose
actor eligibility or recheck game control flags between entry and main.

Property references retain the full installed 16-bit index. Program schema 4
rejects older exports that could discard the high property byte or the distinct
object function-expression encoding.
The host owns property semantics; the portable runtime forwards the complete
reference and does not select an actor field itself.

The host receives native call provenance before operand evaluation. Statements
and direct function values reset command state; object function values retain
the point value, including explicit Me calls. Hooks preserve current state.
Task passes load active slots and save empty, paused and pending slots. Completed
slots leave their resulting command state shared without saving it. Initial
allocation saves the first invocation result. Game hosts retain shared state
above actors and six banks per actor; portable hosts may omit these callbacks.

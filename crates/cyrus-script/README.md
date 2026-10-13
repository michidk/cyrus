# cyrus-script

Portable SOUP execution from `cyrus-assets::script::Program`. No Bevy types,
renderer, filesystem, install parser, audio handle, or socket appears in its API.
The asset crate still brings a transitive Bevy asset dependency.

`Runtime::invoke` runs an explicitly eligible main or interrupt stage through an
authored yield or blocking task. The host supplies synchronous native services,
completion tokens, End notifications, shared global flags, and a diagnostic
instruction budget. Budget exhaustion and unsupported operations retain a sticky
world/actor/instruction error. The core supplies no production clock or activation.

**Verified:** operand structure, cooperative invocation, End continuation, separate
invocation cursors and actor return continuation, and integer operators follow preservation's
[SOUP contract](https://github.com/michidk/redguard-preservation/blob/a2ee1f123c9961aac44c3e24193975fb030600df/docs/engine/SOUP.md).
Synthetic public-interface tests cover immediate statements, subroutines,
shared locals, blocking completion, arithmetic, branches, and sticky failures.
These tests do not establish complete game fidelity.

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

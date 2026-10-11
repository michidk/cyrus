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
[SOUP contract](https://github.com/michidk/redguard-preservation/blob/feat/soup-runtime-records/docs/engine/SOUP.md).
Synthetic public-interface tests cover immediate statements, subroutines,
shared locals, blocking completion, arithmetic, branches, and sticky failures.
These tests do not establish complete game fidelity.

Six actor-owned task slots support blocking tasks, multitasks, operand
reevaluation, explicit task passes, pause/resume, stop and teardown cancellation.
Native operands evaluate from last to first; handlers receive authored argument order.
`invoke_body` excludes the existing-task pass; `invoke` combines a main task pass
with body processing for the existing animation adapter. The caller supplies
eligibility and task timing. `Yield`, Anchor and ScriptRV retain actor state.
Exceeding task capacity fails before a seventh host effect. Full save restoration
and complete native command services remain unsupported. Object-targeted task
routing is explicitly unavailable until receiver ownership is implemented. `State` is serializable for inspection;
serialization alone is not a save/load implementation. Production task tokens
must remain bound to their owning actor and world generation.

Run `cargo test -p cyrus-script`. New handlers belong to `cyrus-game`; format and
engine discoveries belong to preservation.

## Goal

Make `spawn_agent` block by default in both collaboration surfaces:

- legacy V1 `spawn_agent`
- `multi_agent_v2` `spawn_agent`

The new default should return control to the parent agent when the spawned child reaches any turn boundary, not only when the child has fully finished its task.

That means the parent should resume when the child emits either:

- `TurnComplete`
- `TurnAborted`

This includes cases where the child ends a turn because it is blocked, needs clarification, hit a problem, or was interrupted before the task itself is fully complete.

The change must preserve an explicit asynchronous escape hatch for cases where the parent intentionally wants background execution.

## Current behavior

Today both spawn flows return immediately after the child thread is created.

### V1

The legacy handler in `codex-rs/core/src/tools/handlers/multi_agents/spawn.rs` creates the child and returns `agent_id` immediately. If the parent wants to wait, it must make a separate `wait_agent` call against one or more child thread ids.

The legacy `wait_agent` implementation waits for a final child status. In practice that means it wakes for:

- `Completed`
- `Errored`
- `Shutdown`
- `NotFound`

It does not treat `Interrupted` as a wake condition because `Interrupted` is currently considered non-final.

### V2

The `multi_agent_v2` handler in `codex-rs/core/src/tools/handlers/multi_agents_v2/spawn.rs` creates the child and returns the canonical `task_name` immediately.

The separate V2 `wait_agent` tool does not wait on child status directly. It waits on any mailbox-sequence change in the parent session. In practice this is driven by inter-agent notifications, especially the child-to-parent completion envelope emitted from `codex-rs/core/src/codex.rs`.

Today that parent notification only happens for terminal child turns. The current implementation explicitly ignores interrupted turns.

### Prompt pressure

The tool description in `codex-rs/tools/src/agent_tool.rs` strongly frames `spawn_agent` as an asynchronous sidecar primitive and tells the model to avoid blocking waits on the critical path. That description matches the current runtime but reinforces the default behavior that this design is changing.

## Requirements

The approved contract for this change is:

1. `spawn_agent` becomes blocking by default in both V1 and V2.
2. `spawn_agent` gains an explicit opt-out parameter:
   - `blocking`
3. `blocking` defaults to `true` when omitted.
4. `blocking = false` preserves the current immediate-return behavior.
5. The blocking path must return control to the parent on any child turn boundary:
   - `TurnComplete`
   - `TurnAborted`
6. This wake condition applies even when the child task is not fully done yet.
7. The tool result should remain centered on child identity:
   - V1 continues to return `agent_id`
   - V2 continues to return `task_name`
8. The blocking result should also include the child status observed at the wake boundary so the parent knows why control returned.

## Approaches considered

Three approaches were considered:

1. Change only the tool description or prompt guidance
2. Change runtime behavior so `spawn_agent` blocks by default, with `blocking = false` as an escape hatch
3. Add a new blocking-only tool and keep `spawn_agent` asynchronous

The chosen approach is option 2.

Changing only the prompt would leave the runtime footgun intact and would not guarantee the new behavior.

Adding a new tool would preserve backwards compatibility at the API level, but it would keep the default wrong for the intended workflow and would force the model to choose between two nearly identical primitives.

## Chosen design

Add `blocking: Option<bool>` to both `spawn_agent` schemas and handlers.

Semantics:

- omitted: behave as `blocking = true`
- `true`: spawn the child, then block until the first child turn boundary is observed
- `false`: spawn the child and return immediately, preserving current behavior

The blocking path should not call the public `wait_agent` tool or depend on an additional model round-trip. The handler itself should wait internally using the same lower-level primitives that the existing wait flows already use.

## API changes

### V1 `spawn_agent`

`spawn_agent` in the legacy collaboration surface gains:

- `blocking: bool` with default `true`

The result shape becomes:

- `agent_id`
- `nickname`
- `status`

`status` carries the first non-running status observed after spawn.

### V2 `spawn_agent`

`spawn_agent` in `multi_agent_v2` gains:

- `blocking: bool` with default `true`

The result shape becomes:

- `task_name`
- `nickname` when metadata is visible
- `status`

If metadata hiding is enabled, the result still includes:

- `task_name`
- `status`

### Tool description updates

The tool description in `codex-rs/tools/src/agent_tool.rs` must be updated to reflect the new default:

- `spawn_agent` blocks by default until the child reaches a turn boundary
- use `blocking = false` when intentional background execution is desired

The existing description that pushes the model away from blocking waits should be removed or rewritten so it no longer contradicts the runtime.

## Runtime behavior

### Boundary definition

For this change, a child turn boundary means the parent may resume when the child emits either:

- `TurnComplete`
- `TurnAborted`

This includes `TurnAborted(Interrupted)`.

The implementation should not require the child task to be terminal. A child that ends a turn because it needs help should still wake the parent.

### V1 wake path

The V1 blocking wait should subscribe to the child status stream after spawn and return when the child status changes away from the "still running" states:

- `PendingInit`
- `Running`

The wake path should therefore return on:

- `Interrupted`
- `Completed`
- `Errored`
- `Shutdown`
- `NotFound`

This is intentionally broader than the current legacy `wait_agent` final-only behavior.

Because the blocking default changes the meaning of "wait for the child to hand control back", the legacy `wait_agent` tool should also be aligned to this wake contract so the two primitives do not disagree about what a child boundary means.

### V2 wake path

The V2 blocking wait should use the parent mailbox notification path that already exists for child-to-parent coordination.

The current implementation in `codex-rs/core/src/codex.rs` only notifies the parent for terminal child turns. That must change.

The parent should be notified on any child turn boundary from a spawned V2 child:

- `TurnComplete`
- `TurnAborted`

That notification should carry the same structured `AgentStatus` currently embedded in the standard subagent notification envelope.

This allows the blocking `spawn_agent` handler to wait on the next matching mailbox notification and then return with the child identity and observed status.

### Matching the wake to the spawned child

The blocking handler must wait only for the child it just spawned, not for arbitrary mailbox activity from other children.

For V1, this is naturally keyed by child thread id.

For V2, the wait must filter mailbox notifications by the spawned child's canonical agent path. A mailbox sequence change alone is not sufficient if other agents may also send mail to the same parent.

### No timeout in this first slice

This design intentionally does not add a timeout parameter to `spawn_agent`.

Reasons:

- the requested behavior is "blocking by default"
- `blocking = false` already provides the explicit async escape hatch
- adding timeout semantics would introduce another return mode and another user-visible policy decision that was not part of the approved contract

Timeout behavior can be added later if there is a concrete need.

## `wait_agent` alignment

Even though the primary goal is changing `spawn_agent`, the separate wait tools should stay coherent with the new meaning of child handoff.

### Legacy `wait_agent`

Update the V1 wait logic so it wakes on the first child status that is not:

- `PendingInit`
- `Running`

That means `Interrupted` should now wake the parent instead of being treated as still in progress.

### V2 `wait_agent`

Keep the "mailbox-driven" model, but ensure that child boundary notifications now occur for both:

- `TurnComplete`
- `TurnAborted`

This makes `wait_agent` and blocking `spawn_agent` observe the same child handoff events.

## Files expected to change

Primary implementation files:

- `codex-rs/tools/src/agent_tool.rs`
- `codex-rs/tools/src/agent_tool_tests.rs`
- `codex-rs/core/src/tools/handlers/multi_agents/spawn.rs`
- `codex-rs/core/src/tools/handlers/multi_agents/wait.rs`
- `codex-rs/core/src/tools/handlers/multi_agents_v2/spawn.rs`
- `codex-rs/core/src/tools/handlers/multi_agents_v2/wait.rs`
- `codex-rs/core/src/codex.rs`
- `codex-rs/core/src/tools/handlers/multi_agents_tests.rs`
- `codex-rs/core/src/tools/spec_tests.rs`

Supporting refactors are acceptable if they keep the behavior localized and avoid inflating already central modules more than necessary.

## Test plan

Follow TDD for the behavior change.

Minimum regression coverage:

1. V1 `spawn_agent` with omitted `blocking` waits until child `TurnComplete`
2. V1 `spawn_agent` with omitted `blocking` waits until child `TurnAborted(Interrupted)`
3. V1 `spawn_agent` with `blocking = false` returns immediately
4. V2 `spawn_agent` with omitted `blocking` waits until child `TurnComplete`
5. V2 `spawn_agent` with omitted `blocking` waits until child `TurnAborted(Interrupted)`
6. V2 `spawn_agent` with `blocking = false` returns immediately
7. V1 `wait_agent` wakes on `Interrupted`
8. V2 parent notification fires on interrupted child turns

Existing tests that assert interrupted V2 child turns do not notify the parent must be updated to the new contract.

Schema and description tests should also assert:

- `blocking` is present in both V1 and V2 `spawn_agent` schemas
- `blocking` is optional at the API layer
- the tool description states that blocking is the default and `blocking = false` is the async opt-out

## Non-goals

This design does not include:

- a new dedicated blocking-only tool
- automatic timeout behavior for `spawn_agent`
- changes to `send_message`, `followup_task`, or `close_agent`
- broader redesign of the mailbox protocol beyond the minimum needed to wake on all child turn boundaries

## Risks and mitigations

### Risk: unrelated mailbox activity wakes the wrong wait

Mitigation:

- key V2 wake logic to the specific spawned child agent path, not only to mailbox sequence changes

### Risk: prompt/runtime contract drift

Mitigation:

- update tool descriptions and schema tests in the same change as the runtime behavior

### Risk: V1 and V2 diverge semantically again

Mitigation:

- explicitly align both `spawn_agent` and `wait_agent` around the same notion of child handoff

## Rollout expectation

After this change, the default mental model for delegation becomes:

- spawn a subagent
- wait automatically for the child to hand control back at the next turn boundary
- use `blocking = false` only when background parallelism is intentional

That should make sequential delegation flows the default and remove the current need for prompt-only discipline to keep parent and child control flow aligned.

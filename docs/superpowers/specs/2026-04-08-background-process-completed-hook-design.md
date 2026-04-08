## Goal

Add a first-class hook for background terminal completions so Codex can resume follow-up work when a long-running `exec_command` finishes after the originating turn has already moved on.

The motivating cases are:

- `babysit-pr --watch` and similar watcher flows
- long Rust builds or test runs in this repository
- other background shell tasks where the model wants a later continuation opportunity

## Current behavior

Today Codex emits `ExecCommandEnd` when a unified-exec background process finishes, but that event is only visible to the UI and app-server surfaces. It does not itself create model-visible pending work, does not reopen an idle thread, and does not map to any existing hook event.

The current hook surface is limited to:

- `PreToolUse`
- `PostToolUse`
- `SessionStart`
- `UserPromptSubmit`
- `Stop`

That makes detached background execution a contract mismatch for skills such as `babysit-pr`: the process can keep running after the turn ends, but Codex has no explicit lifecycle event for "this background command completed later, decide what to do now".

## Chosen design

Implement a new first-class hook event named `BackgroundProcessCompleted` and pair it with a new optional `exec_command` parameter:

- `completion_behavior`

The initial wire values are:

- `auto`
- `wake`
- `ignore`

`auto` becomes the default when the parameter is omitted.

### `completion_behavior` semantics

- `auto`
  - If the process completes before the originating turn has fully handed control back, keep the existing synchronous completion behavior.
  - If the process completes later and the owning thread is idle, enqueue a follow-up completion notification, run the new hook, and start a new turn automatically.
  - If the process completes later and the owning thread is still active, do not interrupt or enqueue a later follow-up. This keeps the default convenient without injecting surprise work into a thread that is already busy doing something else.

- `wake`
  - Same as `auto` when the thread is idle.
  - If the process completes later while the owning thread is still active, queue a follow-up completion notification for the next turn instead of dropping it. Do not interrupt the active turn.
  - This is the mode the model should use for long-running work it explicitly intends to revisit, such as `cargo test`, `cargo build`, or a babysitting/watcher process.

- `ignore`
  - Keep emitting `ExecCommandEnd` to UI/app-server consumers.
  - Do not enqueue any follow-up work and do not invoke the new hook.

This keeps the default low-risk while still giving the model an explicit way to request a later wake-up.

## Why a new hook event

Three approaches were considered:

1. New `BackgroundProcessCompleted` hook plus `completion_behavior`
2. Reuse `PostToolUse` for late completions
3. Add an internal wake-up path with no hook event

The first approach is the recommended one.

Reusing `PostToolUse` would blur two distinct contracts:

- synchronous post-tool handling for the original tool call
- asynchronous completion handling that can happen after turn completion

The current code and tests already treat interactive or still-running exec sessions as outside the `PostToolUse` path, so overloading that hook would make the existing model harder to reason about.

Adding only an internal wake-up path would solve one symptom but would not expose a deliberate lifecycle point for customization, policy, observability, or skill-specific behavior.

## New hook contract

### Hook name

Add `BackgroundProcessCompleted` to the core hook event enum, hook discovery, schemas, and app-server-exported hook types.

### Hook config surface

`hooks.json` gains a new top-level event section:

```json
{
  "hooks": {
    "BackgroundProcessCompleted": [
      {
        "matcher": "cargo (build|test)",
        "hooks": [
          {
            "type": "command",
            "command": "..."
          }
        ]
      }
    ]
  }
}
```

This event should support a matcher, and the matcher input should be the final shell command string. That makes it possible to target only selected classes of background work without introducing another selector mechanism.

The new event should remain compatible with the existing `allowSubagent` behavior.

### Hook scope

`BackgroundProcessCompleted` should be modeled as a thread-scoped hook.

Rationale:

- it may fire after the originating turn has completed
- the original turn id remains useful as metadata, but the event concept belongs to the thread lifecycle rather than a currently active turn
- thread scope matches the "late completion" nature more closely than turn scope

### Hook request payload

The hook request should include enough information for policy and follow-up decisions without dumping unbounded output into the hook layer.

Required fields:

- `session_id`
- `originating_turn_id`
- `cwd`
- `transcript_path`
- `model`
- `permission_mode`
- `call_id`
- `process_id`
- `command`
- `exit_code`
- `duration_ms`
- `status`
- `completion_behavior`
- `is_subagent`
- `aggregated_output_tail`

`aggregated_output_tail` should be a bounded tail view of the final output, not the full transcript blob.

## Runtime behavior

### Late-completion detection

The new behavior applies only when a process completion is observed after the original `exec_command` tool response has already returned a live `process_id` and the follow-up is no longer part of the current synchronous tool-result loop.

Short-lived commands that finish within the original turn should keep the existing path and should not create a second follow-up lifecycle event.

### Internal follow-up item

Late completions need a model-visible bridge into the normal turn pipeline.

The runtime should introduce an internal session-scoped pending completion record owned by the thread that launched the process. This is not a new public model API. It is an internal unit of pending work that carries the bounded completion payload described above.

This internal record must support two behaviors:

- immediate consumption by a newly started turn when the thread is idle
- queued consumption by the next turn when `completion_behavior = wake` and another turn is already active

### Turn-start behavior

When a new turn is started because of a queued background completion:

1. Run `BackgroundProcessCompleted` hooks for the queued record.
2. Record any additional context or stop/block outputs from the hook.
3. Materialize a compact model-visible notification describing the completion so the model can act on it.

That notification should be phrased as a first-party runtime note, not as fake user input.

Example shape:

> Background process completed after the previous turn ended. Command: `cargo test -p codex-tui`. Exit code: 0. Output tail: ...

This lets the model continue naturally without pretending the user said anything.

## Interaction with active threads

Codex can already tell whether a thread or agent is still active. The design relies on that.

The behavior should be:

- if the thread is idle, `auto` and `wake` may start a new turn
- if the thread is active, `auto` drops the follow-up and `wake` queues it for the next turn
- `ignore` never creates thread work

No mode should interrupt an active turn just because a background process finished.

## Prompt and skill guidance

The runtime change alone is not sufficient. The model needs guidance for when to request resumable background behavior.

Update prompt and skill guidance so Codex learns this pattern:

- use `completion_behavior = wake` for long-running work it explicitly intends to revisit
- examples:
  - `cargo build`
  - `cargo test`
  - watcher loops such as PR babysitting
  - background scripts whose result determines the next step
- use `completion_behavior = auto` when a wake-up would be nice but not essential
- use `completion_behavior = ignore` for fire-and-forget tasks

The `babysit-pr` skill in both locations must be updated:

- `/home/k3/git/codex/.codex/skills/babysit-pr/SKILL.md`
- `/home/k3/git/babysit-pr-skill/SKILL.md`
- `/home/k3/git/babysit-pr-skill/README.md`

Those updates should teach the skill to request resumable background behavior when it intentionally hands a watcher or long poller back to the runtime.

## Cross-repo contract change

There is already contract drift between:

- the in-repo `codex` babysit skill, which assumes same-turn ownership of a live watcher
- the standalone `babysit-pr-skill` repo, whose current watch contract exits on non-passive states

The implementation work should align these two views around the new runtime capability:

- Codex runtime gains resumable background completion hooks
- skill guidance explains when to rely on them
- watcher docs describe when the process is intentionally backgrounded and when same-turn ownership still applies

The docs should not claim automatic resume unless the command is launched with the resumable completion behavior.

## Error handling

The new path should be defensive:

- if hook serialization fails, emit hook failure events and continue with the default model-visible completion note
- if hook execution fails, surface the failure in hook notifications and continue unless the hook explicitly stops the follow-up path
- if the owning thread no longer exists, drop the follow-up quietly after emitting the regular `ExecCommandEnd`
- if the process output is huge, bound the notification tail before queueing
- if duplicate completion notifications are observed for the same `process_id` and `call_id`, process only one

## Testing strategy

Follow test-first implementation.

### `codex` repo

Add targeted tests for:

- new hook discovery and matcher behavior for `BackgroundProcessCompleted`
- hook scope and schema export updates
- `exec_command` argument parsing and defaulting for `completion_behavior`
- background completion with `completion_behavior = auto` waking an idle thread
- background completion with `completion_behavior = auto` not enqueueing follow-up while the thread is active
- background completion with `completion_behavior = wake` queueing follow-up for the next turn when the thread is active
- background completion with `completion_behavior = ignore` suppressing wake-up behavior
- no duplicate follow-up for short-lived commands that complete within the original turn

Prefer targeted crate tests over workspace-wide runs.

### `babysit-pr-skill` repo

Add or update tests and docs so the skill contract explicitly reflects:

- when backgrounded monitoring is allowed
- when resumable completion behavior is required
- what the skill should do after being resumed by a background completion

The repo should keep its emitted watch-output contract stable unless the new runtime design requires a documented change.

## Documentation updates

At minimum update:

- hook schema fixtures
- app-server protocol hook exports
- any docs that enumerate supported hook event names
- docs or examples for `exec_command`
- the two `babysit-pr` skill surfaces noted above

## Non-goals

This design does not:

- interrupt active turns on background completion
- change the semantics of existing hook events
- promise that every background process will always reopen a thread by default
- redesign the `babysit-pr` watch payload itself

## Recommendation

Proceed with `BackgroundProcessCompleted` plus `exec_command.completion_behavior`, defaulting omitted values to `auto`.

That keeps the base behavior safe, gives the model an explicit `wake` escape hatch for important long-running tasks, and provides a clear hook contract that can be documented and tested across both the runtime and the `babysit-pr` skill.

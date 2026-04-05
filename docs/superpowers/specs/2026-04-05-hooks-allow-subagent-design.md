# Hooks allowSubagent Design

## Goal

Allow any `hooks.json` handler to opt out of running inside subagent sessions by adding an
optional `allowSubagent` flag with default `true`.

Example:

```json
{
  "type": "command",
  "command": "powershell.exe -NoProfile -ExecutionPolicy Bypass -File \"C:\\Users\\k2\\.codex\\hooks\\session-start-superpowers.ps1\"",
  "statusMessage": "loading superpowers",
  "timeout": 600,
  "allowSubagent": false
}
```

When `allowSubagent` is omitted, behavior must stay exactly as it is today.

## Current State

Every spawned subagent creates a fresh session/thread, and that session goes through the same
hook lifecycle as a primary session. In particular:

- `SessionStart` runs for subagents because new and forked sessions both map to the existing
  `startup` source.
- `PreToolUse`, `PostToolUse`, `UserPromptSubmit`, and `Stop` also use the same configured
  handlers regardless of whether the session is a primary thread or a subagent.

Today there is no hook configuration field that means "do not run this handler for subagents".
The existing `matcher` support is event-specific and is already used for other concepts
(`startup|resume` for `SessionStart`, tool names for tool hooks), so overloading it for
subagent filtering would be confusing and brittle.

## Chosen Design

Add `allowSubagent` as a general per-handler configuration field in `hooks.json`.

### Configuration shape

Extend every hook handler config variant with:

- JSON name: `allowSubagent`
- Rust field: `allow_subagent`
- type: `bool`
- default: `true`

Even though only `command` handlers are supported today, the field should be accepted on every
handler variant so the config shape stays coherent if `prompt` or `agent` handlers are ever
implemented.

### Runtime model

Carry the resolved flag into the discovered handler model:

- add `allow_subagent: bool` to `ConfiguredHandler`
- preserve existing behavior by defaulting it to `true` during deserialization/discovery

Subagent detection should be explicit at runtime rather than inferred from hook payload text.
To support that cleanly across all hook events, add an `is_subagent: bool` field to every hook
request type used by the hooks engine:

- `SessionStartRequest`
- `PreToolUseRequest`
- `PostToolUseRequest`
- `UserPromptSubmitRequest`
- `StopRequest`

`codex-core` should set `is_subagent` from the session source using
`matches!(turn_context.session_source, SessionSource::SubAgent(_))`.

### Handler selection

Filter handlers during hook preview and execution using both:

- the existing event matcher logic
- the new subagent policy

Selection rule:

- primary session: all matching handlers are eligible
- subagent session: only matching handlers with `allowSubagent != false` are eligible

This filtering must happen inside the hooks engine, not in the hook command script, so the
feature works uniformly for all hook types and keeps unsupported handlers from appearing in
preview UI when they will not actually run.

### Hook payloads

Do not change the JSON payload sent to hook commands for this feature.

In particular:

- keep `SessionStart.source` as `startup|resume`
- do not add `is_subagent` or `session_source` to the command input schema as part of this
  change

This keeps the external hook wire format stable and makes `allowSubagent` a pure execution
policy change.

## Validation

Add coverage in the hooks crate and core integration tests.

### Hooks crate

- discovery/config tests showing `allowSubagent` defaults to `true`
- discovery/config tests showing `allowSubagent: false` is preserved on discovered handlers
- dispatcher/event selection tests showing a subagent request skips handlers with
  `allowSubagent: false`
- tests should cover at least `SessionStart` and one non-session event so the behavior is proven
  to be general rather than session-start-specific

### Core integration

Add or update a test that spawns a subagent with hooks enabled and verifies:

- the same hook runs in the primary session
- the hook does not run in the subagent when configured with `allowSubagent: false`

The regression should prove the behavior end-to-end from `hooks.json` parsing through subagent
session startup.

## Alternatives Considered

### 1. Encode subagent filtering in `matcher`

Rejected because `matcher` already has event-specific meaning and does not currently receive a
stable "subagent" input for every event.

### 2. Add subagent metadata to hook command payloads and let scripts decide

Rejected for this change because it expands the public hook wire format and still forces every
hook author to reimplement the same filter in shell code.

### 3. Add a global "disable hooks for subagents" switch

Rejected because it is too coarse. The requirement is per-hook control.

## Out of Scope

- changing the shell or executor used for hooks
- changing the meaning of `SessionStart.source`
- disabling subagents entirely
- adding hook payload fields for subagent metadata
- implementing `prompt` or `agent` hook execution

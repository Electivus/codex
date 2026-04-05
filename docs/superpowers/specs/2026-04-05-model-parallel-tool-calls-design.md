# Model Parallel Tool Calls Design

## Goal

Add a first-class configuration knob that lets users override model parallel tool calling behavior in the same configuration pipeline used for `model` and `model_reasoning_effort`.

## Problem

Today `parallel_tool_calls` in Responses requests is derived from `turn_context.model_info.supports_parallel_tool_calls`. That value comes from model catalog metadata, which means users can only change it indirectly by replacing the model catalog with `model_catalog_json`.

That works, but it is not ergonomic and it does not match how nearby model controls are exposed. Users expect a direct config setting at the same level as `model` and `model_reasoning_effort`.

## Proposed API

Add `model_parallel_tool_calls: Option<bool>` to the user-facing config pipeline.

Supported semantics:

- `None`: preserve existing behavior from model metadata.
- `Some(false)`: force Responses requests to send `parallel_tool_calls = false`.
- `Some(true)`: force Responses requests to send `parallel_tool_calls = true`.

The new field should be supported in:

- top-level `config.toml`
- named profiles
- in-memory `Config`
- `ModelsManagerConfig`

## Architecture

The override should be applied when effective `ModelInfo` is constructed, not at the final request callsite.

That keeps the existing downstream code unchanged:

- `codex-rs/core/src/codex.rs` can continue building prompts from `turn_context.model_info.supports_parallel_tool_calls`
- request serialization can continue using `prompt.parallel_tool_calls`

By applying the override during model info resolution, all session flows that already inherit config through `Config` and `ModelInfo` automatically see the same behavior, including subagents and derived flows.

## Implementation Notes

1. Add `model_parallel_tool_calls: Option<bool>` to the config/profile types beside the existing model-level settings.
2. Thread the new field through `Config::to_models_manager_config()`.
3. Extend `ModelsManagerConfig` with the same field.
4. In `models-manager/src/model_info.rs`, update `with_config_overrides(...)` to overwrite `model.supports_parallel_tool_calls` when the config field is `Some(...)`.
5. Keep existing prompt/request code unchanged unless a test proves a small follow-on adjustment is required.

## Testing

Add coverage at three levels:

1. `models-manager` unit tests for override semantics:
   - `Some(false)` forces support off
   - `Some(true)` forces support on
   - `None` preserves original model metadata
2. `core` config loading tests that parse the new TOML field from config/profile input.
3. `core` request-path test that verifies a model which normally supports parallel tool calls sends `parallel_tool_calls = false` when the config override is set.

## Documentation

Update user-facing config documentation/comments where `model` and `model_reasoning_effort` are documented so the new field is discoverable and its semantics are clear.

## Scope

This change does not alter per-tool local executor parallelism flags. It only controls the model-facing `parallel_tool_calls` capability override.

# Model Parallel Tool Calls Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a config override that controls model parallel tool calling through the same config pipeline as `model` and `model_reasoning_effort`.

**Architecture:** Add a new optional config field, thread it into `ModelsManagerConfig`, and apply it while constructing effective `ModelInfo` so downstream prompt and request code keeps using resolved model metadata. Verify behavior with unit, config, and request-path tests.

**Tech Stack:** Rust, TOML config loading, serde, codex model metadata pipeline, cargo test

---

### Task 1: Add failing models-manager override tests

**Files:**
- Modify: `codex-rs/models-manager/src/model_info_tests.rs`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn parallel_tool_calls_override_false_disables_support() {
    let mut model = model_info_from_slug("unknown-model");
    model.supports_parallel_tool_calls = true;
    let config = ModelsManagerConfig {
        model_parallel_tool_calls: Some(false),
        ..Default::default()
    };

    let updated = with_config_overrides(model, &config);

    assert!(!updated.supports_parallel_tool_calls);
}

#[test]
fn parallel_tool_calls_override_true_enables_support() {
    let model = model_info_from_slug("unknown-model");
    let config = ModelsManagerConfig {
        model_parallel_tool_calls: Some(true),
        ..Default::default()
    };

    let updated = with_config_overrides(model, &config);

    assert!(updated.supports_parallel_tool_calls);
}

#[test]
fn parallel_tool_calls_override_none_preserves_model_value() {
    let mut model = model_info_from_slug("unknown-model");
    model.supports_parallel_tool_calls = true;
    let config = ModelsManagerConfig::default();

    let updated = with_config_overrides(model.clone(), &config);

    assert_eq!(updated.supports_parallel_tool_calls, model.supports_parallel_tool_calls);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p codex-models-manager parallel_tool_calls_override`
Expected: FAIL because `ModelsManagerConfig` does not yet have `model_parallel_tool_calls` and `with_config_overrides(...)` does not apply it.

- [ ] **Step 3: Write minimal implementation**

```rust
#[derive(Debug, Clone, Default)]
pub struct ModelsManagerConfig {
    pub model_context_window: Option<i64>,
    pub model_auto_compact_token_limit: Option<i64>,
    pub tool_output_token_limit: Option<usize>,
    pub base_instructions: Option<String>,
    pub personality_enabled: bool,
    pub model_supports_reasoning_summaries: Option<bool>,
    pub model_parallel_tool_calls: Option<bool>,
    pub model_catalog: Option<ModelsResponse>,
}
```

```rust
if let Some(parallel_tool_calls) = config.model_parallel_tool_calls {
    model.supports_parallel_tool_calls = parallel_tool_calls;
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p codex-models-manager parallel_tool_calls_override`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add codex-rs/models-manager/src/config.rs codex-rs/models-manager/src/model_info.rs codex-rs/models-manager/src/model_info_tests.rs
git commit -m "feat: add model parallel tool calls override"
```

### Task 2: Add failing config parsing tests

**Files:**
- Modify: `codex-rs/core/src/config/config_tests.rs`
- Modify: `codex-rs/core/src/config/mod.rs`
- Modify: `codex-rs/core/src/config/profile.rs`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn config_loads_model_parallel_tool_calls_from_top_level_toml() -> std::io::Result<()> {
    let cfg = ConfigToml {
        model_parallel_tool_calls: Some(false),
        ..Default::default()
    };

    let config = load_config_with_cli_overrides_for_test(cfg)?;

    assert_eq!(config.model_parallel_tool_calls, Some(false));
    Ok(())
}

#[test]
fn profile_overrides_model_parallel_tool_calls() -> std::io::Result<()> {
    let config = load_profile_backed_config_for_test(
        r#"
[profiles.test-profile]
model_parallel_tool_calls = false
"#,
        "test-profile",
    )?;

    assert_eq!(config.model_parallel_tool_calls, Some(false));
    Ok(())
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p codex-core model_parallel_tool_calls`
Expected: FAIL because config types do not yet expose the new field.

- [ ] **Step 3: Write minimal implementation**

```rust
pub struct Config {
    // ...
    pub model_reasoning_effort: Option<ReasoningEffort>,
    pub model_parallel_tool_calls: Option<bool>,
    pub model_reasoning_summary: Option<ReasoningSummary>,
    // ...
}
```

```rust
pub struct ConfigProfile {
    pub model: Option<String>,
    pub model_reasoning_effort: Option<ReasoningEffort>,
    pub model_parallel_tool_calls: Option<bool>,
    pub model_reasoning_summary: Option<ReasoningSummary>,
    // ...
}
```

```rust
model_parallel_tool_calls: config_profile
    .model_parallel_tool_calls
    .or(cfg.model_parallel_tool_calls),
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p codex-core model_parallel_tool_calls`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add codex-rs/core/src/config/config_tests.rs codex-rs/core/src/config/mod.rs codex-rs/core/src/config/profile.rs
git commit -m "feat: parse model parallel tool calls config"
```

### Task 3: Add failing request-path coverage

**Files:**
- Modify: `codex-rs/core/src/client_tests.rs` or a nearby request-path test file
- Modify: `codex-rs/core/src/config/mod.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[tokio::test]
async fn responses_request_disables_parallel_tool_calls_when_config_forces_it() {
    let mut config = test_config();
    config.model_parallel_tool_calls = Some(false);

    let model_info = construct_model_info_offline("gpt-5-codex", &config);

    assert!(!model_info.supports_parallel_tool_calls);

    let prompt = Prompt {
        input: vec![],
        tools: vec![],
        parallel_tool_calls: model_info.supports_parallel_tool_calls,
        base_instructions: BaseInstructions {
            text: "test".to_string(),
        },
        personality: None,
        output_schema: None,
    };

    assert!(!prompt.parallel_tool_calls);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p codex-core responses_request_disables_parallel_tool_calls_when_config_forces_it -- --exact`
Expected: FAIL until `Config::to_models_manager_config()` threads the new field into effective model resolution.

- [ ] **Step 3: Write minimal implementation**

```rust
ModelsManagerConfig {
    model_context_window: self.model_context_window,
    model_auto_compact_token_limit: self.model_auto_compact_token_limit,
    tool_output_token_limit: self.tool_output_token_limit,
    base_instructions: self.base_instructions.clone(),
    personality_enabled: self.features.enabled(Feature::Personality),
    model_supports_reasoning_summaries: self.model_supports_reasoning_summaries,
    model_parallel_tool_calls: self.model_parallel_tool_calls,
    model_catalog: self.model_catalog.clone(),
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p codex-core responses_request_disables_parallel_tool_calls_when_config_forces_it -- --exact`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add codex-rs/core/src/client_tests.rs codex-rs/core/src/config/mod.rs
git commit -m "feat: apply parallel tool calls override to requests"
```

### Task 4: Update docs and verify

**Files:**
- Modify: `codex-rs/core/src/config/mod.rs`
- Modify: documentation comments or nearby user-facing docs that describe model settings

- [ ] **Step 1: Write the doc update**

```rust
/// Optional override for model parallel tool calling support.
/// When set, this overrides the selected model metadata for `parallel_tool_calls`.
pub model_parallel_tool_calls: Option<bool>,
```

- [ ] **Step 2: Run formatting**

Run: `just fmt`
Expected: formatting completes successfully

- [ ] **Step 3: Run crate tests**

Run: `cargo test -p codex-models-manager`
Expected: PASS

- [ ] **Step 4: Run core targeted tests**

Run: `cargo test -p codex-core model_parallel_tool_calls`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add docs/superpowers/specs/2026-04-05-model-parallel-tool-calls-design.md docs/superpowers/plans/2026-04-05-model-parallel-tool-calls.md codex-rs/core/src/config/mod.rs
git commit -m "docs: describe model parallel tool calls override"
```

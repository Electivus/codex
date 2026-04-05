# Memories Phase Reasoning Effort Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the memories pipeline reasoning effort configurable per phase through `[memories]` in `config.toml`, while preserving the current defaults of `low` for extraction and `medium` for consolidation.

**Architecture:** Extend `codex-config` memories settings with two new reasoning-effort fields that resolve to concrete defaults in `MemoriesConfig`. Thread those effective values into the phase-1 request context and the phase-2 subagent config, then cover the new behavior with focused config and memories tests plus the generated config schema and docs update.

**Tech Stack:** Rust, serde/toml, schemars JSON schema generation, tokio tests, `cargo test`, `just`

---

## File Map

- `codex-rs/config/src/types.rs`
  - Defines `MemoriesToml` and `MemoriesConfig`; this is the only place that should own the new config keys and their effective defaults.
- `codex-rs/core/src/config/config_tests.rs`
  - Holds config parsing/defaulting coverage; add focused tests for the new `[memories]` keys here.
- `codex-rs/core/src/memories/phase1.rs`
  - Builds the phase-1 request context; replace the hardcoded reasoning effort here.
- `codex-rs/core/src/memories/phase1_tests.rs`
  - Unit-test the phase-1 request-context wiring here.
- `codex-rs/core/src/memories/phase2.rs`
  - Builds the consolidation subagent config; replace the hardcoded reasoning effort here.
- `codex-rs/core/src/memories/tests.rs`
  - Contains the phase-2 dispatch harness; add the integration test for subagent reasoning effort here.
- `codex-rs/core/config.schema.json`
  - Generated config schema fixture that must be refreshed after changing `MemoriesToml`.
- `docs/config.md`
  - Add a short user-facing note describing the new `[memories]` keys and their defaults.

### Task 1: Add Config Fields And Effective Defaults

**Files:**
- Modify: `codex-rs/core/src/config/config_tests.rs`
- Modify: `codex-rs/config/src/types.rs`
- Test: `codex-rs/core/src/config/config_tests.rs`

- [ ] **Step 1: Write the failing config test for the new memories keys**

```rust
#[test]
fn memories_reasoning_effort_defaults_and_overrides() {
    let parsed = toml::from_str::<ConfigToml>(
        r#"
[memories]
extract_reasoning_effort = "high"
consolidation_reasoning_effort = "low"
"#,
    )
    .expect("TOML deserialization should succeed");

    assert_eq!(
        parsed.memories,
        Some(MemoriesToml {
            no_memories_if_mcp_or_web_search: None,
            generate_memories: None,
            use_memories: None,
            max_raw_memories_for_consolidation: None,
            max_unused_days: None,
            max_rollout_age_days: None,
            max_rollouts_per_startup: None,
            min_rollout_idle_hours: None,
            extract_model: None,
            consolidation_model: None,
            extract_reasoning_effort: Some(ReasoningEffort::High),
            consolidation_reasoning_effort: Some(ReasoningEffort::Low),
        })
    );

    let defaults = Config::load_from_base_config_with_overrides(
        ConfigToml::default(),
        ConfigOverrides::default(),
        tempdir().expect("tempdir").path().to_path_buf(),
    )
    .expect("load default config");
    assert_eq!(defaults.memories.extract_reasoning_effort, ReasoningEffort::Low);
    assert_eq!(
        defaults.memories.consolidation_reasoning_effort,
        ReasoningEffort::Medium
    );
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p codex-core memories_reasoning_effort_defaults_and_overrides -- --exact`

Expected: FAIL because `MemoriesToml` and `MemoriesConfig` do not yet define `extract_reasoning_effort` and `consolidation_reasoning_effort`.

- [ ] **Step 3: Implement the new TOML fields and effective defaults**

```rust
use codex_protocol::openai_models::ReasoningEffort;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MemoriesToml {
    /// When `true`, web searches and MCP tool calls mark the thread `memory_mode` as `"polluted"`.
    pub no_memories_if_mcp_or_web_search: Option<bool>,
    /// When `false`, newly created threads are stored with `memory_mode = "disabled"` in the state DB.
    pub generate_memories: Option<bool>,
    /// When `false`, skip injecting memory usage instructions into developer prompts.
    pub use_memories: Option<bool>,
    /// Maximum number of recent raw memories retained for global consolidation.
    pub max_raw_memories_for_consolidation: Option<usize>,
    /// Maximum number of days since a memory was last used before it becomes ineligible for phase 2 selection.
    pub max_unused_days: Option<i64>,
    /// Maximum age of the threads used for memories.
    pub max_rollout_age_days: Option<i64>,
    /// Maximum number of rollout candidates processed per pass.
    pub max_rollouts_per_startup: Option<usize>,
    /// Minimum idle time between last thread activity and memory creation (hours). > 12h recommended.
    pub min_rollout_idle_hours: Option<i64>,
    /// Model used for thread summarisation.
    pub extract_model: Option<String>,
    /// Model used for memory consolidation.
    pub consolidation_model: Option<String>,
    /// Reasoning effort used for phase-1 memory extraction.
    pub extract_reasoning_effort: Option<ReasoningEffort>,
    /// Reasoning effort used for phase-2 memory consolidation.
    pub consolidation_reasoning_effort: Option<ReasoningEffort>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoriesConfig {
    pub no_memories_if_mcp_or_web_search: bool,
    pub generate_memories: bool,
    pub use_memories: bool,
    pub max_raw_memories_for_consolidation: usize,
    pub max_unused_days: i64,
    pub max_rollout_age_days: i64,
    pub max_rollouts_per_startup: usize,
    pub min_rollout_idle_hours: i64,
    pub extract_model: Option<String>,
    pub consolidation_model: Option<String>,
    pub extract_reasoning_effort: ReasoningEffort,
    pub consolidation_reasoning_effort: ReasoningEffort,
}

impl Default for MemoriesConfig {
    fn default() -> Self {
        Self {
            no_memories_if_mcp_or_web_search: false,
            generate_memories: true,
            use_memories: true,
            max_raw_memories_for_consolidation: DEFAULT_MEMORIES_MAX_RAW_MEMORIES_FOR_CONSOLIDATION,
            max_unused_days: DEFAULT_MEMORIES_MAX_UNUSED_DAYS,
            max_rollout_age_days: DEFAULT_MEMORIES_MAX_ROLLOUT_AGE_DAYS,
            max_rollouts_per_startup: DEFAULT_MEMORIES_MAX_ROLLOUTS_PER_STARTUP,
            min_rollout_idle_hours: DEFAULT_MEMORIES_MIN_ROLLOUT_IDLE_HOURS,
            extract_model: None,
            consolidation_model: None,
            extract_reasoning_effort: ReasoningEffort::Low,
            consolidation_reasoning_effort: ReasoningEffort::Medium,
        }
    }
}

impl From<MemoriesToml> for MemoriesConfig {
    fn from(toml: MemoriesToml) -> Self {
        let defaults = Self::default();
        Self {
            no_memories_if_mcp_or_web_search: toml
                .no_memories_if_mcp_or_web_search
                .unwrap_or(defaults.no_memories_if_mcp_or_web_search),
            generate_memories: toml.generate_memories.unwrap_or(defaults.generate_memories),
            use_memories: toml.use_memories.unwrap_or(defaults.use_memories),
            max_raw_memories_for_consolidation: toml
                .max_raw_memories_for_consolidation
                .unwrap_or(defaults.max_raw_memories_for_consolidation)
                .min(4096),
            max_unused_days: toml
                .max_unused_days
                .unwrap_or(defaults.max_unused_days)
                .clamp(0, 365),
            max_rollout_age_days: toml
                .max_rollout_age_days
                .unwrap_or(defaults.max_rollout_age_days)
                .clamp(0, 90),
            max_rollouts_per_startup: toml
                .max_rollouts_per_startup
                .unwrap_or(defaults.max_rollouts_per_startup)
                .min(128),
            min_rollout_idle_hours: toml
                .min_rollout_idle_hours
                .unwrap_or(defaults.min_rollout_idle_hours)
                .clamp(1, 48),
            extract_model: toml.extract_model,
            consolidation_model: toml.consolidation_model,
            extract_reasoning_effort: toml
                .extract_reasoning_effort
                .unwrap_or(defaults.extract_reasoning_effort),
            consolidation_reasoning_effort: toml
                .consolidation_reasoning_effort
                .unwrap_or(defaults.consolidation_reasoning_effort),
        }
    }
}
```

- [ ] **Step 4: Run the config test to verify it passes**

Run: `cargo test -p codex-core memories_reasoning_effort_defaults_and_overrides -- --exact`

Expected: PASS with the new `MemoriesToml` fields deserializing and the default `MemoriesConfig` values resolving to `low` and `medium`.

- [ ] **Step 5: Commit the config change**

```bash
git add codex-rs/config/src/types.rs codex-rs/core/src/config/config_tests.rs
git commit -m "feat: add memories reasoning effort config"
```

### Task 2: Wire Phase 1 To The Configured Extract Effort

**Files:**
- Modify: `codex-rs/core/src/memories/phase1_tests.rs`
- Modify: `codex-rs/core/src/memories/phase1.rs`
- Test: `codex-rs/core/src/memories/phase1_tests.rs`

- [ ] **Step 1: Write the failing phase-1 wiring test**

```rust
use crate::codex::make_session_and_context;
use crate::config::test_config;
use codex_protocol::openai_models::ReasoningEffort;
use std::sync::Arc;

#[tokio::test]
async fn build_request_context_uses_memories_extract_reasoning_effort() {
    let (session, _turn_context) = make_session_and_context().await;
    let mut config = test_config();
    config.memories.extract_reasoning_effort = ReasoningEffort::High;

    let context = build_request_context(&Arc::new(session), &config).await;

    assert_eq!(context.reasoning_effort, Some(ReasoningEffort::High));
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p codex-core build_request_context_uses_memories_extract_reasoning_effort -- --exact`

Expected: FAIL because `build_request_context()` still hardcodes phase 1 to `phase_one::REASONING_EFFORT`.

- [ ] **Step 3: Replace the hardcoded phase-1 effort with the effective memories config**

```rust
impl RequestContext {
    pub(in crate::memories) fn from_turn_context(
        turn_context: &TurnContext,
        turn_metadata_header: Option<String>,
        model_info: ModelInfo,
        reasoning_effort: ReasoningEffortConfig,
    ) -> Self {
        Self {
            model_info,
            turn_metadata_header,
            session_telemetry: turn_context.session_telemetry.clone(),
            reasoning_effort: Some(reasoning_effort),
            reasoning_summary: turn_context.reasoning_summary,
            service_tier: turn_context.config.service_tier,
        }
    }
}

async fn build_request_context(session: &Arc<Session>, config: &Config) -> RequestContext {
    let model_name = config
        .memories
        .extract_model
        .clone()
        .unwrap_or(phase_one::MODEL.to_string());
    let model = session
        .services
        .models_manager
        .get_model_info(&model_name, &config.to_models_manager_config())
        .await;
    let turn_context = session.new_default_turn().await;
    RequestContext::from_turn_context(
        turn_context.as_ref(),
        turn_context.turn_metadata_state.current_header_value(),
        model,
        config.memories.extract_reasoning_effort,
    )
}
```

- [ ] **Step 4: Run the phase-1 test to verify it passes**

Run: `cargo test -p codex-core build_request_context_uses_memories_extract_reasoning_effort -- --exact`

Expected: PASS with `context.reasoning_effort == Some(ReasoningEffort::High)`.

- [ ] **Step 5: Commit the phase-1 wiring**

```bash
git add codex-rs/core/src/memories/phase1.rs codex-rs/core/src/memories/phase1_tests.rs
git commit -m "feat: respect memories extract reasoning effort"
```

### Task 3: Wire Phase 2 To The Configured Consolidation Effort

**Files:**
- Modify: `codex-rs/core/src/memories/tests.rs`
- Modify: `codex-rs/core/src/memories/phase2.rs`
- Test: `codex-rs/core/src/memories/tests.rs`

- [ ] **Step 1: Write the failing phase-2 integration test**

```rust
#[tokio::test]
async fn dispatch_uses_configured_consolidation_reasoning_effort() {
    let mut config = test_config();
    config.memories.consolidation_reasoning_effort = ReasoningEffort::High;
    let harness = DispatchHarness::with_config(config).await;
    harness.seed_stage1_output(Utc::now().timestamp()).await;

    phase2::run(&harness.session, Arc::clone(&harness.config)).await;

    let thread_ids = harness.manager.list_thread_ids().await;
    pretty_assertions::assert_eq!(thread_ids.len(), 1);
    let subagent = harness
        .manager
        .get_thread(thread_ids[0])
        .await
        .expect("get consolidation thread");
    let config_snapshot = subagent.config_snapshot().await;
    pretty_assertions::assert_eq!(
        config_snapshot.model_reasoning_effort,
        Some(ReasoningEffort::High)
    );

    harness.shutdown_threads().await;
}
```

Add the helper used by the test in the same module:

```rust
impl DispatchHarness {
    async fn with_config(mut config: Config) -> Self {
        let codex_home = tempfile::tempdir().expect("create temp codex home");
        config.codex_home = codex_home.path().to_path_buf();
        config.cwd = config.codex_home.abs();
        let config = Arc::new(config);

        let state_db = codex_state::StateRuntime::init(
            config.codex_home.clone(),
            config.model_provider_id.clone(),
        )
        .await
        .expect("initialize state db");

        let manager = ThreadManager::with_models_provider_and_home_for_tests(
            CodexAuth::from_api_key("dummy"),
            config.model_provider.clone(),
            config.codex_home.clone(),
            std::sync::Arc::new(codex_exec_server::EnvironmentManager::new(
                /*exec_server_url*/ None,
            )),
        );
        let (mut session, _turn_context) = make_session_and_context().await;
        session.services.state_db = Some(Arc::clone(&state_db));
        session.services.agent_control = manager.agent_control();

        Self {
            _codex_home: codex_home,
            config,
            session: Arc::new(session),
            manager,
            state_db,
        }
    }

    async fn new() -> Self {
        Self::with_config(test_config()).await
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p codex-core dispatch_uses_configured_consolidation_reasoning_effort -- --exact`

Expected: FAIL because phase 2 still sets `agent_config.model_reasoning_effort` to the hardcoded `phase_two::REASONING_EFFORT`.

- [ ] **Step 3: Replace the hardcoded phase-2 effort with the effective memories config**

```rust
agent_config.model = Some(
    config
        .memories
        .consolidation_model
        .clone()
        .unwrap_or(phase_two::MODEL.to_string()),
);
agent_config.model_reasoning_effort = Some(config.memories.consolidation_reasoning_effort);
```

- [ ] **Step 4: Run the phase-2 integration test to verify it passes**

Run: `cargo test -p codex-core dispatch_uses_configured_consolidation_reasoning_effort -- --exact`

Expected: PASS with the spawned consolidation subagent carrying `Some(ReasoningEffort::High)` in its config snapshot.

- [ ] **Step 5: Commit the phase-2 wiring**

```bash
git add codex-rs/core/src/memories/phase2.rs codex-rs/core/src/memories/tests.rs
git commit -m "feat: respect memories consolidation reasoning effort"
```

### Task 4: Refresh Schema, Update Docs, And Verify Crates

**Files:**
- Modify: `docs/config.md`
- Modify: `codex-rs/core/config.schema.json`
- Test: `codex-rs/core/src/config/schema_tests.rs`

- [ ] **Step 1: Run the schema fixture test to verify the generated schema is stale**

Run: `cargo test -p codex-core config_schema_matches_fixture -- --exact`

Expected: FAIL with a diff that shows the two new `MemoriesToml` properties are missing from `codex-rs/core/config.schema.json`.

- [ ] **Step 2: Regenerate the config schema**

Run: `just write-config-schema`

Expected: `codex-rs/core/config.schema.json` is rewritten with `extract_reasoning_effort` and `consolidation_reasoning_effort` under `MemoriesToml`.

- [ ] **Step 3: Update the user-facing config docs**

Add this section near the other config notes in `docs/config.md`:

```md
## Memories phase reasoning effort

Under `[memories]`, `extract_reasoning_effort` controls phase-1 rollout
extraction and `consolidation_reasoning_effort` controls phase-2 memory
consolidation.

When unset, Codex keeps the current built-in defaults:

- `extract_reasoning_effort = "low"`
- `consolidation_reasoning_effort = "medium"`
```

- [ ] **Step 4: Re-run the schema test to verify it passes**

Run: `cargo test -p codex-core config_schema_matches_fixture -- --exact`

Expected: PASS with no schema diff.

- [ ] **Step 5: Run targeted crate verification before formatting**

Run: `cargo test -p codex-config`

Expected: PASS.

Run: `cargo test -p codex-core`

Expected: PASS for the changed core crate.

Note: because this work touches `codex-core`, ask the user before running the full workspace `cargo test` / `just test` suite.

- [ ] **Step 6: Run Rust formatting and scoped lint fixes**

Run: `cd codex-rs && just fmt`

Expected: formatting completes successfully.

Run: `cd codex-rs && just fix -p codex-core`

Expected: scoped lint fixes complete successfully.

Do not re-run tests after `just fmt` or `just fix -p codex-core`; follow the repository instruction as written.

- [ ] **Step 7: Commit the schema and docs update**

```bash
git add docs/config.md codex-rs/core/config.schema.json
git commit -m "docs: document memories reasoning effort config"
```

## Self-Review

- Spec coverage: the plan covers config shape/defaults, phase-1 wiring, phase-2 wiring, tests, schema refresh, and docs.
- Placeholder scan: no unresolved placeholders or deferred implementation markers remain in the task steps.
- Type consistency: the same field names are used throughout the plan:
  - `extract_reasoning_effort`
  - `consolidation_reasoning_effort`
  - `config.memories.extract_reasoning_effort`
  - `config.memories.consolidation_reasoning_effort`

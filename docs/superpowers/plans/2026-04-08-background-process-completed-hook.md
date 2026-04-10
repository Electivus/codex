# Background Process Completed Hook Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a first-class `BackgroundProcessCompleted` hook plus `exec_command.completion_behavior`, so late unified-exec completions can wake idle threads, queue follow-up work safely, and teach both bundled and standalone `babysit-pr` skills when to rely on the new runtime path.

**Architecture:** Keep synchronous `PostToolUse` behavior unchanged and model late completions as a separate session-scoped pending-work record owned by the launching thread. The unified-exec async watcher should enqueue that record only for true late completions, the turn-start pipeline should run the new thread-scoped hook before recording a compact developer/runtime note, and the wake policy should be controlled by an explicit `completion_behavior` enum with `auto`, `wake`, and `ignore`.

**Tech Stack:** Rust, `codex-core`, `codex-hooks`, `codex-tools`, `codex-protocol`, `codex-app-server-protocol`, JSON schema fixtures, Python `unittest`, Markdown, YAML

---

## File Map

- `codex-rs/protocol/src/protocol.rs`
  Adds `HookEventName::BackgroundProcessCompleted` to the core protocol surface exported to other crates.
- `codex-rs/app-server-protocol/src/protocol/v2.rs`
  Mirrors the new hook enum variant into the v2 app-server surface and schema fixtures.
- `codex-rs/hooks/src/events/background_process_completed.rs`
  New hook request/outcome module for previewing, serializing, running, and parsing the new event.
- `codex-rs/hooks/src/events/common.rs`
  Teaches matcher support that this new event matches on the final command string.
- `codex-rs/hooks/src/events/mod.rs`
  Registers the new event module.
- `codex-rs/hooks/src/engine/config.rs`
  Parses `BackgroundProcessCompleted` handler groups from `hooks.json`.
- `codex-rs/hooks/src/engine/discovery.rs`
  Discovers handlers for the new event and validates matcher usage.
- `codex-rs/hooks/src/engine/dispatcher.rs`
  Marks the new event as thread-scoped and matcher-aware.
- `codex-rs/hooks/src/engine/mod.rs`
  Exposes preview/run entry points for the new event.
- `codex-rs/hooks/src/registry.rs`
  Wires the new preview/run methods through `Hooks`.
- `codex-rs/hooks/src/lib.rs`
  Re-exports the new request/outcome types.
- `codex-rs/hooks/src/schema.rs`
  Adds the new command input/output wire schemas and fixture generation.
- `codex-rs/core/src/background_process_completion.rs`
  New focused module for `CompletionBehavior`, the queued late-completion record, dedupe keying, and developer-note rendering.
- `codex-rs/core/src/state/session.rs`
  Stores queued background completion records and exposes enqueue/take helpers.
- `codex-rs/core/src/tools/handlers/unified_exec.rs`
  Parses `completion_behavior` from `exec_command` arguments and forwards it into unified-exec requests.
- `codex-rs/core/src/tools/handlers/unified_exec_tests.rs`
  Covers argument parsing/defaulting and preserves current `PostToolUse` behavior for still-running sessions.
- `codex-rs/core/src/unified_exec/mod.rs`
  Extends `ExecCommandRequest` and `ProcessEntry` so the async watcher knows the completion policy and origin metadata.
- `codex-rs/core/src/unified_exec/process_manager.rs`
  Persists the completion policy on stored processes and passes it into the async watcher without changing the short-lived path.
- `codex-rs/core/src/unified_exec/async_watcher.rs`
  Converts true late completions into queued background-completion records and decides whether to wake the thread now, queue for later, or ignore.
- `codex-rs/core/src/hook_runtime.rs`
  Runs `BackgroundProcessCompleted` hooks and materializes the runtime developer note before the model sees the follow-up turn.
- `codex-rs/core/src/codex.rs`
  Exposes enqueue/take helpers on `Session` and reuses next-turn pending input plumbing without pretending the user said anything.
- `codex-rs/core/src/lib.rs`
  Registers the new focused core module.
- `codex-rs/tools/src/local_tool.rs`
  Adds `completion_behavior` to the public `exec_command` tool schema and description.
- `codex-rs/tools/src/local_tool_tests.rs`
  Locks the tool schema so the new parameter is intentional and documented.
- `codex-rs/core/tests/suite/unified_exec.rs`
  End-to-end regression coverage for idle wake-up, active-turn queue/drop behavior, ignore mode, and the no-duplicate short-lived path.
- `codex-rs/core/tests/suite/hooks.rs`
  End-to-end regression coverage proving `BackgroundProcessCompleted` hooks run with the expected payload and can inject additional context.
- `.codex/skills/babysit-pr/SKILL.md`
  Bundled skill guidance for when to use `completion_behavior = wake` versus same-turn watch ownership.
- `.codex/skills/babysit-pr/agents/openai.yaml`
  Bundled agent prompt guidance so the default babysitter prompt uses the new runtime capability deliberately.
- `<path-to-babysit-pr-skill>/SKILL.md`
  Standalone skill contract update for resumable background monitoring.
- `<path-to-babysit-pr-skill>/README.md`
  Standalone docs update so install-time behavior matches the runtime contract.
- `<path-to-babysit-pr-skill>/agents/openai.yaml`
  Standalone agent prompt alignment with the new `completion_behavior` guidance.
- `<path-to-babysit-pr-skill>/tests/test_skill_contract.py`
  New lightweight documentation-contract test that keeps `SKILL.md`, `README.md`, and the agent prompt aligned on `completion_behavior = wake`.

### Task 1: Add the New Hook Event Surface

**Files:**
- Create: `codex-rs/hooks/src/events/background_process_completed.rs`
- Modify: `codex-rs/protocol/src/protocol.rs`
- Modify: `codex-rs/app-server-protocol/src/protocol/v2.rs`
- Modify: `codex-rs/hooks/src/events/common.rs`
- Modify: `codex-rs/hooks/src/events/mod.rs`
- Modify: `codex-rs/hooks/src/engine/config.rs`
- Modify: `codex-rs/hooks/src/engine/discovery.rs`
- Modify: `codex-rs/hooks/src/engine/dispatcher.rs`
- Modify: `codex-rs/hooks/src/engine/mod.rs`
- Modify: `codex-rs/hooks/src/registry.rs`
- Modify: `codex-rs/hooks/src/lib.rs`
- Modify: `codex-rs/hooks/src/schema.rs`
- Test: `codex-rs/hooks/src/events/background_process_completed.rs`
- Test: `codex-rs/hooks/src/events/common.rs`
- Test: `codex-rs/hooks/src/engine/dispatcher.rs`
- Test: `codex-rs/app-server-protocol/tests/schema_fixtures.rs`

- [ ] **Step 1: Write the failing hook-surface tests**

```rust
#[test]
fn background_process_completed_uses_thread_scope_and_matcher() {
    let handlers = vec![make_handler(
        HookEventName::BackgroundProcessCompleted,
        Some("^cargo (build|test)$"),
        "echo hook",
        /*allow_subagent*/ true,
        /*display_order*/ 0,
    )];

    let selected = select_handlers(
        &handlers,
        HookEventName::BackgroundProcessCompleted,
        Some("cargo test"),
        /*is_subagent*/ false,
    );

    assert_eq!(selected.len(), 1);
    assert_eq!(
        super::scope_for_event(HookEventName::BackgroundProcessCompleted),
        HookScope::Thread,
    );
}

#[test]
fn supported_events_keep_background_process_completed_matchers() {
    assert_eq!(
        matcher_pattern_for_event(
            HookEventName::BackgroundProcessCompleted,
            Some("^cargo test$"),
        ),
        Some("^cargo test$")
    );
}

#[test]
fn background_process_completed_command_input_serializes_expected_fields() {
    let input = BackgroundProcessCompletedCommandInput {
        session_id: "thread-1".to_string(),
        originating_turn_id: "turn-1".to_string(),
        transcript_path: NullableString::from_path(Some("/tmp/rollout.jsonl".into())),
        cwd: "/repo".to_string(),
        hook_event_name: "BackgroundProcessCompleted".to_string(),
        model: "gpt-5".to_string(),
        permission_mode: "never".to_string(),
        call_id: "call-1".to_string(),
        process_id: "1000".to_string(),
        command: "cargo test -p codex-core".to_string(),
        exit_code: 0,
        duration_ms: 1250,
        status: "completed".to_string(),
        completion_behavior: "wake".to_string(),
        is_subagent: false,
        aggregated_output_tail: "test result: ok".to_string(),
    };

    let json = serde_json::to_value(input).expect("serialize hook input");
    assert_eq!(json["hook_event_name"], "BackgroundProcessCompleted");
    assert_eq!(json["command"], "cargo test -p codex-core");
    assert_eq!(json["completion_behavior"], "wake");
}
```

- [ ] **Step 2: Run the targeted hooks and protocol tests to verify they fail**

Run: `cargo test -p codex-hooks background_process_completed_uses_thread_scope_and_matcher`
Expected: FAIL because the new enum variant and event module do not exist yet.

Run: `cargo test -p codex-hooks supported_events_keep_background_process_completed_matchers`
Expected: FAIL because `matcher_pattern_for_event` does not recognize the new event yet.

- [ ] **Step 3: Implement the new hook event plumbing**

```rust
pub enum HookEventName {
    PreToolUse,
    PostToolUse,
    SessionStart,
    BackgroundProcessCompleted,
    UserPromptSubmit,
    Stop,
}

pub struct BackgroundProcessCompletedRequest {
    pub session_id: ThreadId,
    pub originating_turn_id: String,
    pub cwd: PathBuf,
    pub transcript_path: Option<PathBuf>,
    pub model: String,
    pub permission_mode: String,
    pub call_id: String,
    pub process_id: String,
    pub command: String,
    pub exit_code: i32,
    pub duration_ms: i64,
    pub status: String,
    pub completion_behavior: String,
    pub is_subagent: bool,
    pub aggregated_output_tail: String,
}

match event_name {
    HookEventName::PreToolUse
    | HookEventName::PostToolUse
    | HookEventName::SessionStart
    | HookEventName::BackgroundProcessCompleted => matcher,
    HookEventName::UserPromptSubmit | HookEventName::Stop => None,
}

match event_name {
    HookEventName::SessionStart | HookEventName::BackgroundProcessCompleted => HookScope::Thread,
    HookEventName::PreToolUse
    | HookEventName::PostToolUse
    | HookEventName::UserPromptSubmit
    | HookEventName::Stop => HookScope::Turn,
}
```

- [ ] **Step 4: Regenerate exported schemas and rerun the targeted crates**

Run: `just write-hooks-schema`
Expected: generated hook schema fixtures include `BackgroundProcessCompleted`.

Run: `just write-app-server-schema`
Expected: generated app-server schema fixtures include the new hook enum variant.

Run: `cargo test -p codex-hooks`
Expected: PASS

Run: `cargo test -p codex-app-server-protocol schema_fixtures`
Expected: PASS

- [ ] **Step 5: Commit the hook-surface slice**

```bash
git add codex-rs/protocol/src/protocol.rs codex-rs/app-server-protocol/src/protocol/v2.rs codex-rs/hooks/src/events/background_process_completed.rs codex-rs/hooks/src/events/common.rs codex-rs/hooks/src/events/mod.rs codex-rs/hooks/src/engine/config.rs codex-rs/hooks/src/engine/discovery.rs codex-rs/hooks/src/engine/dispatcher.rs codex-rs/hooks/src/engine/mod.rs codex-rs/hooks/src/registry.rs codex-rs/hooks/src/lib.rs codex-rs/hooks/src/schema.rs
git commit -m "feat: add background process completed hook surface"
```

### Task 2: Add `completion_behavior` to `exec_command`

**Files:**
- Create: `codex-rs/core/src/background_process_completion.rs`
- Modify: `codex-rs/core/src/lib.rs`
- Modify: `codex-rs/core/src/tools/handlers/unified_exec.rs`
- Modify: `codex-rs/core/src/tools/handlers/unified_exec_tests.rs`
- Modify: `codex-rs/core/src/unified_exec/mod.rs`
- Modify: `codex-rs/tools/src/local_tool.rs`
- Modify: `codex-rs/tools/src/local_tool_tests.rs`

- [ ] **Step 1: Write the failing tool-schema and argument-parsing tests**

```rust
#[test]
fn exec_command_tool_includes_completion_behavior() {
    let tool = create_exec_command_tool(CommandToolOptions {
        allow_login_shell: true,
        exec_permission_approvals_enabled: false,
    });

    let ToolSpec::Function(spec) = tool else {
        panic!("expected function tool");
    };

    assert!(
        spec.parameters
            .properties
            .as_ref()
            .expect("exec_command input schema should stay object-shaped")
            .contains_key("completion_behavior"),
        "exec_command should expose completion_behavior"
    );
}

#[test]
fn exec_command_args_default_completion_behavior_to_auto() -> anyhow::Result<()> {
    let args: ExecCommandArgs = parse_arguments(r#"{"cmd":"cargo test"}"#)?;
    assert_eq!(args.completion_behavior, CompletionBehavior::Auto);
    Ok(())
}

#[test]
fn exec_command_args_accept_explicit_wake_behavior() -> anyhow::Result<()> {
    let args: ExecCommandArgs =
        parse_arguments(r#"{"cmd":"cargo test","completion_behavior":"wake"}"#)?;
    assert_eq!(args.completion_behavior, CompletionBehavior::Wake);
    Ok(())
}
```

- [ ] **Step 2: Run the targeted schema and handler tests to verify they fail**

Run: `cargo test -p codex-tools exec_command_tool_includes_completion_behavior`
Expected: FAIL because the schema does not expose `completion_behavior` yet.

Run: `cargo test -p codex-core exec_command_args_default_completion_behavior_to_auto`
Expected: FAIL because `ExecCommandArgs` does not parse the new field yet.

- [ ] **Step 3: Implement the enum and plumb it into unified-exec requests**

```rust
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CompletionBehavior {
    #[default]
    Auto,
    Wake,
    Ignore,
}

pub(crate) struct ExecCommandArgs {
    cmd: String,
    #[serde(default)]
    pub(crate) workdir: Option<String>,
    #[serde(default)]
    completion_behavior: CompletionBehavior,
    // existing fields...
}

pub(crate) struct ExecCommandRequest {
    pub command: Vec<String>,
    pub process_id: i32,
    pub completion_behavior: CompletionBehavior,
    // existing fields...
}

properties.insert(
    "completion_behavior".to_string(),
    JsonSchema::string(Some(
        "Whether Codex should create a follow-up turn when the process completes after the original turn. Defaults to \"auto\". Use \"wake\" for long-running work Codex intends to revisit, and \"ignore\" for fire-and-forget commands."
            .to_string(),
    )),
);
```

- [ ] **Step 4: Rerun the targeted crates**

Run: `cargo test -p codex-tools exec_command_tool_matches_expected_spec`
Expected: PASS with the new schema field included.

Run: `cargo test -p codex-core exec_command_args_default_completion_behavior_to_auto`
Expected: PASS

Run: `cargo test -p codex-core exec_command_args_accept_explicit_wake_behavior`
Expected: PASS

- [ ] **Step 5: Commit the `exec_command` API slice**

```bash
git add codex-rs/core/src/background_process_completion.rs codex-rs/core/src/lib.rs codex-rs/core/src/tools/handlers/unified_exec.rs codex-rs/core/src/tools/handlers/unified_exec_tests.rs codex-rs/core/src/unified_exec/mod.rs codex-rs/tools/src/local_tool.rs codex-rs/tools/src/local_tool_tests.rs
git commit -m "feat: add unified exec completion behavior"
```

### Task 3: Wake Idle Threads for True Late Completions

**Files:**
- Modify: `codex-rs/core/src/state/session.rs`
- Modify: `codex-rs/core/src/codex.rs`
- Modify: `codex-rs/core/src/unified_exec/process_manager.rs`
- Modify: `codex-rs/core/src/unified_exec/async_watcher.rs`
- Modify: `codex-rs/core/tests/suite/unified_exec.rs`

- [ ] **Step 1: Write the failing idle-wake and no-duplicate regression tests**

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn background_completion_auto_wakes_idle_thread() -> Result<()> {
    let call_id = "uexec-background-auto";
    let args = json!({
        "cmd": "sleep 0.5; printf READY",
        "yield_time_ms": 250,
        "completion_behavior": "auto",
    });

    let responses = vec![
        sse(vec![
            ev_response_created("resp-1"),
            ev_function_call(call_id, "exec_command", &serde_json::to_string(&args)?),
            ev_completed("resp-1"),
        ]),
        sse(vec![
            ev_response_created("resp-2"),
            ev_assistant_message("msg-1", "background completion observed"),
            ev_completed("resp-2"),
        ]),
    ];

    let request_log = mount_sse_sequence(&server, responses).await;
    submit_unified_exec_turn(&test, "run a resumable background command", SandboxPolicy::DangerFullAccess).await?;

    wait_for_event(&test.codex, |event| matches!(event, EventMsg::TurnComplete(_))).await;

    let requests = request_log.requests();
    assert_eq!(requests.len(), 2, "expected a follow-up turn after late completion");
    assert!(
        requests[1]
            .input()
            .iter()
            .any(|item| item.to_string().contains("Background process completed after the previous turn ended")),
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn short_lived_exec_command_does_not_enqueue_background_follow_up() -> Result<()> {
    let call_id = "uexec-short-lived-no-follow-up";
    let args = json!({
        "cmd": "printf short-lived",
        "yield_time_ms": 1000,
        "completion_behavior": "wake",
    });

    let request_log = mount_sse_sequence(&server, vec![
        sse(vec![
            ev_response_created("resp-1"),
            ev_function_call(call_id, "exec_command", &serde_json::to_string(&args)?),
            ev_completed("resp-1"),
        ]),
        sse(vec![
            ev_response_created("resp-2"),
            ev_assistant_message("msg-1", "done"),
            ev_completed("resp-2"),
        ]),
    ]).await;

    submit_unified_exec_turn(&test, "run short lived command", SandboxPolicy::DangerFullAccess).await?;
    wait_for_event(&test.codex, |event| matches!(event, EventMsg::TurnComplete(_))).await;
    assert_eq!(request_log.requests().len(), 2, "short-lived commands must not create a third turn");
    Ok(())
}
```

- [ ] **Step 2: Run the targeted core tests to verify they fail**

Run: `cargo test -p codex-core background_completion_auto_wakes_idle_thread -- --exact`
Expected: FAIL because late completions only emit `ExecCommandEnd` today and do not wake a new turn.

Run: `cargo test -p codex-core short_lived_exec_command_does_not_enqueue_background_follow_up -- --exact`
Expected: FAIL because the new queue type and guard logic do not exist yet.

- [ ] **Step 3: Implement the queued completion record and idle wake path**

```rust
pub(crate) struct BackgroundProcessCompletionRecord {
    pub call_id: String,
    pub process_id: i32,
    pub originating_turn_id: String,
    pub command: String,
    pub exit_code: i32,
    pub duration_ms: i64,
    pub status: String,
    pub completion_behavior: CompletionBehavior,
    pub aggregated_output_tail: String,
    pub is_subagent: bool,
}

impl SessionState {
    pub(crate) fn queue_background_process_completion(
        &mut self,
        record: BackgroundProcessCompletionRecord,
    ) {
        if self
            .pending_background_process_completions
            .iter()
            .any(|existing| existing.call_id == record.call_id && existing.process_id == record.process_id)
        {
            return;
        }
        self.pending_background_process_completions.push(record);
    }
}

match completion_behavior {
    CompletionBehavior::Ignore => return,
    CompletionBehavior::Auto | CompletionBehavior::Wake => {
        session_ref.queue_background_process_completion(record).await;
        session_ref.maybe_start_turn_for_pending_work().await;
    }
}
```

- [ ] **Step 4: Rerun the targeted core tests**

Run: `cargo test -p codex-core background_completion_auto_wakes_idle_thread -- --exact`
Expected: PASS

Run: `cargo test -p codex-core short_lived_exec_command_does_not_enqueue_background_follow_up -- --exact`
Expected: PASS

- [ ] **Step 5: Commit the idle-wake slice**

```bash
git add codex-rs/core/src/state/session.rs codex-rs/core/src/codex.rs codex-rs/core/src/unified_exec/process_manager.rs codex-rs/core/src/unified_exec/async_watcher.rs codex-rs/core/tests/suite/unified_exec.rs
git commit -m "feat: wake idle threads for late exec completions"
```

### Task 4: Implement Active-Turn `auto`/`wake`/`ignore` Semantics

**Files:**
- Modify: `codex-rs/core/src/state/session.rs`
- Modify: `codex-rs/core/src/unified_exec/async_watcher.rs`
- Modify: `codex-rs/core/tests/suite/unified_exec.rs`

- [ ] **Step 1: Write the failing active-turn policy tests**

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn background_completion_auto_drops_when_thread_is_active() -> Result<()> {
    let long_call_id = "uexec-auto-drop";
    let followup_call_id = "uexec-active-turn";

    // First turn starts a background process that exits while the second turn is active.
    // The second turn stays busy via streaming SSE long enough for the first process to finish.
    // `auto` should not enqueue a later background completion turn.

    assert_eq!(request_log.requests().len(), 2, "auto should drop the completion while another turn is active");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn background_completion_wake_queues_after_active_turn_finishes() -> Result<()> {
    let long_call_id = "uexec-wake-queue";

    // Same timing as the previous test, but the original exec_command uses
    // `"completion_behavior": "wake"`. After the active turn finishes, Codex
    // should start one more turn that carries the runtime completion note.

    assert_eq!(request_log.requests().len(), 3, "wake should queue one extra turn after the active turn completes");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn background_completion_ignore_suppresses_follow_up_turns() -> Result<()> {
    let args = json!({
        "cmd": "sleep 0.5; printf IGNORE",
        "yield_time_ms": 250,
        "completion_behavior": "ignore",
    });

    // The regular ExecCommandEnd event should still arrive, but no synthetic
    // follow-up turn should be created.

    assert_eq!(request_log.requests().len(), 1);
    Ok(())
}
```

- [ ] **Step 2: Run the targeted core tests to verify they fail**

Run: `cargo test -p codex-core background_completion_auto_drops_when_thread_is_active -- --exact`
Expected: FAIL because there is no active-turn-specific policy branch yet.

Run: `cargo test -p codex-core background_completion_wake_queues_after_active_turn_finishes -- --exact`
Expected: FAIL because queued completions are not preserved once another turn is active.

Run: `cargo test -p codex-core background_completion_ignore_suppresses_follow_up_turns -- --exact`
Expected: FAIL because `ignore` is not wired in the async watcher yet.

- [ ] **Step 3: Implement the active-turn policy branch in the watcher**

```rust
let has_active_turn = session_ref.active_turn.lock().await.is_some();

match (record.completion_behavior, has_active_turn) {
    (CompletionBehavior::Ignore, _) => {}
    (CompletionBehavior::Auto, false) => {
        session_ref.queue_background_process_completion(record).await;
        session_ref.maybe_start_turn_for_pending_work().await;
    }
    (CompletionBehavior::Auto, true) => {}
    (CompletionBehavior::Wake, false) => {
        session_ref.queue_background_process_completion(record).await;
        session_ref.maybe_start_turn_for_pending_work().await;
    }
    (CompletionBehavior::Wake, true) => {
        session_ref.queue_background_process_completion(record).await;
    }
}
```

- [ ] **Step 4: Rerun the targeted active-turn tests**

Run: `cargo test -p codex-core background_completion_auto_drops_when_thread_is_active -- --exact`
Expected: PASS

Run: `cargo test -p codex-core background_completion_wake_queues_after_active_turn_finishes -- --exact`
Expected: PASS

Run: `cargo test -p codex-core background_completion_ignore_suppresses_follow_up_turns -- --exact`
Expected: PASS

- [ ] **Step 5: Commit the policy semantics slice**

```bash
git add codex-rs/core/src/state/session.rs codex-rs/core/src/unified_exec/async_watcher.rs codex-rs/core/tests/suite/unified_exec.rs
git commit -m "feat: honor late exec completion wake policy"
```

### Task 5: Run the New Hook and Record a Runtime Note

**Files:**
- Modify: `codex-rs/core/src/hook_runtime.rs`
- Modify: `codex-rs/core/src/codex.rs`
- Modify: `codex-rs/core/tests/suite/hooks.rs`
- Modify: `codex-rs/core/tests/suite/unified_exec.rs`

- [ ] **Step 1: Write the failing end-to-end hook integration test**

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn background_process_completed_hook_runs_before_runtime_note() -> Result<()> {
    write_background_process_completed_hook(
        test.codex_home_path(),
        Some("^sleep 0.5; printf HOOKED$"),
        "background hook context",
    )?;

    let requests = request_log.requests();
    let followup_request = requests.last().expect("follow-up request");

    let serialized = serde_json::to_string(followup_request.input()).expect("serialize request");
    assert!(serialized.contains("background hook context"));
    assert!(serialized.contains("Background process completed after the previous turn ended"));
    Ok(())
}
```

- [ ] **Step 2: Run the targeted core hook test to verify it fails**

Run: `cargo test -p codex-core background_process_completed_hook_runs_before_runtime_note -- --exact`
Expected: FAIL because queued background completions are not yet inspected by the hook runtime.

- [ ] **Step 3: Implement hook execution and runtime-note materialization**

```rust
pub(crate) async fn run_background_process_completed_hooks(
    sess: &Arc<Session>,
    turn_context: &Arc<TurnContext>,
    record: &BackgroundProcessCompletionRecord,
) -> HookRuntimeOutcome {
    let request = codex_hooks::BackgroundProcessCompletedRequest {
        session_id: sess.conversation_id,
        originating_turn_id: record.originating_turn_id.clone(),
        cwd: turn_context.cwd.to_path_buf(),
        transcript_path: sess.hook_transcript_path().await,
        model: turn_context.model_info.slug.clone(),
        permission_mode: hook_permission_mode(turn_context),
        call_id: record.call_id.clone(),
        process_id: record.process_id.to_string(),
        command: record.command.clone(),
        exit_code: record.exit_code,
        duration_ms: record.duration_ms,
        status: record.status.clone(),
        completion_behavior: record.completion_behavior.as_str().to_string(),
        is_subagent: record.is_subagent,
        aggregated_output_tail: record.aggregated_output_tail.clone(),
    };

    // Preview, emit started/completed events, and collect additional context.
}

let runtime_note = ResponseInputItem::Message {
    role: "developer".to_string(),
    content: vec![ContentItem::InputText {
        text: record.runtime_note_text(),
    }],
};
```

- [ ] **Step 4: Rerun the end-to-end hook tests**

Run: `cargo test -p codex-core background_process_completed_hook_runs_before_runtime_note -- --exact`
Expected: PASS

Run: `cargo test -p codex-core hooks -- --nocapture`
Expected: PASS for the existing hooks suite plus the new background-completion case.

- [ ] **Step 5: Commit the hook-runtime slice**

```bash
git add codex-rs/core/src/hook_runtime.rs codex-rs/core/src/codex.rs codex-rs/core/tests/suite/hooks.rs codex-rs/core/tests/suite/unified_exec.rs
git commit -m "feat: run background completion hooks before follow-up turns"
```

### Task 6: Update Prompt and Skill Guidance in Both Repositories

**Files:**
- Modify: `.codex/skills/babysit-pr/SKILL.md`
- Modify: `.codex/skills/babysit-pr/agents/openai.yaml`
- Modify: `<path-to-babysit-pr-skill>/SKILL.md`
- Modify: `<path-to-babysit-pr-skill>/README.md`
- Modify: `<path-to-babysit-pr-skill>/agents/openai.yaml`
- Create: `<path-to-babysit-pr-skill>/tests/test_skill_contract.py`

- [ ] **Step 1: Write the failing standalone skill contract test**

```python
import pathlib
import unittest


ROOT = pathlib.Path(__file__).resolve().parent.parent


class SkillContractTests(unittest.TestCase):
    def test_docs_and_agent_prompt_mention_completion_behavior_wake(self):
        skill_md = (ROOT / "SKILL.md").read_text(encoding="utf-8")
        readme = (ROOT / "README.md").read_text(encoding="utf-8")
        agent_yaml = (ROOT / "agents" / "openai.yaml").read_text(encoding="utf-8")

        for text in (skill_md, readme, agent_yaml):
            self.assertIn("completion_behavior = wake", text)
            self.assertIn("--watch", text)


if __name__ == "__main__":
    unittest.main()
```

- [ ] **Step 2: Run the standalone skill tests to verify they fail**

Run: `python3 -m unittest <path-to-babysit-pr-skill>/tests/test_skill_contract.py -v`
Expected: FAIL because the docs and agent prompt do not mention `completion_behavior = wake` yet.

- [ ] **Step 3: Update bundled and standalone skill guidance**

```md
When you intentionally launch a long-running watcher or build/test command that
Codex must revisit after the current turn ends, call `exec_command` with
`completion_behavior = wake`.

Use `completion_behavior = auto` only when a late follow-up would be optional.
Use `completion_behavior = ignore` for fire-and-forget commands.

For `babysit-pr --watch`:
- prefer same-turn watch ownership when you are actively consuming the stream
- if you intentionally hand the watcher back to the runtime, launch it with
  `completion_behavior = wake`
- do not claim automatic resume unless that parameter was set
```

- [ ] **Step 4: Rerun the standalone test and a focused bundled-skill sanity check**

Run: `python3 -m unittest <path-to-babysit-pr-skill>/tests/test_skill_contract.py -v`
Expected: PASS

Run: `python3 -m unittest discover -s <path-to-babysit-pr-skill>/tests -p "test_*.py"`
Expected: PASS

- [ ] **Step 5: Commit the bundled Codex skill updates**

```bash
git add .codex/skills/babysit-pr/SKILL.md .codex/skills/babysit-pr/agents/openai.yaml
git commit -m "docs: teach bundled babysit-pr about resumable watches"
```

- [ ] **Step 6: Commit the standalone `babysit-pr-skill` updates**

```bash
cd <path-to-babysit-pr-skill>
git add SKILL.md README.md agents/openai.yaml tests/test_skill_contract.py
git commit -m "docs: teach babysit-pr-skill about resumable watches"
```

### Task 7: Final Verification, Formatting, and Handoff

**Files:**
- Modify: all files touched above
- Test: `codex-rs/core/tests/suite/unified_exec.rs`
- Test: `codex-rs/core/tests/suite/hooks.rs`
- Test: `codex-rs/tools/src/local_tool_tests.rs`
- Test: `codex-rs/hooks/src/schema.rs`
- Test: `<path-to-babysit-pr-skill>/tests/test_skill_contract.py`

- [ ] **Step 1: Run the focused Rust crate suites**

Run: `cargo test -p codex-hooks`
Expected: PASS

Run: `cargo test -p codex-tools`
Expected: PASS

Run: `cargo test -p codex-app-server-protocol`
Expected: PASS

Run: `cargo test -p codex-core unified_exec`
Expected: PASS

Run: `cargo test -p codex-core hooks`
Expected: PASS

- [ ] **Step 2: Regenerate and verify schema fixtures one last time**

Run: `just write-hooks-schema`
Expected: no diff after regeneration.

Run: `just write-app-server-schema`
Expected: no diff after regeneration.

- [ ] **Step 3: Run formatting and scoped lint fixes**

Run: `cd codex-rs && just fmt`
Expected: PASS

Run: `cd codex-rs && just fix -p codex-hooks`
Expected: PASS

Run: `cd codex-rs && just fix -p codex-tools`
Expected: PASS

Run: `cd codex-rs && just fix -p codex-core`
Expected: PASS

Run: `cd codex-rs && just fix -p codex-protocol`
Expected: PASS

Run: `cd codex-rs && just fix -p codex-app-server-protocol`
Expected: PASS

- [ ] **Step 4: Run the standalone skill suite once more after docs updates**

Run: `python3 -m unittest discover -s <path-to-babysit-pr-skill>/tests -p "test_*.py"`
Expected: PASS

- [ ] **Step 5: Verify both repositories are clean after the slice commits**

```bash
git status --short
cd <path-to-babysit-pr-skill> && git status --short
```

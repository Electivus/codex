# Blocking Subagent Spawn Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `spawn_agent` block by default in both V1 and `multi_agent_v2`, returning control to the parent on any child `TurnComplete` or `TurnAborted`, while preserving an explicit `blocking = false` async opt-out.

**Architecture:** Keep the public tool name unchanged and add a new optional `blocking` parameter with default `true` in both schemas. Implement the blocking behavior inside each spawn handler, not as a model-driven `wait_agent` round-trip: V1 should wait on the child status stream until the first non-running boundary status, while V2 should subscribe to the parent mailbox before spawn, wait for a matching child-boundary notification envelope, and return the observed status along with the child identity. Update `codex.rs` so V2 child-to-parent notifications fire on any turn boundary, not only terminal ones, then align `wait_agent` semantics and tool descriptions to the new default.

**Tech Stack:** Rust, `codex-core`, `codex-tools`, `codex-protocol`, `serde`, `tokio`, Markdown

---

## File Map

- `codex-rs/tools/src/agent_tool.rs`
  Add the new `blocking` parameter to both `spawn_agent` schemas, add `status` to both output schemas, and rewrite the spawn/wait descriptions so the default is clearly blocking.
- `codex-rs/tools/src/agent_tool_tests.rs`
  Lock the V1/V2 tool schemas and descriptions so `blocking` and `status` cannot regress silently.
- `codex-rs/core/src/tools/spec_tests.rs`
  Update the rendered tool-description assertions for the new default wording.
- `codex-rs/core/src/tools/handlers/multi_agents_common.rs`
  Add focused helper logic for “child handoff boundary” status detection that both legacy spawn and legacy wait can share.
- `codex-rs/core/src/tools/handlers/multi_agents/spawn.rs`
  Parse `blocking`, block by default on the child status stream, and return the observed `status` in the V1 result.
- `codex-rs/core/src/tools/handlers/multi_agents/wait.rs`
  Align legacy `wait_agent` with the new handoff semantics so `Interrupted` wakes the parent instead of timing out.
- `codex-rs/core/src/codex.rs`
  Notify the parent on any spawned V2 child turn boundary, not only terminal child turns.
- `codex-rs/core/src/tools/handlers/multi_agents_v2/spawn.rs`
  Parse `blocking`, subscribe to mailbox sequence before spawn to avoid fast-child races, wait for the spawned child’s matching boundary notification, and return `status` in the V2 result.
- `codex-rs/core/src/tools/handlers/multi_agents_v2/wait.rs`
  Expose or host the mailbox-notification matching logic shared with blocking V2 spawn, while keeping the public `wait_agent` response shape unchanged.
- `codex-rs/core/src/tools/handlers/multi_agents_tests.rs`
  Add blocking/non-blocking regression tests for both spawn flows, update the interrupted-notification contract, and align legacy wait expectations.

### Task 1: Lock the Public `spawn_agent` Schema and Tool Copy

**Files:**
- Modify: `codex-rs/tools/src/agent_tool.rs`
- Modify: `codex-rs/tools/src/agent_tool_tests.rs`
- Modify: `codex-rs/core/src/tools/spec_tests.rs`
- Test: `codex-rs/tools/src/agent_tool_tests.rs`
- Test: `codex-rs/core/src/tools/spec_tests.rs`

- [ ] **Step 1: Write the failing schema and description tests first**

```rust
#[test]
fn spawn_agent_tool_v1_exposes_blocking_opt_out_and_status() {
    let tool = create_spawn_agent_tool_v1(SpawnAgentToolOptions {
        available_models: &[],
        agent_type_description: "role help".to_string(),
        hide_agent_type_model_reasoning: false,
        include_usage_hint: true,
        usage_hint_text: None,
    });

    let ToolSpec::Function(ResponsesApiTool {
        description,
        parameters,
        output_schema,
        ..
    }) = tool
    else {
        panic!("spawn_agent should be a function tool");
    };
    let properties = parameters
        .properties
        .as_ref()
        .expect("spawn_agent should use object params");

    assert!(properties.contains_key("blocking"));
    assert!(description.contains("blocks by default"));
    assert!(description.contains("Set `blocking` to false"));
    assert_eq!(
        output_schema.expect("spawn_agent output schema")["required"],
        json!(["agent_id", "nickname", "status"])
    );
}

#[test]
fn spawn_agent_tool_v2_exposes_blocking_opt_out_and_status() {
    let tool = create_spawn_agent_tool_v2(SpawnAgentToolOptions {
        available_models: &[],
        agent_type_description: "role help".to_string(),
        hide_agent_type_model_reasoning: false,
        include_usage_hint: true,
        usage_hint_text: None,
    });

    let ToolSpec::Function(ResponsesApiTool {
        description,
        parameters,
        output_schema,
        ..
    }) = tool
    else {
        panic!("spawn_agent should be a function tool");
    };
    let properties = parameters
        .properties
        .as_ref()
        .expect("spawn_agent should use object params");

    assert!(properties.contains_key("blocking"));
    assert!(description.contains("returns when the child finishes a turn"));
    assert_eq!(
        output_schema.expect("spawn_agent output schema")["required"],
        json!(["task_name", "nickname", "status"])
    );
}
```

- [ ] **Step 2: Run the targeted schema tests to verify they fail for the right reason**

Run: `cargo test -p codex-tools spawn_agent_tool_v1_exposes_blocking_opt_out_and_status`
Expected: FAIL because the V1 schema does not yet include `blocking` or `status`.

Run: `cargo test -p codex-tools spawn_agent_tool_v2_exposes_blocking_opt_out_and_status`
Expected: FAIL because the V2 schema does not yet include `blocking` or `status`.

- [ ] **Step 3: Update the tool schema and descriptions with the new default**

```rust
fn spawn_agent_output_schema_v1() -> Value {
    json!({
        "type": "object",
        "properties": {
            "agent_id": {
                "type": "string",
                "description": "Thread identifier for the spawned agent."
            },
            "nickname": {
                "type": ["string", "null"],
                "description": "User-facing nickname for the spawned agent when available."
            },
            "status": {
                "description": "Child status observed when control returned to the parent.",
                "allOf": [agent_status_output_schema()]
            }
        },
        "required": ["agent_id", "nickname", "status"],
        "additionalProperties": false
    })
}

fn spawn_agent_common_properties_v2(agent_type_description: &str) -> BTreeMap<String, JsonSchema> {
    BTreeMap::from([
        (
            "message".to_string(),
            JsonSchema::string(Some("Initial plain-text task for the new agent.".to_string())),
        ),
        (
            "blocking".to_string(),
            JsonSchema::boolean(Some(
                "When true or omitted, wait until the child agent finishes a turn and hand control back to the parent. Set to false for background async execution."
                    .to_string(),
            )),
        ),
        (
            "agent_type".to_string(),
            JsonSchema::string(Some(agent_type_description.to_string())),
        ),
        (
            "fork_turns".to_string(),
            JsonSchema::string(Some(
                "Optional number of turns to fork. Defaults to `all`. Use `none`, `all`, or a positive integer string such as `3` to fork only the most recent turns."
                    .to_string(),
            )),
        ),
        (
            "model".to_string(),
            JsonSchema::string(Some(
                "Optional model override for the new agent. Replaces the inherited model."
                    .to_string(),
            )),
        ),
        (
            "reasoning_effort".to_string(),
            JsonSchema::string(Some(
                "Optional reasoning effort override for the new agent. Replaces the inherited reasoning effort."
                    .to_string(),
            )),
        ),
    ])
}

let tool_description = format!(
    r#"
        {agent_role_guidance}
        Spawns an agent to work on the specified task. By default this call blocks until the child agent reaches a turn boundary and hands control back to the parent. Set `blocking` to false when you intentionally want background execution."#
);
```

- [ ] **Step 4: Update the rendered-description assertions in `codex-core`**

```rust
assert_regex_match(
    r#"(?sx)
        ^\s*
        No\ picker-visible\ models\ are\ currently\ loaded\.
        \s+Spawns\ an\ agent\ to\ work\ on\ the\ specified\ task\.
        \s+By\ default\ this\ call\ blocks\ until\ the\ child\ agent\ reaches\ a\ turn\ boundary\ and\ hands\ control\ back\ to\ the\ parent\.
        \s+Set\ `blocking`\ to\ false\ when\ you\ intentionally\ want\ background\ execution\.
        \s*$
    "#,
    &description,
);
```

- [ ] **Step 5: Run the focused `codex-tools` and `codex-core` tests to verify the public surface is locked**

Run: `cargo test -p codex-tools agent_tool_tests`
Expected: PASS

Run: `cargo test -p codex-core spawn_agent_description_omits_usage_hint_when_disabled`
Expected: PASS with the new blocking-default wording.

Run: `cargo test -p codex-core spawn_agent_description_uses_configured_usage_hint_text`
Expected: PASS with the new blocking-default wording and custom hint text preserved.

- [ ] **Step 6: Commit the public tool-surface slice**

```bash
git add codex-rs/tools/src/agent_tool.rs codex-rs/tools/src/agent_tool_tests.rs codex-rs/core/src/tools/spec_tests.rs
git commit -m "feat: default spawn_agent to blocking in tool schema"
```

### Task 2: Implement Legacy V1 Blocking Spawn and Boundary-Aligned Wait

**Files:**
- Modify: `codex-rs/core/src/tools/handlers/multi_agents_common.rs`
- Modify: `codex-rs/core/src/tools/handlers/multi_agents/spawn.rs`
- Modify: `codex-rs/core/src/tools/handlers/multi_agents/wait.rs`
- Modify: `codex-rs/core/src/tools/handlers/multi_agents_tests.rs`
- Test: `codex-rs/core/src/tools/handlers/multi_agents_tests.rs`

- [ ] **Step 1: Write the failing V1 regression tests**

```rust
#[tokio::test]
async fn spawn_agent_blocks_until_child_turn_completes() {
    let (mut session, turn) = make_session_and_context().await;
    let manager = thread_manager();
    session.services.agent_control = manager.agent_control();
    let session = Arc::new(session);
    let turn = Arc::new(turn);

    let spawn_task = tokio::spawn({
        let session = session.clone();
        let turn = turn.clone();
        async move {
            SpawnAgentHandler
                .handle(invocation(
                    session,
                    turn,
                    "spawn_agent",
                    function_payload(json!({"message": "inspect this repo"})),
                ))
                .await
        }
    });
    tokio::task::yield_now().await;
    assert!(
        !spawn_task.is_finished(),
        "spawn_agent should still be waiting for the child turn boundary"
    );
}

#[tokio::test]
async fn wait_agent_returns_interrupted_status_without_timing_out() {
    let (mut session, turn) = make_session_and_context().await;
    let manager = thread_manager();
    session.services.agent_control = manager.agent_control();
    let config = turn.config.as_ref().clone();
    let thread = manager.start_thread(config).await.expect("start thread");
    let agent_id = thread.thread_id;

    let child_turn = thread.codex.session.new_default_turn().await;
    thread
        .codex
        .session
        .send_event(
            child_turn.as_ref(),
            EventMsg::TurnAborted(TurnAbortedEvent {
                turn_id: Some(child_turn.sub_id.clone()),
                reason: TurnAbortReason::Interrupted,
                completed_at: None,
                duration_ms: None,
            }),
        )
        .await;

    let output = WaitAgentHandler
        .handle(invocation(
            Arc::new(session),
            Arc::new(turn),
            "wait_agent",
            function_payload(json!({
                "targets": [agent_id.to_string()],
                "timeout_ms": 1000
            })),
        ))
        .await
        .expect("wait_agent should succeed");

    let (content, _) = expect_text_output(output);
    let result: wait::WaitAgentResult =
        serde_json::from_str(&content).expect("wait result should parse");
    assert_eq!(
        result.status.get(&agent_id.to_string()),
        Some(&AgentStatus::Interrupted)
    );
}
```

- [ ] **Step 2: Run the V1 tests to verify they fail before the implementation**

Run: `cargo test -p codex-core spawn_agent_blocks_until_child_turn_completes`
Expected: FAIL because V1 `spawn_agent` still returns immediately.

Run: `cargo test -p codex-core wait_agent_returns_interrupted_status_without_timing_out`
Expected: FAIL because V1 `wait_agent` still treats `Interrupted` as non-final.

- [ ] **Step 3: Implement a shared “child handoff boundary” helper and use it in legacy wait**

```rust
pub(crate) fn is_child_handoff_boundary_status(status: &AgentStatus) -> bool {
    !matches!(status, AgentStatus::PendingInit | AgentStatus::Running)
}

pub(crate) async fn wait_for_agent_handoff_status(
    session: Arc<Session>,
    thread_id: ThreadId,
    mut status_rx: Receiver<AgentStatus>,
) -> Option<(ThreadId, AgentStatus)> {
    let mut status = status_rx.borrow().clone();
    if is_child_handoff_boundary_status(&status) {
        return Some((thread_id, status));
    }

    loop {
        if status_rx.changed().await.is_err() {
            let latest = session.services.agent_control.get_status(thread_id).await;
            return is_child_handoff_boundary_status(&latest).then_some((thread_id, latest));
        }
        status = status_rx.borrow().clone();
        if is_child_handoff_boundary_status(&status) {
            return Some((thread_id, status));
        }
    }
}
```

- [ ] **Step 4: Make V1 `spawn_agent` block by default and return `status`**

```rust
#[derive(Debug, Deserialize)]
struct SpawnAgentArgs {
    message: Option<String>,
    items: Option<Vec<UserInput>>,
    agent_type: Option<String>,
    model: Option<String>,
    reasoning_effort: Option<ReasoningEffort>,
    #[serde(default)]
    fork_context: bool,
    blocking: Option<bool>,
}

#[derive(Debug, Serialize)]
pub(crate) struct SpawnAgentResult {
    agent_id: String,
    nickname: Option<String>,
    status: AgentStatus,
}

let blocking = args.blocking.unwrap_or(true);
let new_thread_id = result?.thread_id;
let status = if blocking {
    let status_rx = session
        .services
        .agent_control
        .subscribe_status(new_thread_id)
        .await
        .map_err(|err| collab_agent_error(new_thread_id, err))?;
    let (_, status) = wait_for_agent_handoff_status(session.clone(), new_thread_id, status_rx)
        .await
        .ok_or_else(|| {
            FunctionCallError::RespondToModel("spawned agent disappeared before status update".to_string())
        })?;
    status
} else {
    session.services.agent_control.get_status(new_thread_id).await
};
```

- [ ] **Step 5: Run the focused V1 handler tests and commit the legacy slice**

Run: `cargo test -p codex-core spawn_agent_blocks_until_child_turn_completes`
Expected: PASS

Run: `cargo test -p codex-core wait_agent_returns_interrupted_status_without_timing_out`
Expected: PASS

Run: `cargo test -p codex-core spawn_agent_returns_agent_id_without_task_name`
Expected: PASS with the new `status` field included in the payload.

```bash
git add codex-rs/core/src/tools/handlers/multi_agents_common.rs codex-rs/core/src/tools/handlers/multi_agents/spawn.rs codex-rs/core/src/tools/handlers/multi_agents/wait.rs codex-rs/core/src/tools/handlers/multi_agents_tests.rs
git commit -m "feat: block legacy spawn_agent on child turn boundaries"
```

### Task 3: Implement V2 Boundary Notifications and Blocking Spawn

**Files:**
- Modify: `codex-rs/core/src/codex.rs`
- Modify: `codex-rs/core/src/tools/handlers/multi_agents_v2/spawn.rs`
- Modify: `codex-rs/core/src/tools/handlers/multi_agents_v2/wait.rs`
- Modify: `codex-rs/core/src/tools/handlers/multi_agents_tests.rs`
- Test: `codex-rs/core/src/tools/handlers/multi_agents_tests.rs`

- [ ] **Step 1: Write the failing V2 regression tests, including the interrupted-turn contract**

```rust
#[tokio::test]
async fn multi_agent_v2_interrupted_turn_notifies_parent() {
    let (mut session, mut turn) = make_session_and_context().await;
    let manager = thread_manager();
    let root = manager
        .start_thread((*turn.config).clone())
        .await
        .expect("root thread should start");
    session.services.agent_control = manager.agent_control();
    session.conversation_id = root.thread_id;
    let mut config = turn.config.as_ref().clone();
    let _ = config.features.enable(Feature::MultiAgentV2);
    turn.config = Arc::new(config);
    let session = Arc::new(session);
    let turn = Arc::new(turn);

    SpawnAgentHandlerV2
        .handle(invocation(
            session.clone(),
            turn.clone(),
            "spawn_agent",
            function_payload(json!({
                "message": "boot worker",
                "task_name": "worker",
                "blocking": false
            })),
        ))
        .await
        .expect("spawn worker");

    let agent_id = session
        .services
        .agent_control
        .resolve_agent_reference(session.conversation_id, &turn.session_source, "worker")
        .await
        .expect("worker should resolve");
    let thread = manager
        .get_thread(agent_id)
        .await
        .expect("worker thread should exist");

    let aborted_turn = thread.codex.session.new_default_turn().await;
    thread
        .codex
        .session
        .send_event(
            aborted_turn.as_ref(),
            EventMsg::TurnAborted(TurnAbortedEvent {
                turn_id: Some(aborted_turn.sub_id.clone()),
                reason: TurnAbortReason::Interrupted,
                completed_at: None,
                duration_ms: None,
            }),
        )
        .await;

    let notifications = manager
        .captured_ops()
        .into_iter()
        .filter_map(|(id, op)| {
            (id == root.thread_id)
                .then_some(op)
                .and_then(|op| match op {
                    Op::InterAgentCommunication { communication }
                        if communication.author.as_str() == "/root/worker"
                            && communication.recipient == AgentPath::root()
                            && communication.other_recipients.is_empty()
                            && !communication.trigger_turn =>
                    {
                        Some(communication.content)
                    }
                    _ => None,
                })
        })
        .collect::<Vec<_>>();

    let expected = format_subagent_notification_message(
        "/root/worker",
        &AgentStatus::Interrupted,
    );
    assert_eq!(notifications, vec![expected]);
}

#[tokio::test]
async fn multi_agent_v2_spawn_blocks_until_matching_child_boundary() {
    let (mut session, mut turn) = make_session_and_context().await;
    let manager = thread_manager();
    let root = manager
        .start_thread((*turn.config).clone())
        .await
        .expect("root thread should start");
    session.services.agent_control = manager.agent_control();
    session.conversation_id = root.thread_id;
    let mut config = (*turn.config).clone();
    config
        .features
        .enable(Feature::MultiAgentV2)
        .expect("test config should allow feature update");
    turn.config = Arc::new(config);

    let session = Arc::new(session);
    let turn = Arc::new(turn);
    let spawn_task = tokio::spawn({
        let session = session.clone();
        let turn = turn.clone();
        async move {
            SpawnAgentHandlerV2
                .handle(invocation(
                    session,
                    turn,
                    "spawn_agent",
                    function_payload(json!({
                        "message": "inspect this repo",
                        "task_name": "worker"
                    })),
                ))
                .await
        }
    });
    tokio::task::yield_now().await;
    assert!(
        !spawn_task.is_finished(),
        "V2 spawn_agent should still be waiting for the child boundary envelope"
    );
}
```

- [ ] **Step 2: Run the V2 tests to verify the current code still fails the new contract**

Run: `cargo test -p codex-core multi_agent_v2_interrupted_turn_notifies_parent`
Expected: FAIL because `codex.rs` currently suppresses interrupted-child parent notifications.

Run: `cargo test -p codex-core multi_agent_v2_spawn_blocks_until_matching_child_boundary`
Expected: FAIL because V2 `spawn_agent` still returns immediately.

- [ ] **Step 3: Update parent notification flow in `codex.rs` to fire on any child turn boundary**

```rust
async fn maybe_notify_parent_of_child_turn_boundary(
    &self,
    turn_context: &TurnContext,
    msg: &EventMsg,
) {
    if !self.enabled(Feature::MultiAgentV2) {
        return;
    }

    if !matches!(msg, EventMsg::TurnComplete(_) | EventMsg::TurnAborted(_)) {
        return;
    }

    let SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
        parent_thread_id,
        agent_path: Some(child_agent_path),
        ..
    }) = &turn_context.session_source
    else {
        return;
    };

    let Some(status) = agent_status_from_event(msg) else {
        return;
    };

    self.forward_child_completion_to_parent(*parent_thread_id, child_agent_path, status)
        .await;
}
```

- [ ] **Step 4: Make V2 `spawn_agent` subscribe before spawn, wait for the matching child envelope, and return `status`**

```rust
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SpawnAgentArgs {
    message: String,
    task_name: String,
    agent_type: Option<String>,
    model: Option<String>,
    reasoning_effort: Option<ReasoningEffort>,
    fork_turns: Option<String>,
    fork_context: Option<bool>,
    blocking: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub(crate) enum SpawnAgentResult {
    WithNickname {
        task_name: String,
        nickname: Option<String>,
        status: AgentStatus,
    },
    HiddenMetadata {
        task_name: String,
        status: AgentStatus,
    },
}

let blocking = args.blocking.unwrap_or(true);
let mut mailbox_seq_rx = blocking.then(|| session.subscribe_mailbox_seq());
let result = session
    .services
    .agent_control
    .spawn_agent_with_metadata(
        config,
        match (spawn_source.get_agent_path(), initial_operation) {
            (Some(recipient), Op::UserInput { items, .. })
                if items.iter().all(|item| matches!(item, UserInput::Text { .. })) =>
            {
                Op::InterAgentCommunication {
                    communication: InterAgentCommunication::new(
                        turn.session_source
                            .get_agent_path()
                            .unwrap_or_else(AgentPath::root),
                        recipient,
                        Vec::new(),
                        prompt.clone(),
                        /*trigger_turn*/ true,
                    ),
                }
            }
            (_, initial_operation) => initial_operation,
        },
        Some(spawn_source),
        SpawnAgentOptions {
            fork_parent_spawn_call_id: fork_mode.as_ref().map(|_| call_id.clone()),
            fork_mode,
        },
    )
    .await
    .map_err(collab_spawn_error);

let task_name = new_agent_path.ok_or_else(|| {
    FunctionCallError::RespondToModel(
        "spawned agent is missing a canonical task name".to_string(),
    )
})?;

let status = if blocking {
    wait::wait_for_child_boundary_notification(
        session.clone(),
        mailbox_seq_rx.as_mut().expect("blocking receiver"),
        task_name.as_str(),
    )
    .await?
} else {
    session
        .services
        .agent_control
        .get_status(result?.thread_id)
        .await
};
```

- [ ] **Step 5: Add the mailbox-matching helper and prove unrelated mail does not unblock the wrong spawn**

```rust
pub(crate) async fn wait_for_child_boundary_notification(
    session: Arc<Session>,
    mailbox_seq_rx: &mut tokio::sync::watch::Receiver<u64>,
    child_agent_path: &str,
) -> Result<AgentStatus, FunctionCallError> {
    fn response_item_as_inter_agent_communication(
        item: &ResponseInputItem,
    ) -> Option<InterAgentCommunication> {
        match item {
            ResponseInputItem::Message { content, .. } => {
                InterAgentCommunication::from_message_content(content)
            }
            _ => None,
        }
    }

    fn parse_boundary_status_from_notification(content: &str) -> Result<Option<AgentStatus>, FunctionCallError> {
        let inner = content
            .strip_prefix(SUBAGENT_NOTIFICATION_OPEN_TAG)
            .and_then(|content| content.strip_suffix(SUBAGENT_NOTIFICATION_CLOSE_TAG));
        let Some(inner) = inner else {
            return Ok(None);
        };

        let payload: serde_json::Value = serde_json::from_str(inner).map_err(|err| {
            FunctionCallError::RespondToModel(
                format!("invalid subagent notification payload: {err}"),
            )
        })?;
        let status = serde_json::from_value::<AgentStatus>(payload["status"].clone()).map_err(
            |err| {
                FunctionCallError::RespondToModel(
                    format!("invalid subagent notification status: {err}"),
                )
            },
        )?;
        Ok(Some(status))
    }

    loop {
        let pending = session.get_pending_input().await;
        for item in pending {
            if let Some(communication) = response_item_as_inter_agent_communication(&item) {
                if communication.author.as_str() != child_agent_path {
                    continue;
                }
                if let Some(status) = parse_boundary_status_from_notification(&communication.content)? {
                    return Ok(status);
                }
            }
        }

        mailbox_seq_rx
            .changed()
            .await
            .map_err(|_| FunctionCallError::RespondToModel("mailbox closed while waiting for child boundary".to_string()))?;
    }
}
```

- [ ] **Step 6: Run the focused V2 handler tests and commit the V2 slice**

Run: `cargo test -p codex-core multi_agent_v2_interrupted_turn_notifies_parent`
Expected: PASS

Run: `cargo test -p codex-core multi_agent_v2_spawn_blocks_until_matching_child_boundary`
Expected: PASS

Run: `cargo test -p codex-core multi_agent_v2_wait_agent_returns_summary_for_mailbox_activity`
Expected: PASS with interrupted-child notifications included in the same mailbox model.

```bash
git add codex-rs/core/src/codex.rs codex-rs/core/src/tools/handlers/multi_agents_v2/spawn.rs codex-rs/core/src/tools/handlers/multi_agents_v2/wait.rs codex-rs/core/src/tools/handlers/multi_agents_tests.rs
git commit -m "feat: block multi-agent v2 spawn on child turn boundaries"
```

### Task 4: Final Verification, Lint/Fmt, and Integration Handoff

**Files:**
- Modify: `codex-rs/tools/src/agent_tool.rs`
- Modify: `codex-rs/tools/src/agent_tool_tests.rs`
- Modify: `codex-rs/core/src/tools/spec_tests.rs`
- Modify: `codex-rs/core/src/tools/handlers/multi_agents_common.rs`
- Modify: `codex-rs/core/src/tools/handlers/multi_agents/spawn.rs`
- Modify: `codex-rs/core/src/tools/handlers/multi_agents/wait.rs`
- Modify: `codex-rs/core/src/codex.rs`
- Modify: `codex-rs/core/src/tools/handlers/multi_agents_v2/spawn.rs`
- Modify: `codex-rs/core/src/tools/handlers/multi_agents_v2/wait.rs`
- Modify: `codex-rs/core/src/tools/handlers/multi_agents_tests.rs`

- [ ] **Step 1: Run the targeted crate tests that prove the feature works end-to-end**

Run: `cargo test -p codex-tools agent_tool_tests`
Expected: PASS

Run: `cargo test -p codex-core spawn_agent_`
Expected: PASS for the updated spawn-agent handler coverage.

Run: `cargo test -p codex-core wait_agent_`
Expected: PASS for the updated wait semantics.

Run: `cargo test -p codex-core multi_agent_v2_`
Expected: PASS for the updated V2 notification and blocking behavior.

- [ ] **Step 2: Run crate-scoped lint fixes, then format**

Run: `just fix -p codex-tools`
Expected: PASS with no remaining clippy violations in the tools crate.

Run: `just fix -p codex-core`
Expected: PASS with no remaining clippy violations in the core crate.

Run: `just fmt`
Expected: PASS and leave only intentional formatting changes.

- [ ] **Step 3: Ask the user before the full workspace suite, because this repo requires approval for `cargo test` across the whole workspace**

Prompt to user:

```text
The targeted crate tests passed. The repo instructions require asking before running the full workspace suite because this change touches codex-core. Do you want me to run `cargo test` from `codex-rs` now?
```

- [ ] **Step 4: If the user approves, run the full suite from `codex-rs`**

Run: `cargo test`
Expected: PASS across the workspace.

- [ ] **Step 5: Commit the final integrated change**

```bash
git add codex-rs/tools/src/agent_tool.rs codex-rs/tools/src/agent_tool_tests.rs codex-rs/core/src/tools/spec_tests.rs codex-rs/core/src/tools/handlers/multi_agents_common.rs codex-rs/core/src/tools/handlers/multi_agents/spawn.rs codex-rs/core/src/tools/handlers/multi_agents/wait.rs codex-rs/core/src/codex.rs codex-rs/core/src/tools/handlers/multi_agents_v2/spawn.rs codex-rs/core/src/tools/handlers/multi_agents_v2/wait.rs codex-rs/core/src/tools/handlers/multi_agents_tests.rs
git commit -m "feat: make spawn_agent blocking by default"
```

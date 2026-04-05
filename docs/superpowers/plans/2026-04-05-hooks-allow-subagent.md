# Hooks allowSubagent Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let any `hooks.json` handler opt out of subagent sessions with `allowSubagent: false` while preserving current behavior when the field is omitted.

**Architecture:** Keep `allowSubagent` as a per-handler execution policy in `codex-hooks`, not a hook-payload change. Parse and store the flag in discovered handlers, propagate `is_subagent` through the internal hook request types, and filter handlers centrally in the dispatcher so every hook event (`SessionStart`, `PreToolUse`, `PostToolUse`, `UserPromptSubmit`, `Stop`) gets the same behavior.

**Tech Stack:** Rust, `codex-hooks`, `codex-core`, `serde`, `wiremock`, `cargo test`, `just fmt`, `just fix`

---

### Task 1: Parse and Preserve `allowSubagent` in Hook Discovery

**Files:**
- Modify: `codex-rs/hooks/src/engine/config.rs`
- Modify: `codex-rs/hooks/src/engine/mod.rs`
- Modify: `codex-rs/hooks/src/engine/discovery.rs`
- Test: `codex-rs/hooks/src/engine/discovery.rs`

- [ ] **Step 1: Write the failing discovery tests**

```rust
#[test]
fn discovery_defaults_allow_subagent_to_true() {
    let parsed: HooksFile = serde_json::from_value(serde_json::json!({
        "hooks": {
            "SessionStart": [{
                "hooks": [{
                    "type": "command",
                    "command": "echo hello"
                }]
            }]
        }
    }))
    .expect("parse hooks file");

    let HookHandlerConfig::Command { allow_subagent, .. } =
        parsed.hooks.session_start[0].hooks[0].clone()
    else {
        panic!("expected command hook");
    };

    assert_eq!(allow_subagent, true);
}

#[test]
fn discovery_preserves_allow_subagent_false() {
    let mut handlers = Vec::new();
    let mut warnings = Vec::new();
    let mut display_order = 0;

    append_group_handlers(
        &mut handlers,
        &mut warnings,
        &mut display_order,
        Path::new("/tmp/hooks.json"),
        HookEventName::Stop,
        /*matcher*/ None,
        vec![HookHandlerConfig::Command {
            command: "echo hello".to_string(),
            timeout_sec: None,
            r#async: false,
            status_message: None,
            allow_subagent: false,
        }],
    );

    assert_eq!(warnings, Vec::<String>::new());
    assert_eq!(handlers.len(), 1);
    assert_eq!(handlers[0].allow_subagent, false);
}
```

- [ ] **Step 2: Run the targeted hooks discovery tests to verify they fail**

Run: `cargo test -p codex-hooks discovery_defaults_allow_subagent_to_true`

Run: `cargo test -p codex-hooks discovery_preserves_allow_subagent_false`

Expected: FAIL because `HookHandlerConfig::Command` and `ConfiguredHandler` do not yet carry `allow_subagent`.

- [ ] **Step 3: Add `allowSubagent` to the hook config structs**

```rust
fn default_allow_subagent() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub(crate) enum HookHandlerConfig {
    #[serde(rename = "command")]
    Command {
        command: String,
        #[serde(default, rename = "timeout", alias = "timeoutSec")]
        timeout_sec: Option<u64>,
        #[serde(default)]
        r#async: bool,
        #[serde(default, rename = "statusMessage")]
        status_message: Option<String>,
        #[serde(default = "default_allow_subagent", rename = "allowSubagent")]
        allow_subagent: bool,
    },
    #[serde(rename = "prompt")]
    Prompt {
        #[serde(default = "default_allow_subagent", rename = "allowSubagent")]
        allow_subagent: bool,
    },
    #[serde(rename = "agent")]
    Agent {
        #[serde(default = "default_allow_subagent", rename = "allowSubagent")]
        allow_subagent: bool,
    },
}
```

- [ ] **Step 4: Carry the resolved flag into discovered handlers**

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConfiguredHandler {
    pub event_name: codex_protocol::protocol::HookEventName,
    pub matcher: Option<String>,
    pub command: String,
    pub timeout_sec: u64,
    pub allow_subagent: bool,
    pub status_message: Option<String>,
    pub source_path: PathBuf,
    pub display_order: i64,
}
```

```rust
HookHandlerConfig::Command {
    command,
    timeout_sec,
    r#async,
    status_message,
    allow_subagent,
} => {
    // existing async/empty-command checks unchanged
    handlers.push(ConfiguredHandler {
        event_name,
        matcher: matcher.map(ToOwned::to_owned),
        command,
        timeout_sec,
        allow_subagent,
        status_message,
        source_path: source_path.to_path_buf(),
        display_order: *display_order,
    });
}
```

- [ ] **Step 5: Re-run the targeted discovery tests**

Run: `cargo test -p codex-hooks discovery_defaults_allow_subagent_to_true`

Run: `cargo test -p codex-hooks discovery_preserves_allow_subagent_false`

Expected: PASS

- [ ] **Step 6: Commit the discovery/config slice**

```bash
git add codex-rs/hooks/src/engine/config.rs codex-rs/hooks/src/engine/mod.rs codex-rs/hooks/src/engine/discovery.rs
git commit -m "feat: parse allowSubagent for hooks"
```

### Task 2: Make Hook Selection Subagent-Aware Across All Events

**Files:**
- Modify: `codex-rs/hooks/src/engine/dispatcher.rs`
- Modify: `codex-rs/hooks/src/events/session_start.rs`
- Modify: `codex-rs/hooks/src/events/pre_tool_use.rs`
- Modify: `codex-rs/hooks/src/events/post_tool_use.rs`
- Modify: `codex-rs/hooks/src/events/user_prompt_submit.rs`
- Modify: `codex-rs/hooks/src/events/stop.rs`
- Test: `codex-rs/hooks/src/engine/dispatcher.rs`

- [ ] **Step 1: Write failing dispatcher coverage for one matcher-driven event and one matcher-free event**

```rust
#[test]
fn session_start_skips_handlers_that_disallow_subagents() {
    let handlers = vec![
        make_handler(
            HookEventName::SessionStart,
            Some("^startup$"),
            "echo allowed",
            /*allow_subagent*/ true,
            /*display_order*/ 0,
        ),
        make_handler(
            HookEventName::SessionStart,
            Some("^startup$"),
            "echo blocked",
            /*allow_subagent*/ false,
            /*display_order*/ 1,
        ),
    ];

    let selected = select_handlers(
        &handlers,
        HookEventName::SessionStart,
        Some("startup"),
        /*is_subagent*/ true,
    );

    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].command, "echo allowed");
}

#[test]
fn stop_keeps_handlers_for_primary_even_when_allow_subagent_is_false() {
    let handlers = vec![make_handler(
        HookEventName::Stop,
        /*matcher*/ None,
        "echo stop",
        /*allow_subagent*/ false,
        /*display_order*/ 0,
    )];

    let selected = select_handlers(
        &handlers,
        HookEventName::Stop,
        /*matcher_input*/ None,
        /*is_subagent*/ false,
    );

    assert_eq!(selected.len(), 1);
}
```

- [ ] **Step 2: Run the dispatcher tests to verify they fail**

Run: `cargo test -p codex-hooks session_start_skips_handlers_that_disallow_subagents`

Run: `cargo test -p codex-hooks stop_keeps_handlers_for_primary_even_when_allow_subagent_is_false`

Expected: FAIL because `select_handlers` and the test helper do not yet accept `is_subagent`.

- [ ] **Step 3: Add `is_subagent` to every internal hook request type**

```rust
pub struct SessionStartRequest {
    pub session_id: ThreadId,
    pub cwd: PathBuf,
    pub transcript_path: Option<PathBuf>,
    pub model: String,
    pub permission_mode: String,
    pub source: SessionStartSource,
    pub is_subagent: bool,
}

pub struct PreToolUseRequest {
    pub session_id: ThreadId,
    pub turn_id: String,
    pub cwd: PathBuf,
    pub transcript_path: Option<PathBuf>,
    pub model: String,
    pub permission_mode: String,
    pub tool_name: String,
    pub tool_use_id: String,
    pub command: String,
    pub is_subagent: bool,
}
```

Apply the same field to `PostToolUseRequest`, `UserPromptSubmitRequest`, and `StopRequest`.

- [ ] **Step 4: Filter handlers centrally in the dispatcher**

```rust
pub(crate) fn select_handlers(
    handlers: &[ConfiguredHandler],
    event_name: HookEventName,
    matcher_input: Option<&str>,
    is_subagent: bool,
) -> Vec<ConfiguredHandler> {
    handlers
        .iter()
        .filter(|handler| handler.event_name == event_name)
        .filter(|handler| !is_subagent || handler.allow_subagent)
        .filter(|handler| match event_name {
            HookEventName::PreToolUse
            | HookEventName::PostToolUse
            | HookEventName::SessionStart => {
                matches_matcher(handler.matcher.as_deref(), matcher_input)
            }
            HookEventName::UserPromptSubmit | HookEventName::Stop => true,
        })
        .cloned()
        .collect()
}
```

- [ ] **Step 5: Update every hook preview/run entry point to pass the new flag**

```rust
dispatcher::select_handlers(
    handlers,
    HookEventName::SessionStart,
    Some(request.source.as_str()),
    request.is_subagent,
)
```

Use the same `request.is_subagent` pass-through in `pre_tool_use`, `post_tool_use`,
`user_prompt_submit`, and `stop`.

- [ ] **Step 6: Re-run the targeted dispatcher tests**

Run: `cargo test -p codex-hooks session_start_skips_handlers_that_disallow_subagents`

Run: `cargo test -p codex-hooks stop_keeps_handlers_for_primary_even_when_allow_subagent_is_false`

Expected: PASS

- [ ] **Step 7: Commit the selection/runtime slice in `codex-hooks`**

```bash
git add codex-rs/hooks/src/engine/dispatcher.rs codex-rs/hooks/src/events/session_start.rs codex-rs/hooks/src/events/pre_tool_use.rs codex-rs/hooks/src/events/post_tool_use.rs codex-rs/hooks/src/events/user_prompt_submit.rs codex-rs/hooks/src/events/stop.rs
git commit -m "feat: skip subagent hooks when disallowed"
```

### Task 3: Propagate Subagent State from `codex-core` into Hook Requests

**Files:**
- Modify: `codex-rs/core/src/hook_runtime.rs`
- Modify: `codex-rs/core/src/codex.rs`
- Test: `codex-rs/core/tests/suite/hooks.rs`

- [ ] **Step 1: Add a local helper for deriving subagent state from the turn context**

```rust
fn hook_is_subagent(turn_context: &TurnContext) -> bool {
    matches!(
        turn_context.session_source,
        codex_protocol::protocol::SessionSource::SubAgent(_)
    )
}
```

- [ ] **Step 2: Set `is_subagent` on the hook requests built in `hook_runtime.rs`**

```rust
let request = codex_hooks::SessionStartRequest {
    session_id: sess.conversation_id,
    cwd: turn_context.cwd.to_path_buf(),
    transcript_path: sess.hook_transcript_path().await,
    model: turn_context.model_info.slug.clone(),
    permission_mode: hook_permission_mode(turn_context),
    source: session_start_source,
    is_subagent: hook_is_subagent(turn_context),
};
```

Apply the same `is_subagent: hook_is_subagent(turn_context)` field when building:

- `PreToolUseRequest`
- `PostToolUseRequest`
- `UserPromptSubmitRequest`

- [ ] **Step 3: Set `is_subagent` on `StopRequest` in `codex.rs`**

```rust
let stop_request = codex_hooks::StopRequest {
    session_id: sess.conversation_id,
    turn_id: turn_context.sub_id.clone(),
    cwd: turn_context.cwd.to_path_buf(),
    transcript_path: sess.hook_transcript_path().await,
    model: turn_context.model_info.slug.clone(),
    permission_mode: hook_permission_mode(&turn_context),
    stop_hook_active,
    last_assistant_message: last_agent_message.clone(),
    is_subagent: matches!(
        turn_context.session_source,
        codex_protocol::protocol::SessionSource::SubAgent(_)
    ),
};
```

- [ ] **Step 4: Run an existing core hook regression to verify the wiring still compiles and behaves**

Run: `cargo test -p codex-core --test suite session_start_hook_sees_materialized_transcript_path`

Expected: PASS

- [ ] **Step 5: Commit the `codex-core` propagation slice**

```bash
git add codex-rs/core/src/hook_runtime.rs codex-rs/core/src/codex.rs
git commit -m "feat: propagate subagent state into hook requests"
```

### Task 4: Add an End-to-End Regression for Spawned Subagents

**Files:**
- Modify: `codex-rs/core/tests/suite/hooks.rs`
- Test: `codex-rs/core/tests/suite/hooks.rs`

Keep this regression inside the existing Unix-only `hooks.rs` suite. Do not widen
`codex-rs/core/tests/suite/mod.rs` scope as part of this feature.

- [ ] **Step 1: Extend the session-start test fixture helper to control `allowSubagent`**

```rust
fn write_session_start_hook_recording_transcript(
    home: &Path,
    allow_subagent: bool,
) -> Result<()> {
    let script_path = home.join("session_start_hook.py");
    let log_path = home.join("session_start_hook_log.jsonl");
    let hooks = serde_json::json!({
        "hooks": {
            "SessionStart": [{
                "hooks": [{
                    "type": "command",
                    "command": format!("python3 {}", script_path.display()),
                    "statusMessage": "running session start hook",
                    "allowSubagent": allow_subagent,
                }]
            }]
        }
    });

    fs::write(&script_path, script).context("write session start hook script")?;
    fs::write(home.join("hooks.json"), hooks.to_string()).context("write hooks.json")?;
    Ok(())
}
```

Update the existing `session_start_hook_sees_materialized_transcript_path` test to call
`write_session_start_hook_recording_transcript(home, /*allow_subagent*/ true)`.

- [ ] **Step 2: Add local helpers for spawned-thread observation if `hooks.rs` does not already have them**

```rust
fn body_contains(req: &wiremock::Request, text: &str) -> bool {
    String::from_utf8(req.body.clone())
        .ok()
        .is_some_and(|body| body.contains(text))
}

async fn wait_for_spawned_thread_id(test: &TestCodex) -> Result<String> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        let ids = test.thread_manager.list_thread_ids().await;
        if let Some(spawned_id) = ids
            .iter()
            .find(|id| **id != test.session_configured.session_id)
        {
            return Ok(spawned_id.to_string());
        }
        if tokio::time::Instant::now() >= deadline {
            anyhow::bail!("timed out waiting for spawned thread id");
        }
        sleep(Duration::from_millis(10)).await;
    }
}
```

- [ ] **Step 3: Write the failing end-to-end regression**

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_start_hook_allow_subagent_false_skips_spawned_subagent() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let spawn_args = serde_json::to_string(&serde_json::json!({
        "message": "child: do work",
    }))?;

    mount_sse_once_match(
        &server,
        |req: &wiremock::Request| body_contains(req, "spawn a child and continue"),
        sse(vec![
            ev_response_created("resp-parent-1"),
            ev_function_call("spawn-call-1", "spawn_agent", &spawn_args),
            ev_completed("resp-parent-1"),
        ]),
    )
    .await;

    mount_sse_once_match(
        &server,
        |req: &wiremock::Request| body_contains(req, "child: do work"),
        sse(vec![
            ev_response_created("resp-child-1"),
            ev_assistant_message("msg-child-1", "child done"),
            ev_completed("resp-child-1"),
        ]),
    )
    .await;

    mount_sse_once_match(
        &server,
        |req: &wiremock::Request| body_contains(req, "spawn-call-1"),
        sse(vec![
            ev_response_created("resp-parent-2"),
            ev_assistant_message("msg-parent-2", "parent done"),
            ev_completed("resp-parent-2"),
        ]),
    )
    .await;

    let test = test_codex()
        .with_pre_build_hook(|home| {
            write_session_start_hook_recording_transcript(home, /*allow_subagent*/ false)
                .expect("write session start hook fixture");
        })
        .with_config(|config| {
            config.features.enable(Feature::CodexHooks).expect("enable hooks");
            config.features.enable(Feature::Collab).expect("enable collab");
        })
        .build(&server)
        .await?;

    test.submit_turn("spawn a child and continue").await?;
    let _spawned_id = wait_for_spawned_thread_id(&test).await?;

    let hook_inputs = read_session_start_hook_inputs(test.codex_home_path())?;
    assert_eq!(hook_inputs.len(), 1);

    Ok(())
}
```

- [ ] **Step 4: Run the new regression to verify it fails before the implementation is complete**

Run: `cargo test -p codex-core --test suite session_start_hook_allow_subagent_false_skips_spawned_subagent`

Expected: FAIL because both the primary session and the spawned subagent currently run the same
`SessionStart` hook.

- [ ] **Step 5: Run the new regression after Tasks 1-3 are in place**

Run: `cargo test -p codex-core --test suite session_start_hook_allow_subagent_false_skips_spawned_subagent`

Expected: PASS with exactly one logged `SessionStart` hook invocation.

- [ ] **Step 6: Commit the end-to-end regression**

```bash
git add codex-rs/core/tests/suite/hooks.rs
git commit -m "test: cover allowSubagent for spawned hooks"
```

### Task 5: Verification and Final Cleanup

**Files:**
- Modify: `codex-rs/hooks/src/engine/config.rs`
- Modify: `codex-rs/hooks/src/engine/mod.rs`
- Modify: `codex-rs/hooks/src/engine/discovery.rs`
- Modify: `codex-rs/hooks/src/engine/dispatcher.rs`
- Modify: `codex-rs/hooks/src/events/session_start.rs`
- Modify: `codex-rs/hooks/src/events/pre_tool_use.rs`
- Modify: `codex-rs/hooks/src/events/post_tool_use.rs`
- Modify: `codex-rs/hooks/src/events/user_prompt_submit.rs`
- Modify: `codex-rs/hooks/src/events/stop.rs`
- Modify: `codex-rs/core/src/hook_runtime.rs`
- Modify: `codex-rs/core/src/codex.rs`
- Modify: `codex-rs/core/tests/suite/hooks.rs`

- [ ] **Step 1: Run the hooks crate test suite**

Run: `cargo test -p codex-hooks`

Expected: PASS

- [ ] **Step 2: Run the targeted core hook regressions**

Run: `cargo test -p codex-core --test suite session_start_hook_sees_materialized_transcript_path`

Run: `cargo test -p codex-core --test suite session_start_hook_allow_subagent_false_skips_spawned_subagent`

Expected: PASS

- [ ] **Step 3: Run the crate-scoped Clippy autofixes**

Run: `just fix -p codex-hooks`

Expected: Clippy fixes apply cleanly or report no changes needed

Run: `just fix -p codex-core`

Expected: Clippy fixes apply cleanly or report no changes needed

- [ ] **Step 4: Format the workspace Rust code**

Run: `just fmt`

Expected: `rustfmt` completes without further diffs

- [ ] **Step 5: Confirm no hook schema regeneration is needed**

```text
No `codex-rs/hooks/src/schema.rs` command input/output shapes changed, so do not run
`write_hooks_schema_fixtures` for this feature.
```

- [ ] **Step 6: Create the final implementation commit**

```bash
git add codex-rs/hooks/src/engine/config.rs codex-rs/hooks/src/engine/mod.rs codex-rs/hooks/src/engine/discovery.rs codex-rs/hooks/src/engine/dispatcher.rs codex-rs/hooks/src/events/session_start.rs codex-rs/hooks/src/events/pre_tool_use.rs codex-rs/hooks/src/events/post_tool_use.rs codex-rs/hooks/src/events/user_prompt_submit.rs codex-rs/hooks/src/events/stop.rs codex-rs/core/src/hook_runtime.rs codex-rs/core/src/codex.rs codex-rs/core/tests/suite/hooks.rs
git commit -m "feat: add allowSubagent hook control"
```

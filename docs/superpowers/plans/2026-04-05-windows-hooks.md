# Windows Hooks Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable `hooks.json` lifecycle hooks on Windows without changing the existing executor behavior.

**Architecture:** Keep hook discovery and execution identical across platforms by removing the Windows-only disable guard in `codex-hooks`. Validate the change with a regression test that loads a temporary `hooks.json` through `ConfigLayerStack` and confirms the engine exposes a `SessionStart` handler on Windows.

**Tech Stack:** Rust, `codex-hooks`, `codex-config`, `tempfile`, `cargo test`

---

### Task 1: Add a Regression Test for Windows Hook Discovery

**Files:**
- Modify: `codex-rs/hooks/src/engine/mod.rs`
- Test: `codex-rs/hooks/src/engine/mod.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn enabled_engine_discovers_hooks_from_config_layers() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    std::fs::write(
        temp_dir.path().join("hooks.json"),
        serde_json::json!({
            "hooks": {
                "SessionStart": [{
                    "hooks": [{
                        "type": "command",
                        "command": "echo hook",
                        "statusMessage": "warming shell",
                    }]
                }]
            }
        })
        .to_string(),
    )
    .expect("write hooks.json");

    let config_toml = codex_utils_absolute_path::AbsolutePathBuf::from_absolute_path(
        temp_dir.path().join("config.toml"),
    )
    .expect("absolute config path");
    let config_layer_stack = codex_config::ConfigLayerStack::default().with_user_config(
        &config_toml,
        toml::Value::Table(toml::map::Map::new()),
    );

    let engine = ClaudeHooksEngine::new(
        /*enabled*/ true,
        Some(&config_layer_stack),
        CommandShell {
            program: String::new(),
            args: Vec::new(),
        },
    );

    assert!(engine.warnings().is_empty());
    assert_eq!(
        engine
            .preview_session_start(&crate::events::session_start::SessionStartRequest {
                session_id: codex_protocol::ThreadId::new(),
                cwd: temp_dir.path().to_path_buf(),
                transcript_path: None,
                model: "gpt-5".to_string(),
                permission_mode: "default".to_string(),
                source: crate::events::session_start::SessionStartSource::Startup,
            })
            .len(),
        1,
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p codex-hooks enabled_engine_discovers_hooks_from_config_layers`
Expected on current Windows behavior: FAIL because the engine still emits the startup warning and discovers zero handlers.

- [ ] **Step 3: Write minimal implementation**

```rust
let _ = schema_loader::generated_hook_schemas();
let discovered = discovery::discover_handlers(config_layer_stack);
Self {
    handlers: discovered.handlers,
    warnings: discovered.warnings,
    shell,
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p codex-hooks enabled_engine_discovers_hooks_from_config_layers`
Expected: PASS

### Task 2: Remove the Windows-Only Disable Guard

**Files:**
- Modify: `codex-rs/hooks/src/engine/mod.rs`
- Test: `codex-rs/hooks/src/engine/mod.rs`

- [ ] **Step 1: Remove the early return**

```rust
if !enabled {
    return Self {
        handlers: Vec::new(),
        warnings: Vec::new(),
        shell,
    };
}
```

- [ ] **Step 2: Keep discovery unchanged for all platforms**

```rust
let _ = schema_loader::generated_hook_schemas();
let discovered = discovery::discover_handlers(config_layer_stack);
Self {
    handlers: discovered.handlers,
    warnings: discovered.warnings,
    shell,
}
```

- [ ] **Step 3: Run the targeted crate tests**

Run: `cargo test -p codex-hooks`
Expected: PASS

- [ ] **Step 4: Format the crate**

Run: `just fmt`
Expected: `rustfmt` completes without changes left to apply

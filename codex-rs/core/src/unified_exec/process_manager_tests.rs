use super::*;
use crate::background_process_completion::CompletionBehavior;
use crate::codex::make_session_and_context;
use crate::exec::ExecCapturePolicy;
use crate::exec::ExecExpiration;
use crate::unified_exec::NoopSpawnLifecycle;
use crate::unified_exec::UnifiedExecContext;
use crate::unified_exec::async_watcher::start_streaming_output;
use crate::unified_exec::head_tail_buffer::HeadTailBuffer;
use codex_sandboxing::SandboxType;
use core_test_support::skip_if_sandbox;
use pretty_assertions::assert_eq;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::Duration;
use tokio::time::Instant;

#[test]
fn unified_exec_env_injects_defaults() {
    let env = apply_unified_exec_env(HashMap::new());
    let expected = HashMap::from([
        ("NO_COLOR".to_string(), "1".to_string()),
        ("TERM".to_string(), "dumb".to_string()),
        ("LANG".to_string(), "C.UTF-8".to_string()),
        ("LC_CTYPE".to_string(), "C.UTF-8".to_string()),
        ("LC_ALL".to_string(), "C.UTF-8".to_string()),
        ("COLORTERM".to_string(), String::new()),
        ("PAGER".to_string(), "cat".to_string()),
        ("GIT_PAGER".to_string(), "cat".to_string()),
        ("GH_PAGER".to_string(), "cat".to_string()),
        ("CODEX_CI".to_string(), "1".to_string()),
    ]);

    assert_eq!(env, expected);
}

#[test]
fn unified_exec_env_overrides_existing_values() {
    let mut base = HashMap::new();
    base.insert("NO_COLOR".to_string(), "0".to_string());
    base.insert("PATH".to_string(), "/usr/bin".to_string());

    let env = apply_unified_exec_env(base);

    assert_eq!(env.get("NO_COLOR"), Some(&"1".to_string()));
    assert_eq!(env.get("PATH"), Some(&"/usr/bin".to_string()));
}

#[test]
fn exec_server_process_id_matches_unified_exec_process_id() {
    assert_eq!(exec_server_process_id(/*process_id*/ 4321), "4321");
}

#[test]
fn pruning_prefers_exited_processes_outside_recently_used() {
    let now = Instant::now();
    let meta = vec![
        (1, now - Duration::from_secs(40), false),
        (2, now - Duration::from_secs(30), true),
        (3, now - Duration::from_secs(20), false),
        (4, now - Duration::from_secs(19), false),
        (5, now - Duration::from_secs(18), false),
        (6, now - Duration::from_secs(17), false),
        (7, now - Duration::from_secs(16), false),
        (8, now - Duration::from_secs(15), false),
        (9, now - Duration::from_secs(14), false),
        (10, now - Duration::from_secs(13), false),
    ];

    let candidate = UnifiedExecProcessManager::process_id_to_prune_from_meta(&meta);

    assert_eq!(candidate, Some(2));
}

#[test]
fn pruning_falls_back_to_lru_when_no_exited() {
    let now = Instant::now();
    let meta = vec![
        (1, now - Duration::from_secs(40), false),
        (2, now - Duration::from_secs(30), false),
        (3, now - Duration::from_secs(20), false),
        (4, now - Duration::from_secs(19), false),
        (5, now - Duration::from_secs(18), false),
        (6, now - Duration::from_secs(17), false),
        (7, now - Duration::from_secs(16), false),
        (8, now - Duration::from_secs(15), false),
        (9, now - Duration::from_secs(14), false),
        (10, now - Duration::from_secs(13), false),
    ];

    let candidate = UnifiedExecProcessManager::process_id_to_prune_from_meta(&meta);

    assert_eq!(candidate, Some(1));
}

#[test]
fn pruning_protects_recent_processes_even_if_exited() {
    let now = Instant::now();
    let meta = vec![
        (1, now - Duration::from_secs(40), false),
        (2, now - Duration::from_secs(30), false),
        (3, now - Duration::from_secs(20), true),
        (4, now - Duration::from_secs(19), false),
        (5, now - Duration::from_secs(18), false),
        (6, now - Duration::from_secs(17), false),
        (7, now - Duration::from_secs(16), false),
        (8, now - Duration::from_secs(15), false),
        (9, now - Duration::from_secs(14), false),
        (10, now - Duration::from_secs(13), true),
    ];

    let candidate = UnifiedExecProcessManager::process_id_to_prune_from_meta(&meta);

    // (10) is exited but among the last 8; we should drop the LRU outside that set.
    assert_eq!(candidate, Some(1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn store_process_persists_completion_behavior_in_entry() -> anyhow::Result<()> {
    skip_if_sandbox!(Ok(()));

    let (session, turn) = make_session_and_context().await;
    let session = Arc::new(session);
    let turn = Arc::new(turn);
    let manager = &session.services.unified_exec_manager;
    let process_id = manager.allocate_process_id().await;
    let cwd = turn.cwd.clone().to_path_buf();
    let command = vec![
        "bash".to_string(),
        "-lc".to_string(),
        "printf persisted".to_string(),
    ];
    let request = ExecRequest::new(
        command.clone(),
        cwd.clone(),
        std::env::vars().collect::<HashMap<String, String>>(),
        /*network*/ None,
        ExecExpiration::DefaultTimeout,
        ExecCapturePolicy::ShellTool,
        SandboxType::None,
        turn.windows_sandbox_level,
        /*windows_sandbox_private_desktop*/ false,
        turn.sandbox_policy.get().clone(),
        turn.file_system_sandbox_policy.clone(),
        turn.network_sandbox_policy,
        /*arg0*/ None,
    );
    let process = Arc::new(
        manager
            .open_session_with_exec_env(
                process_id,
                &request,
                /*tty*/ false,
                Box::new(NoopSpawnLifecycle),
                turn.environment.as_ref().expect("turn environment"),
            )
            .await?,
    );
    let context =
        UnifiedExecContext::new(Arc::clone(&session), Arc::clone(&turn), "call".to_string());

    manager
        .store_process(
            Arc::clone(&process),
            &context,
            "printf persisted",
            &command,
            cwd,
            Instant::now(),
            process_id,
            CompletionBehavior::Wake,
            /*tty*/ false,
            /*network_approval_id*/ None,
            Arc::new(tokio::sync::Mutex::new(HeadTailBuffer::default())),
        )
        .await;

    let store = manager.process_store.lock().await;
    let entry = store
        .processes
        .get(&process_id)
        .expect("expected stored process entry");
    assert_eq!(entry.completion_behavior, CompletionBehavior::Wake);
    drop(store);

    manager.release_process_id(process_id).await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn backfill_late_completion_after_arming_queues_already_exited_process() -> anyhow::Result<()>
{
    skip_if_sandbox!(Ok(()));

    let (session, turn) = make_session_and_context().await;
    let session = Arc::new(session);
    let turn = Arc::new(turn);
    let manager = &session.services.unified_exec_manager;
    let process_id = manager.allocate_process_id().await;
    let cwd = turn.cwd.clone().to_path_buf();
    let command = vec![
        "bash".to_string(),
        "-lc".to_string(),
        "printf raced".to_string(),
    ];
    let request = ExecRequest::new(
        command.clone(),
        cwd.clone(),
        std::env::vars().collect::<HashMap<String, String>>(),
        /*network*/ None,
        ExecExpiration::DefaultTimeout,
        ExecCapturePolicy::ShellTool,
        SandboxType::None,
        turn.windows_sandbox_level,
        /*windows_sandbox_private_desktop*/ false,
        turn.sandbox_policy.get().clone(),
        turn.file_system_sandbox_policy.clone(),
        turn.network_sandbox_policy,
        /*arg0*/ None,
    );
    let process = Arc::new(
        manager
            .open_session_with_exec_env(
                process_id,
                &request,
                /*tty*/ false,
                Box::new(NoopSpawnLifecycle),
                turn.environment.as_ref().expect("turn environment"),
            )
            .await?,
    );
    let context =
        UnifiedExecContext::new(Arc::clone(&session), Arc::clone(&turn), "call".to_string());
    let transcript = Arc::new(tokio::sync::Mutex::new(HeadTailBuffer::default()));
    start_streaming_output(&process, &context, Arc::clone(&transcript));

    let late_completion_eligible = manager
        .store_process(
            Arc::clone(&process),
            &context,
            "printf raced",
            &command,
            cwd,
            Instant::now(),
            process_id,
            CompletionBehavior::Wake,
            /*tty*/ false,
            /*network_approval_id*/ None,
            Arc::clone(&transcript),
        )
        .await;

    tokio::time::timeout(Duration::from_secs(10), async {
        while !process.has_exited() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("process should exit");

    late_completion_eligible.store(true, Ordering::Relaxed);
    backfill_late_completion_after_arming(LateCompletionBackfill {
        context: &context,
        process: &process,
        transcript: &transcript,
        late_completion_eligible: &late_completion_eligible,
        raw_command: "printf raced",
        cwd: turn.cwd.as_path(),
        process_id,
        completion_behavior: CompletionBehavior::Wake,
        started_at: Instant::now(),
    })
    .await;

    let queued = session.take_pending_background_process_completions().await;
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].call_id, "call");
    assert_eq!(queued[0].process_id, process_id);
    assert_eq!(queued[0].completion_behavior, CompletionBehavior::Wake);

    manager.release_process_id(process_id).await;
    Ok(())
}

#[test]
fn backfilled_aggregated_output_tail_uses_failure_message_without_stdout() {
    assert_eq!(
        backfilled_aggregated_output_tail("", Some("process failed")),
        "process failed"
    );
}

#[test]
fn backfilled_aggregated_output_tail_appends_failure_message_after_stdout() {
    assert_eq!(
        backfilled_aggregated_output_tail("stdout", Some("process failed")),
        "stdout\nprocess failed"
    );
}

use super::maybe_queue_background_completion;
use super::split_valid_utf8_prefix_with_max;

use crate::background_process_completion::BackgroundProcessCompletionRecord;
use crate::background_process_completion::BackgroundProcessCompletionStatus;
use crate::background_process_completion::CompletionBehavior;
use crate::codex::make_session_and_context;
use crate::state::ActiveTurn;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::TurnStartedEvent;
use pretty_assertions::assert_eq;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

#[test]
fn split_valid_utf8_prefix_respects_max_bytes_for_ascii() {
    let mut buf = b"hello word!".to_vec();

    let first =
        split_valid_utf8_prefix_with_max(&mut buf, /*max_bytes*/ 5).expect("expected prefix");
    assert_eq!(first, b"hello".to_vec());
    assert_eq!(buf, b" word!".to_vec());

    let second =
        split_valid_utf8_prefix_with_max(&mut buf, /*max_bytes*/ 5).expect("expected prefix");
    assert_eq!(second, b" word".to_vec());
    assert_eq!(buf, b"!".to_vec());
}

#[test]
fn split_valid_utf8_prefix_avoids_splitting_utf8_codepoints() {
    // "é" is 2 bytes in UTF-8. With a max of 3 bytes, we should only emit 1 char (2 bytes).
    let mut buf = "ééé".as_bytes().to_vec();

    let first =
        split_valid_utf8_prefix_with_max(&mut buf, /*max_bytes*/ 3).expect("expected prefix");
    assert_eq!(std::str::from_utf8(&first).unwrap(), "é");
    assert_eq!(buf, "éé".as_bytes().to_vec());
}

#[test]
fn split_valid_utf8_prefix_makes_progress_on_invalid_utf8() {
    let mut buf = vec![0xff, b'a', b'b'];

    let first =
        split_valid_utf8_prefix_with_max(&mut buf, /*max_bytes*/ 2).expect("expected prefix");
    assert_eq!(first, vec![0xff]);
    assert_eq!(buf, b"ab".to_vec());
}

#[tokio::test]
async fn auto_completion_ignores_stale_running_status_without_active_turn() {
    let (session, turn) = make_session_and_context().await;
    let session = Arc::new(session);

    *session.active_turn.lock().await = Some(ActiveTurn::default());
    session
        .send_event(
            &turn,
            EventMsg::TurnStarted(TurnStartedEvent {
                turn_id: turn.sub_id.clone(),
                started_at: turn.turn_timing_state.started_at_unix_secs().await,
                model_context_window: turn.model_context_window(),
                collaboration_mode_kind: turn.collaboration_mode.mode,
            }),
        )
        .await;

    *session.active_turn.lock().await = None;

    let late_completion_eligible = Arc::new(AtomicBool::new(false));
    late_completion_eligible.store(true, Ordering::Relaxed);
    let record = BackgroundProcessCompletionRecord {
        call_id: "call-auto-stale-running".to_string(),
        process_id: 42,
        originating_turn_id: turn.sub_id.clone(),
        cwd: turn.cwd.to_path_buf(),
        command: "printf queued".to_string(),
        exit_code: 0,
        duration_ms: i64::try_from(Duration::from_millis(250).as_millis()).unwrap(),
        status: BackgroundProcessCompletionStatus::Completed,
        completion_behavior: CompletionBehavior::Auto,
        is_subagent: false,
        aggregated_output_tail: "queued".to_string(),
    };

    maybe_queue_background_completion(&session, &late_completion_eligible, record.clone()).await;

    assert_eq!(
        session.take_pending_background_process_completions().await,
        vec![record]
    );
}

#[tokio::test]
async fn wake_completion_consumes_late_completion_eligibility_once() {
    let (session, turn) = make_session_and_context().await;
    let session = Arc::new(session);

    let late_completion_eligible = Arc::new(AtomicBool::new(true));
    let record = BackgroundProcessCompletionRecord {
        call_id: "call-wake-once".to_string(),
        process_id: 99,
        originating_turn_id: turn.sub_id.clone(),
        cwd: turn.cwd.to_path_buf(),
        command: "printf queued".to_string(),
        exit_code: 0,
        duration_ms: i64::try_from(Duration::from_millis(250).as_millis()).unwrap(),
        status: BackgroundProcessCompletionStatus::Completed,
        completion_behavior: CompletionBehavior::Wake,
        is_subagent: false,
        aggregated_output_tail: "queued".to_string(),
    };

    maybe_queue_background_completion(&session, &late_completion_eligible, record.clone()).await;

    assert_eq!(
        session.take_pending_background_process_completions().await,
        vec![record.clone()]
    );

    maybe_queue_background_completion(&session, &late_completion_eligible, record).await;

    assert!(
        !session
            .has_queued_background_process_completions_for_next_turn()
            .await
    );
}

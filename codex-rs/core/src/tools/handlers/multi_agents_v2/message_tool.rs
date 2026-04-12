//! Shared argument parsing and dispatch for the v2 text-only agent messaging tools.
//!
//! `send_message` and `followup_task` share the same submission path and differ only in whether the
//! resulting `InterAgentCommunication` should wake the target immediately.

use super::*;
use crate::agent::status::is_handoff_boundary;
use codex_protocol::ThreadId;
use codex_protocol::protocol::InterAgentCommunication;
use std::time::Duration;
use tokio::sync::watch;
use tokio::time::Instant;
use tokio::time::timeout_at;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum MessageDeliveryMode {
    QueueOnly,
    TriggerTurn,
}

#[cfg(test)]
const BLOCKING_FOLLOWUP_HANDOFF_TIMEOUT: Option<Duration> = Some(Duration::from_millis(500));
#[cfg(not(test))]
const BLOCKING_FOLLOWUP_HANDOFF_TIMEOUT: Option<Duration> = None;

#[derive(Debug, Clone)]
pub(crate) struct MessageSubmissionResult {
    pub(crate) status: AgentStatus,
}

#[derive(Debug, Clone)]
struct BlockingFollowupHandoffStatus {
    status: AgentStatus,
    timed_out: bool,
}

impl MessageDeliveryMode {
    /// Returns whether the produced communication should start a turn immediately.
    fn apply(self, communication: InterAgentCommunication) -> InterAgentCommunication {
        match self {
            Self::QueueOnly => InterAgentCommunication {
                trigger_turn: false,
                ..communication
            },
            Self::TriggerTurn => InterAgentCommunication {
                trigger_turn: true,
                ..communication
            },
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Input for the MultiAgentV2 `send_message` tool.
pub(crate) struct SendMessageArgs {
    pub(crate) target: String,
    pub(crate) message: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Input for the MultiAgentV2 `followup_task` tool.
pub(crate) struct FollowupTaskArgs {
    pub(crate) target: String,
    pub(crate) message: String,
    #[serde(default)]
    pub(crate) interrupt: bool,
}

fn message_content(message: String) -> Result<String, FunctionCallError> {
    if message.trim().is_empty() {
        return Err(FunctionCallError::RespondToModel(
            "Empty message can't be sent to an agent".to_string(),
        ));
    }
    Ok(message)
}

/// Handles the shared MultiAgentV2 plain-text message flow for both `send_message` and `followup_task`.
pub(crate) async fn submit_message_string_tool(
    invocation: ToolInvocation,
    mode: MessageDeliveryMode,
    target: String,
    message: String,
    interrupt: bool,
) -> Result<MessageSubmissionResult, FunctionCallError> {
    handle_message_submission(
        invocation,
        mode,
        target,
        message_content(message)?,
        interrupt,
    )
    .await
}

async fn handle_message_submission(
    invocation: ToolInvocation,
    mode: MessageDeliveryMode,
    target: String,
    prompt: String,
    interrupt: bool,
) -> Result<MessageSubmissionResult, FunctionCallError> {
    let ToolInvocation {
        session,
        turn,
        call_id,
        ..
    } = invocation;
    let receiver_thread_id = resolve_agent_target(&session, &turn, &target).await?;
    let receiver_agent = session
        .services
        .agent_control
        .get_agent_metadata(receiver_thread_id)
        .unwrap_or_default();
    if mode == MessageDeliveryMode::TriggerTurn
        && receiver_agent
            .agent_path
            .as_ref()
            .is_some_and(AgentPath::is_root)
    {
        return Err(FunctionCallError::RespondToModel(
            "Tasks can't be assigned to the root agent".to_string(),
        ));
    }
    let blocking_enabled = mode == MessageDeliveryMode::TriggerTurn
        && turn.config.multi_agent_v2.spawn_agent_blocking_enabled;
    let status_rx = if blocking_enabled {
        Some(
            session
                .services
                .agent_control
                .subscribe_status(receiver_thread_id)
                .await
                .map_err(|err| collab_agent_error(receiver_thread_id, err))?,
        )
    } else {
        None
    };
    if interrupt {
        session
            .services
            .agent_control
            .interrupt_agent(receiver_thread_id)
            .await
            .map_err(|err| collab_agent_error(receiver_thread_id, err))?;
    }
    session
        .send_event(
            &turn,
            CollabAgentInteractionBeginEvent {
                call_id: call_id.clone(),
                sender_thread_id: session.conversation_id,
                receiver_thread_id,
                prompt: prompt.clone(),
            }
            .into(),
        )
        .await;
    let receiver_agent_path = receiver_agent.agent_path.clone().ok_or_else(|| {
        FunctionCallError::RespondToModel("target agent is missing an agent_path".to_string())
    })?;
    let communication = InterAgentCommunication::new(
        turn.session_source
            .get_agent_path()
            .unwrap_or_else(AgentPath::root),
        receiver_agent_path,
        Vec::new(),
        prompt.clone(),
        /*trigger_turn*/ true,
    );
    let result = session
        .services
        .agent_control
        .send_inter_agent_communication(receiver_thread_id, mode.apply(communication))
        .await
        .map_err(|err| collab_agent_error(receiver_thread_id, err));
    let blocking_wait = match (&result, status_rx) {
        (Ok(_), Some(status_rx)) => Some(
            wait_for_followup_handoff_status(session.as_ref(), receiver_thread_id, status_rx).await,
        ),
        _ => None,
    };
    let status = match &blocking_wait {
        Some(wait_result) => wait_result.status.clone(),
        None => {
            session
                .services
                .agent_control
                .get_status(receiver_thread_id)
                .await
        }
    };
    session
        .send_event(
            &turn,
            CollabAgentInteractionEndEvent {
                call_id,
                sender_thread_id: session.conversation_id,
                receiver_thread_id,
                receiver_agent_nickname: receiver_agent.agent_nickname,
                receiver_agent_role: receiver_agent.agent_role,
                prompt,
                status: status.clone(),
            }
            .into(),
        )
        .await;
    if let Some(wait_result) = blocking_wait
        && wait_result.timed_out
        && let Some(timeout) = BLOCKING_FOLLOWUP_HANDOFF_TIMEOUT
    {
        return Err(FunctionCallError::RespondToModel(format!(
            "agent `{target}` did not reach its next turn boundary within {} ms",
            timeout.as_millis()
        )));
    }
    result?;

    Ok(MessageSubmissionResult { status })
}

async fn wait_for_followup_handoff_status(
    session: &crate::codex::Session,
    thread_id: ThreadId,
    mut status_rx: watch::Receiver<AgentStatus>,
) -> BlockingFollowupHandoffStatus {
    let deadline = BLOCKING_FOLLOWUP_HANDOFF_TIMEOUT.map(|timeout| Instant::now() + timeout);
    let mut saw_status_change = false;

    loop {
        let status = status_rx.borrow().clone();
        if saw_status_change && is_handoff_boundary(&status) {
            return BlockingFollowupHandoffStatus {
                status,
                timed_out: false,
            };
        }

        match deadline {
            Some(deadline) => match timeout_at(deadline, status_rx.changed()).await {
                Ok(Ok(())) => {
                    saw_status_change = true;
                }
                Ok(Err(_)) => {
                    return BlockingFollowupHandoffStatus {
                        status: session.services.agent_control.get_status(thread_id).await,
                        timed_out: false,
                    };
                }
                Err(_) => {
                    return BlockingFollowupHandoffStatus {
                        status: session.services.agent_control.get_status(thread_id).await,
                        timed_out: true,
                    };
                }
            },
            None => {
                if status_rx.changed().await.is_err() {
                    return BlockingFollowupHandoffStatus {
                        status: session.services.agent_control.get_status(thread_id).await,
                        timed_out: false,
                    };
                }
                saw_status_change = true;
            }
        }
    }
}

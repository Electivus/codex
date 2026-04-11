use super::*;
use crate::codex::Session;
use crate::contextual_user_message::SUBAGENT_NOTIFICATION_CLOSE_TAG;
use crate::contextual_user_message::SUBAGENT_NOTIFICATION_OPEN_TAG;
use codex_protocol::protocol::InterAgentCommunication;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;
use tokio::time::timeout_at;

pub(crate) struct Handler;

impl ToolHandler for Handler {
    type Output = WaitAgentResult;

    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(payload, ToolPayload::Function { .. })
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<Self::Output, FunctionCallError> {
        let ToolInvocation {
            session,
            turn,
            payload,
            call_id,
            ..
        } = invocation;
        let arguments = function_arguments(payload)?;
        let args: WaitArgs = parse_arguments(&arguments)?;
        let timeout_ms = args.timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT_MS);
        let timeout_ms = match timeout_ms {
            ms if ms <= 0 => {
                return Err(FunctionCallError::RespondToModel(
                    "timeout_ms must be greater than zero".to_owned(),
                ));
            }
            ms => ms.clamp(MIN_WAIT_TIMEOUT_MS, MAX_WAIT_TIMEOUT_MS),
        };

        let mut mailbox_seq_rx = session.subscribe_mailbox_seq();

        session
            .send_event(
                &turn,
                CollabWaitingBeginEvent {
                    sender_thread_id: session.conversation_id,
                    receiver_thread_ids: Vec::new(),
                    receiver_agents: Vec::new(),
                    call_id: call_id.clone(),
                }
                .into(),
            )
            .await;

        let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);
        let timed_out = !wait_for_mailbox_change(&mut mailbox_seq_rx, deadline).await;
        let result = WaitAgentResult::from_timed_out(timed_out);

        session
            .send_event(
                &turn,
                CollabWaitingEndEvent {
                    sender_thread_id: session.conversation_id,
                    call_id,
                    agent_statuses: Vec::new(),
                    statuses: HashMap::new(),
                }
                .into(),
            )
            .await;

        Ok(result)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WaitArgs {
    timeout_ms: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct WaitAgentResult {
    pub(crate) message: String,
    pub(crate) timed_out: bool,
}

impl WaitAgentResult {
    fn from_timed_out(timed_out: bool) -> Self {
        let message = if timed_out {
            "Wait timed out."
        } else {
            "Wait completed."
        };
        Self {
            message: message.to_string(),
            timed_out,
        }
    }
}

impl ToolOutput for WaitAgentResult {
    fn log_preview(&self) -> String {
        tool_output_json_text(self, "wait_agent")
    }

    fn success_for_logging(&self) -> bool {
        true
    }

    fn to_response_item(&self, call_id: &str, payload: &ToolPayload) -> ResponseInputItem {
        tool_output_response_item(call_id, payload, self, /*success*/ None, "wait_agent")
    }

    fn code_mode_result(&self, _payload: &ToolPayload) -> JsonValue {
        tool_output_code_mode_result(self, "wait_agent")
    }
}

async fn wait_for_mailbox_change(
    mailbox_seq_rx: &mut tokio::sync::watch::Receiver<u64>,
    deadline: Instant,
) -> bool {
    match timeout_at(deadline, mailbox_seq_rx.changed()).await {
        Ok(Ok(())) => true,
        Ok(Err(_)) | Err(_) => false,
    }
}

pub(crate) async fn wait_for_child_boundary_notification(
    session: Arc<Session>,
    mailbox_seq_rx: &mut tokio::sync::watch::Receiver<u64>,
    child_agent_path: &str,
) -> Result<AgentStatus, FunctionCallError> {
    loop {
        if let Some(communication) = session
            .take_first_matching_mailbox_communication(|communication| {
                communication.author.as_str() == child_agent_path
                    && communication
                        .content
                        .starts_with(SUBAGENT_NOTIFICATION_OPEN_TAG)
                    && communication
                        .content
                        .ends_with(SUBAGENT_NOTIFICATION_CLOSE_TAG)
            })
            .await
        {
            return parse_child_boundary_status_from_notification(&communication);
        }

        mailbox_seq_rx.changed().await.map_err(|_| {
            FunctionCallError::RespondToModel(
                "mailbox closed while waiting for child boundary".to_string(),
            )
        })?;
    }
}

fn parse_child_boundary_status_from_notification(
    communication: &InterAgentCommunication,
) -> Result<AgentStatus, FunctionCallError> {
    let payload = communication
        .content
        .strip_prefix(SUBAGENT_NOTIFICATION_OPEN_TAG)
        .and_then(|content| content.strip_suffix(SUBAGENT_NOTIFICATION_CLOSE_TAG))
        .ok_or_else(|| {
            FunctionCallError::RespondToModel("invalid subagent notification envelope".to_string())
        })?;
    let payload: serde_json::Value = serde_json::from_str(payload).map_err(|err| {
        FunctionCallError::RespondToModel(format!("invalid subagent notification payload: {err}"))
    })?;
    serde_json::from_value(payload["status"].clone()).map_err(|err| {
        FunctionCallError::RespondToModel(format!("invalid subagent notification status: {err}"))
    })
}

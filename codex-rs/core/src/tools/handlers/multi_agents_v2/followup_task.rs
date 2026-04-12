use super::message_tool::FollowupTaskArgs;
use super::message_tool::MessageDeliveryMode;
use super::message_tool::submit_message_string_tool;
use super::*;
use crate::tools::context::FunctionToolOutput;
use serde::Serialize;

#[derive(Debug, Serialize)]
struct FollowupTaskResult {
    status: AgentStatus,
}

pub(crate) struct Handler;

impl ToolHandler for Handler {
    type Output = FunctionToolOutput;

    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(payload, ToolPayload::Function { .. })
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<Self::Output, FunctionCallError> {
        let arguments = function_arguments(invocation.payload.clone())?;
        let args: FollowupTaskArgs = parse_arguments(&arguments)?;
        let blocking_enabled = invocation
            .turn
            .config
            .multi_agent_v2
            .spawn_agent_blocking_enabled;
        let result = submit_message_string_tool(
            invocation,
            MessageDeliveryMode::TriggerTurn,
            args.target,
            args.message,
            args.interrupt,
        )
        .await?;

        if blocking_enabled {
            Ok(FunctionToolOutput::from_text(
                tool_output_json_text(
                    &FollowupTaskResult {
                        status: result.status,
                    },
                    "followup_task",
                ),
                Some(true),
            ))
        } else {
            Ok(FunctionToolOutput::from_text(String::new(), Some(true)))
        }
    }
}

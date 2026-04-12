use std::collections::VecDeque;

use codex_protocol::protocol::AgentStatus;
use codex_protocol::protocol::EventMsg;

const AGENT_HANDOFF_HISTORY_LIMIT: usize = 8;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AgentHandoff {
    pub(crate) sequence: u64,
    pub(crate) status: Option<AgentStatus>,
    recent_statuses: VecDeque<(u64, AgentStatus)>,
}

impl AgentHandoff {
    pub(crate) fn push_status(&mut self, status: AgentStatus) {
        self.sequence += 1;
        self.status = Some(status.clone());
        self.recent_statuses.push_back((self.sequence, status));
        if self.recent_statuses.len() > AGENT_HANDOFF_HISTORY_LIMIT {
            self.recent_statuses.pop_front();
        }
    }

    pub(crate) fn first_status_after(&self, sequence: u64) -> Option<AgentStatus> {
        self.recent_statuses
            .iter()
            .find(|(seq, _)| *seq > sequence)
            .map(|(_, status)| status.clone())
    }

    pub(crate) fn first_status_at_or_after(&self, sequence: u64) -> Option<AgentStatus> {
        self.recent_statuses
            .iter()
            .find(|(seq, _)| *seq >= sequence)
            .map(|(_, status)| status.clone())
    }
}

/// Derive the next agent status from a single emitted event.
/// Returns `None` when the event does not affect status tracking.
pub(crate) fn agent_status_from_event(msg: &EventMsg) -> Option<AgentStatus> {
    match msg {
        EventMsg::TurnStarted(_) => Some(AgentStatus::Running),
        EventMsg::TurnComplete(ev) => Some(AgentStatus::Completed(ev.last_agent_message.clone())),
        EventMsg::TurnAborted(ev) => match ev.reason {
            codex_protocol::protocol::TurnAbortReason::Interrupted => {
                Some(AgentStatus::Interrupted)
            }
            _ => Some(AgentStatus::Errored(format!("{:?}", ev.reason))),
        },
        EventMsg::Error(ev) => Some(AgentStatus::Errored(ev.message.clone())),
        EventMsg::ShutdownComplete => Some(AgentStatus::Shutdown),
        _ => None,
    }
}

pub(crate) fn is_handoff_boundary(status: &AgentStatus) -> bool {
    !matches!(status, AgentStatus::PendingInit | AgentStatus::Running)
}

pub(crate) fn is_final(status: &AgentStatus) -> bool {
    !matches!(
        status,
        AgentStatus::PendingInit | AgentStatus::Running | AgentStatus::Interrupted
    )
}

use std::fmt::Write as _;
use std::path::PathBuf;

use codex_protocol::models::DeveloperInstructions;
use codex_protocol::models::ResponseItem;
use serde::Deserialize;
use serde::Serialize;

/// Controls whether a late background-process completion should trigger follow-up work.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CompletionBehavior {
    #[default]
    Auto,
    Wake,
    Ignore,
}

impl CompletionBehavior {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Wake => "wake",
            Self::Ignore => "ignore",
        }
    }
}

impl std::fmt::Display for CompletionBehavior {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BackgroundProcessCompletionStatus {
    Completed,
    Failed,
}

impl BackgroundProcessCompletionStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

impl std::fmt::Display for BackgroundProcessCompletionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Session-scoped record queued for the next turn when a background process finishes late.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BackgroundProcessCompletionRecord {
    pub(crate) call_id: String,
    pub(crate) process_id: i32,
    pub(crate) originating_turn_id: String,
    pub(crate) cwd: PathBuf,
    pub(crate) command: String,
    pub(crate) exit_code: i32,
    pub(crate) duration_ms: i64,
    pub(crate) status: BackgroundProcessCompletionStatus,
    pub(crate) completion_behavior: CompletionBehavior,
    pub(crate) is_subagent: bool,
    pub(crate) aggregated_output_tail: String,
}

impl BackgroundProcessCompletionRecord {
    pub(crate) fn matches_completion(&self, call_id: &str, process_id: i32) -> bool {
        self.call_id == call_id && self.process_id == process_id
    }

    pub(crate) fn runtime_note_message(&self) -> ResponseItem {
        DeveloperInstructions::new(self.runtime_note()).into()
    }

    fn runtime_note(&self) -> String {
        let mut note = format!(
            "Background process completed after the previous turn ended. Command: `{}`. Exit code: {}. Status: {}.",
            self.command, self.exit_code, self.status
        );
        if !self.aggregated_output_tail.is_empty() {
            let _ = write!(note, " Output tail:\n{}", self.aggregated_output_tail);
        }
        note
    }
}

const BACKGROUND_PROCESS_OUTPUT_TAIL_MAX_CHARS: usize = 4_000;

pub(crate) fn bounded_output_tail(output: &str) -> String {
    let char_count = output.chars().count();
    if char_count <= BACKGROUND_PROCESS_OUTPUT_TAIL_MAX_CHARS {
        output.to_string()
    } else {
        let tail = output
            .chars()
            .skip(char_count - BACKGROUND_PROCESS_OUTPUT_TAIL_MAX_CHARS)
            .collect::<String>();
        format!("...[truncated]\n{tail}")
    }
}

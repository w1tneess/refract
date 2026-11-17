use serde::{Deserialize, Serialize};
use crate::error::RefractError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobState {
    Queued,
    Probing,
    NeedsPassword,
    Running,
    Verifying,
    Saved,
    Rejected,
    Failed,
    Canceled,
}

impl JobState {
    pub fn is_terminal(&self) -> bool {
        matches!(self, JobState::Saved | JobState::Rejected | JobState::Failed | JobState::Canceled)
    }

    pub fn label(&self) -> &'static str {
        match self {
            JobState::Queued => "Waiting",
            JobState::Probing => "Checking file",
            JobState::NeedsPassword => "Needs password",
            JobState::Running => "Working",
            JobState::Verifying => "Checking result",
            JobState::Saved => "Done",
            JobState::Rejected => "Can't do this",
            JobState::Failed => "Failed",
            JobState::Canceled => "Canceled",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub tool_id: String,
    pub input_names: Vec<String>,
    pub input_size: u64,
    pub output_name: Option<String>,
    pub output_size: Option<u64>,
    pub state: JobState,
    pub error: Option<RefractError>,
}

impl Job {
    pub fn new(id: impl Into<String>, tool_id: impl Into<String>, input_names: Vec<String>, input_size: u64) -> Self {
        Self {
            id: id.into(),
            tool_id: tool_id.into(),
            input_names,
            input_size,
            output_name: None,
            output_size: None,
            state: JobState::Queued,
            error: None,
        }
    }
}

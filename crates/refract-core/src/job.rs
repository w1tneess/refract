use crate::error::{ErrorCode, RefractError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    pub const ALL: &'static [JobState] = &[
        JobState::Queued,
        JobState::Probing,
        JobState::NeedsPassword,
        JobState::Running,
        JobState::Verifying,
        JobState::Saved,
        JobState::Rejected,
        JobState::Failed,
        JobState::Canceled,
    ];

    /// Canonical 9-state transition table matching `@refract/core` (`packages/core/src/jobs/state.ts`).
    pub fn can_transition(self, next: JobState) -> bool {
        match self {
            JobState::Queued => matches!(next, JobState::Probing | JobState::Canceled),
            JobState::Probing => matches!(
                next,
                JobState::Running
                    | JobState::NeedsPassword
                    | JobState::Rejected
                    | JobState::Canceled
            ),
            JobState::NeedsPassword => matches!(next, JobState::Probing | JobState::Canceled),
            JobState::Running => {
                matches!(
                    next,
                    JobState::Verifying | JobState::Failed | JobState::Canceled
                )
            }
            JobState::Verifying => {
                matches!(
                    next,
                    JobState::Saved | JobState::Failed | JobState::Canceled
                )
            }
            JobState::Saved | JobState::Rejected | JobState::Failed | JobState::Canceled => false,
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            JobState::Saved | JobState::Rejected | JobState::Failed | JobState::Canceled
        )
    }

    pub fn is_active(self) -> bool {
        matches!(
            self,
            JobState::Probing | JobState::Running | JobState::Verifying
        )
    }

    pub fn label(self) -> &'static str {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BatchStatus {
    Processing,
    Waiting,
    Error,
    Finished,
}

/// Priority matches `@refract/core`: Processing -> Waiting -> Error -> Finished.
pub fn batch_status(states: &[JobState]) -> BatchStatus {
    if states.iter().any(|s| s.is_active()) {
        return BatchStatus::Processing;
    }
    if states
        .iter()
        .any(|s| matches!(s, JobState::Queued | JobState::NeedsPassword))
    {
        return BatchStatus::Waiting;
    }
    if states
        .iter()
        .any(|s| matches!(s, JobState::Failed | JobState::Rejected))
    {
        return BatchStatus::Error;
    }
    BatchStatus::Finished
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
    pub fn new(
        id: impl Into<String>,
        tool_id: impl Into<String>,
        input_names: Vec<String>,
        input_size: u64,
    ) -> Self {
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

    pub fn transition(&mut self, next: JobState) -> Result<(), RefractError> {
        if !self.state.can_transition(next) {
            return Err(RefractError::new(
                ErrorCode::EngineFailed,
                format!(
                    "Illegal job state transition: {:?} -> {:?}",
                    self.state, next
                ),
            ));
        }
        self.state = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn exhaustive_state_transition_matrix_matches_contract() {
        let allowed: HashSet<(JobState, JobState)> = [
            (JobState::Queued, JobState::Probing),
            (JobState::Queued, JobState::Canceled),
            (JobState::Probing, JobState::Running),
            (JobState::Probing, JobState::NeedsPassword),
            (JobState::Probing, JobState::Rejected),
            (JobState::Probing, JobState::Canceled),
            (JobState::NeedsPassword, JobState::Probing),
            (JobState::NeedsPassword, JobState::Canceled),
            (JobState::Running, JobState::Verifying),
            (JobState::Running, JobState::Failed),
            (JobState::Running, JobState::Canceled),
            (JobState::Verifying, JobState::Saved),
            (JobState::Verifying, JobState::Failed),
            (JobState::Verifying, JobState::Canceled),
        ]
        .into_iter()
        .collect();

        assert_eq!(JobState::ALL.len(), 9);
        for &from in JobState::ALL {
            for &to in JobState::ALL {
                let expected = allowed.contains(&(from, to));
                assert_eq!(
                    from.can_transition(to),
                    expected,
                    "Mismatch for transition {:?} -> {:?}",
                    from,
                    to
                );
            }
        }
    }

    #[test]
    fn terminal_and_active_helpers_are_consistent() {
        for &state in JobState::ALL {
            let has_outgoing = JobState::ALL.iter().any(|&next| state.can_transition(next));
            assert_eq!(state.is_terminal(), !has_outgoing);
            if state.is_active() {
                assert!(!state.is_terminal());
                assert!(state.can_transition(JobState::Canceled));
            }
        }
    }

    #[test]
    fn job_transition_enforces_legal_paths_and_rejects_illegal_ones() {
        let mut job = Job::new("1", "compress-pdf", vec!["locked.pdf".into()], 1024);
        assert_eq!(job.state, JobState::Queued);
        assert!(job.transition(JobState::Running).is_err());
        assert!(job.transition(JobState::Saved).is_err());

        job.transition(JobState::Probing).unwrap();
        job.transition(JobState::NeedsPassword).unwrap();
        assert!(job.transition(JobState::Running).is_err());
        job.transition(JobState::Probing).unwrap();
        job.transition(JobState::Running).unwrap();
        job.transition(JobState::Verifying).unwrap();
        job.transition(JobState::Saved).unwrap();
        assert!(job.transition(JobState::Canceled).is_err());
    }

    #[test]
    fn batch_status_priority_matches_web_core() {
        assert_eq!(batch_status(&[]), BatchStatus::Finished);
        assert_eq!(
            batch_status(&[JobState::Saved, JobState::Canceled]),
            BatchStatus::Finished
        );
        assert_eq!(
            batch_status(&[JobState::Saved, JobState::Rejected]),
            BatchStatus::Error
        );
        assert_eq!(
            batch_status(&[JobState::Failed, JobState::NeedsPassword]),
            BatchStatus::Waiting
        );
        assert_eq!(
            batch_status(&[JobState::Queued, JobState::Verifying]),
            BatchStatus::Processing
        );
    }
}

//! Run context shared by every Desktop job execution path (Stage 2.3.6).
//!
//! A run carries one [`CancelToken`] (handed to every cancellable engine call and to the final
//! save) and an `on_stage` callback that reports `Running` once inputs pass preflight and
//! `Verifying` just before outputs are checked and saved.

use refract_core::cancel::CancelToken;
use refract_core::error::RefractError;
use refract_core::job::JobState;
use std::collections::HashSet;

/// Retry is offered for `Failed` jobs (whatever the error, as on Web) and for `Canceled` ones.
/// `Rejected` jobs are never retried: their input or options must change first.
pub fn can_retry(state: JobState) -> bool {
    matches!(state, JobState::Failed | JobState::Canceled)
}

/// Waiting, password-waiting and running jobs can be canceled.
pub fn can_cancel(state: JobState) -> bool {
    matches!(state, JobState::Queued | JobState::NeedsPassword) || state.is_active()
}

/// Jobs that are running stay put: they can't be removed or moved until they finish.
pub fn can_remove_or_move(state: JobState) -> bool {
    !state.is_active()
}

/// The unit of work currently running (its job ids and cancel token), plus cancel requests for
/// jobs that started (`Probing`) a moment before their unit was registered.
///
/// Lock order: take the jobs lock before this one, never the other way round.
#[derive(Default)]
pub struct ActiveRun {
    current: Option<(Vec<i32>, CancelToken)>,
    pending_cancel: HashSet<i32>,
}

impl ActiveRun {
    /// Register `ids` as the running unit and return its token (already canceled if a cancel
    /// for one of them arrived just before).
    pub fn start(&mut self, ids: &[i32]) -> CancelToken {
        let token = CancelToken::new();
        let mut canceled = false;
        for id in ids {
            canceled |= self.pending_cancel.remove(id);
        }
        if canceled {
            token.cancel();
        }
        self.current = Some((ids.to_vec(), token.clone()));
        token
    }

    pub fn finish(&mut self, ids: &[i32]) {
        self.current = None;
        self.pending_cancel.retain(|id| !ids.contains(id));
    }

    pub fn is_running(&self, id: i32) -> bool {
        self.current
            .as_ref()
            .is_some_and(|(ids, _)| ids.contains(&id))
    }

    /// Cancel the running unit if it contains `id`; otherwise remember the request for when the
    /// job's unit starts. Call only for jobs that are active.
    pub fn cancel(&mut self, id: i32) {
        match &self.current {
            Some((ids, token)) if ids.contains(&id) => token.cancel(),
            _ => {
                self.pending_cancel.insert(id);
            }
        }
    }

    /// Cancel whatever is running now.
    pub fn cancel_current(&mut self) {
        if let Some((_, token)) = &self.current {
            token.cancel();
        }
    }
}

pub struct RunCtx<'a> {
    pub cancel: &'a CancelToken,
    on_stage: &'a dyn Fn(JobState),
}

impl<'a> RunCtx<'a> {
    pub fn new(cancel: &'a CancelToken, on_stage: &'a dyn Fn(JobState)) -> Self {
        Self { cancel, on_stage }
    }

    /// Report that the run reached `stage` (`Running` or `Verifying`).
    pub fn stage(&self, stage: JobState) {
        (self.on_stage)(stage);
    }

    /// Inputs passed preflight: stop here if canceled, otherwise report `Running`.
    pub fn begin_work(&self) -> Result<(), RefractError> {
        self.cancel.check()?;
        self.stage(JobState::Running);
        Ok(())
    }

    /// About to check and save outputs: stop here if canceled, otherwise report `Verifying`.
    pub fn begin_verify(&self) -> Result<(), RefractError> {
        self.cancel.check()?;
        self.stage(JobState::Verifying);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_matches_the_state_table() {
        for &s in JobState::ALL {
            assert_eq!(
                can_retry(s),
                matches!(s, JobState::Failed | JobState::Canceled)
            );
            // Anything cancelable has a legal move to Canceled.
            if can_cancel(s) {
                assert!(s.can_transition(JobState::Canceled), "{s:?}");
            }
            assert_eq!(can_remove_or_move(s), !s.is_active());
        }
        assert!(!can_retry(JobState::Rejected));
        assert!(!can_cancel(JobState::Saved));
    }

    #[test]
    fn cancel_reaches_the_running_unit_or_waits_for_it() {
        let mut active = ActiveRun::default();
        let token = active.start(&[1, 2]);
        assert!(active.is_running(2) && !active.is_running(3));
        active.cancel(2);
        assert!(token.is_canceled());
        active.finish(&[1, 2]);
        assert!(!active.is_running(1));

        // Cancel arriving before job 3's unit is registered applies when it starts.
        active.cancel(3);
        assert!(active.start(&[3]).is_canceled());
        active.finish(&[3]);
        // ...and only once: a later attempt starts clean.
        assert!(!active.start(&[3]).is_canceled());
        active.finish(&[3]);

        let token = active.start(&[4]);
        active.cancel_current();
        assert!(token.is_canceled());
    }
}

//! Run context shared by every Desktop job execution path (Stage 2.3.6).
//!
//! A run carries one [`CancelToken`] (handed to every cancellable engine call and to the final
//! save) and an `on_stage` callback that reports `Running` once inputs pass preflight and
//! `Verifying` just before outputs are checked and saved.

use refract_core::cancel::CancelToken;
use refract_core::error::RefractError;
use refract_core::job::JobState;

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

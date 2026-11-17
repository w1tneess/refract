use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorCode {
    EmptyFile,
    DamagedFile,
    UnsupportedFormat,
    LimitExceeded,
    NeedsPassword,
    WrongPassword,
    EngineFailed,
    VerificationFailed,
    Canceled,
}

#[derive(Debug, Clone, Error, Serialize, Deserialize)]
#[error("{message}")]
pub struct RefractError {
    pub code: ErrorCode,
    pub message: String,
    pub details: Option<String>,
}

impl RefractError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
        }
    }

    pub fn with_details(code: ErrorCode, message: impl Into<String>, details: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: Some(details.into()),
        }
    }
}

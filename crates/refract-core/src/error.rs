use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ErrorCode {
    EmptyFile,
    DamagedFile,
    UnsupportedFormat,
    InvalidOption,
    LimitExceeded,
    NeedsPassword,
    WrongPassword,
    MissingDependency,
    EngineFailed,
    VerificationFailed,
    Canceled,
}

impl ErrorCode {
    /// Whether this error represents a pre-execution rejection (maps to `JobState::Rejected`)
    /// rather than an execution/engine failure (`JobState::Failed`).
    pub fn is_rejection(self) -> bool {
        matches!(
            self,
            ErrorCode::EmptyFile
                | ErrorCode::UnsupportedFormat
                | ErrorCode::InvalidOption
                | ErrorCode::LimitExceeded
                | ErrorCode::MissingDependency
        )
    }

    pub fn is_cancellation(self) -> bool {
        matches!(self, ErrorCode::Canceled)
    }

    pub fn is_password_error(self) -> bool {
        matches!(self, ErrorCode::NeedsPassword | ErrorCode::WrongPassword)
    }

    /// Map native error codes onto `@refract/core`'s canonical error categories.
    pub fn canonical_category(self) -> &'static str {
        match self {
            ErrorCode::EmptyFile | ErrorCode::DamagedFile => "Corrupt",
            ErrorCode::UnsupportedFormat
            | ErrorCode::InvalidOption
            | ErrorCode::MissingDependency => "Unsupported",
            ErrorCode::LimitExceeded => "TooLarge",
            ErrorCode::NeedsPassword | ErrorCode::WrongPassword => "Encrypted",
            ErrorCode::EngineFailed | ErrorCode::VerificationFailed => "Internal",
            ErrorCode::Canceled => "Canceled",
        }
    }
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

    pub fn with_details(
        code: ErrorCode,
        message: impl Into<String>,
        details: impl Into<String>,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            details: Some(details.into()),
        }
    }

    pub fn canceled() -> Self {
        Self::new(ErrorCode::Canceled, "Job was canceled.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_code_semantics_classify_rejections_and_failures() {
        for code in [
            ErrorCode::EmptyFile,
            ErrorCode::UnsupportedFormat,
            ErrorCode::InvalidOption,
            ErrorCode::LimitExceeded,
            ErrorCode::MissingDependency,
        ] {
            assert!(code.is_rejection(), "{:?} should be rejection", code);
            assert!(!code.is_cancellation());
            assert!(!code.is_password_error());
        }

        for code in [
            ErrorCode::DamagedFile,
            ErrorCode::EngineFailed,
            ErrorCode::VerificationFailed,
        ] {
            assert!(!code.is_rejection(), "{:?} should not be rejection", code);
        }

        assert!(ErrorCode::Canceled.is_cancellation());
        assert!(!ErrorCode::Canceled.is_rejection());

        assert!(ErrorCode::NeedsPassword.is_password_error());
        assert!(ErrorCode::WrongPassword.is_password_error());
        assert_eq!(ErrorCode::NeedsPassword.canonical_category(), "Encrypted");
        assert_eq!(ErrorCode::LimitExceeded.canonical_category(), "TooLarge");
        assert_eq!(ErrorCode::DamagedFile.canonical_category(), "Corrupt");
        assert_eq!(
            ErrorCode::UnsupportedFormat.canonical_category(),
            "Unsupported"
        );
        assert_eq!(ErrorCode::EngineFailed.canonical_category(), "Internal");
        assert_eq!(ErrorCode::Canceled.canonical_category(), "Canceled");
    }
}

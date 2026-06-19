use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DomainError {
    #[error("{field} must not be empty")]
    EmptyRequiredText { field: &'static str },

    #[error("{field} has invalid value {value}")]
    InvalidEnumValue { field: &'static str, value: String },

    #[error("permissions contains duplicate value {permission}")]
    DuplicatePermission { permission: String },

    #[error("freshness_window_days must be positive")]
    InvalidFreshnessWindowDays,

    #[error("coverage_end_at must be greater than or equal to coverage_start_at")]
    InvalidCoverageWindow,

    #[error("attachment filename must not be empty")]
    EmptyAttachmentFilename,

    #[error("attachment filename must be at most 255 bytes")]
    AttachmentFilenameTooLong,

    #[error("attachment filename contains unsupported characters")]
    InvalidAttachmentFilenameCharacters,

    #[error("attachment filename must not be . or ..")]
    ReservedAttachmentFilename,
}

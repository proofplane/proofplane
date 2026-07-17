use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DomainError {
    #[error("{field} must not be empty")]
    EmptyRequiredText { field: &'static str },

    #[error("{field} must not be blank when provided")]
    BlankOptionalText { field: &'static str },

    #[error("{field} must be at most {maximum} characters")]
    OptionalTextTooLong { field: &'static str, maximum: usize },

    #[error("{field} has invalid value {value}")]
    InvalidEnumValue { field: &'static str, value: String },

    #[error("permissions contains duplicate value {permission}")]
    DuplicatePermission { permission: String },

    #[error("valid_until must be greater than or equal to valid_from")]
    InvalidCoverageWindow,

    #[error("filename must not be empty")]
    EmptySubmissionFilename,

    #[error("filename must be at most 255 bytes")]
    SubmissionFilenameTooLong,

    #[error("filename contains unsupported characters")]
    InvalidSubmissionFilenameCharacters,

    #[error("filename must not be . or ..")]
    ReservedSubmissionFilename,
}

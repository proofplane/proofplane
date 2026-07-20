use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DomainError {
    #[error("{field} must not be empty")]
    EmptyRequiredText { field: &'static str },

    #[error("{field} must not be blank when provided")]
    BlankOptionalText { field: &'static str },

    #[error("{field} must be at most {maximum} characters")]
    RequiredTextTooLong { field: &'static str, maximum: usize },

    #[error("{field} must be at most {maximum} characters")]
    OptionalTextTooLong { field: &'static str, maximum: usize },

    #[error("{field} has invalid value {value}")]
    InvalidEnumValue { field: &'static str, value: String },

    #[error("permissions contains duplicate value {permission}")]
    DuplicatePermission { permission: String },

    #[error("control_ids contains a duplicate value")]
    DuplicatePolicyControlId,

    #[error("valid_until must be greater than or equal to valid_from")]
    InvalidCoverageWindow,

    #[error("document filename must not be empty")]
    EmptyDocumentFilename,

    #[error("document filename must be at most 255 bytes")]
    DocumentFilenameTooLong,

    #[error("document filename contains unsupported characters")]
    InvalidDocumentFilenameCharacters,

    #[error("document filename must not be . or ..")]
    ReservedDocumentFilename,
}

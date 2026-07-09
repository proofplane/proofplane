use chrono::{DateTime, SecondsFormat, Utc};
use rmcp::{model::ErrorCode, service::RequestContext, RoleServer};
use serde::Serialize;
use serde_json::json;
use uuid::Uuid;

use super::super::context::McpRequestContext;
use crate::{
    domain::{DomainError, WorkspacePermission},
    repository::{ConflictKind, Error as RepositoryError},
    services::Error as ServiceError,
    validation::Validation,
};

#[derive(Debug, Serialize)]
struct FieldIssue {
    field: &'static str,
    message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum McpArgumentError {
    Missing { field: &'static str },
    InvalidUuid { field: &'static str },
    InvalidTimestamp { field: &'static str },
}

pub(super) fn authorize_token_workspace(
    ctx: &RequestContext<RoleServer>,
    permission: WorkspacePermission,
) -> Result<McpRequestContext, rmcp::ErrorData> {
    let parts = ctx
        .extensions
        .get::<http::request::Parts>()
        .ok_or_else(|| rmcp::ErrorData::internal_error("request context unavailable", None))?;

    McpRequestContext::authorize_token_workspace(&parts.extensions, &parts.headers, permission)
}

pub(super) fn parse_uuid_arg(
    field: &'static str,
    value: Option<String>,
) -> Result<Uuid, rmcp::ErrorData> {
    required_uuid(field, value)
        .into_result()
        .map_err(argument_errors)
}

pub(super) fn required_uuid(
    field: &'static str,
    value: Option<String>,
) -> Validation<Uuid, McpArgumentError> {
    match value {
        Some(value) => Uuid::parse_str(&value)
            .map(Validation::valid)
            .unwrap_or_else(|_| Validation::invalid(McpArgumentError::InvalidUuid { field })),
        None => Validation::invalid(McpArgumentError::Missing { field }),
    }
}

pub(super) fn optional_timestamp(
    field: &'static str,
    value: Option<String>,
) -> Validation<Option<DateTime<Utc>>, McpArgumentError> {
    match value {
        Some(value) => DateTime::parse_from_rfc3339(&value)
            .map(|parsed| Validation::valid(Some(parsed.with_timezone(&Utc))))
            .unwrap_or_else(|_| Validation::invalid(McpArgumentError::InvalidTimestamp { field })),
        None => Validation::valid(None),
    }
}

pub(super) fn required_timestamp(
    field: &'static str,
    value: Option<String>,
) -> Validation<DateTime<Utc>, McpArgumentError> {
    match value {
        Some(value) => DateTime::parse_from_rfc3339(&value)
            .map(|parsed| Validation::valid(parsed.with_timezone(&Utc)))
            .unwrap_or_else(|_| Validation::invalid(McpArgumentError::InvalidTimestamp { field })),
        None => Validation::invalid(McpArgumentError::Missing { field }),
    }
}

pub(super) fn argument_errors(errors: Vec<McpArgumentError>) -> rmcp::ErrorData {
    let issues: Vec<_> = errors.into_iter().map(FieldIssue::from).collect();

    rmcp::ErrorData::invalid_params(
        "tool argument validation failed",
        Some(json!({
            "problem": {
                "code": "validation_failed",
                "message": "tool argument validation failed",
                "field_issues": issues,
            }
        })),
    )
}

pub(super) fn domain_errors(errors: Vec<DomainError>) -> rmcp::ErrorData {
    let issues: Vec<_> = errors.into_iter().map(FieldIssue::from).collect();

    rmcp::ErrorData::invalid_params(
        "tool argument validation failed",
        Some(json!({
            "problem": {
                "code": "validation_failed",
                "message": "tool argument validation failed",
                "field_issues": issues,
            }
        })),
    )
}

impl From<McpArgumentError> for FieldIssue {
    fn from(error: McpArgumentError) -> Self {
        match error {
            McpArgumentError::Missing { field } => Self {
                field,
                message: "is required".to_owned(),
            },
            McpArgumentError::InvalidUuid { field } => Self {
                field,
                message: "must be a UUID".to_owned(),
            },
            McpArgumentError::InvalidTimestamp { field } => Self {
                field,
                message: "must be an RFC 3339 timestamp".to_owned(),
            },
        }
    }
}

impl From<DomainError> for FieldIssue {
    fn from(error: DomainError) -> Self {
        match error {
            DomainError::EmptyRequiredText { field } => Self {
                field,
                message: format!("{field} must not be empty"),
            },
            DomainError::BlankOptionalText { field } => Self {
                field,
                message: format!("{field} must not be blank when provided"),
            },
            DomainError::OptionalTextTooLong { field, maximum } => Self {
                field,
                message: format!("{field} must be at most {maximum} characters"),
            },
            DomainError::InvalidCoverageWindow => Self {
                field: "coverage_end_at",
                message: "coverage_end_at must be greater than or equal to coverage_start_at"
                    .to_owned(),
            },
            DomainError::InvalidFreshnessWindowDays => Self {
                field: "freshness_window_days",
                message: "freshness_window_days must be positive".to_owned(),
            },
            DomainError::DuplicatePermission { permission } => Self {
                field: "permissions",
                message: format!("permissions contains duplicate value {permission}"),
            },
            DomainError::InvalidEnumValue { field, value } => Self {
                field,
                message: format!("{field} has invalid value {value}"),
            },
            DomainError::EmptyAttachmentFilename => Self {
                field: "filename",
                message: "attachment filename must not be empty".to_owned(),
            },
            DomainError::AttachmentFilenameTooLong => Self {
                field: "filename",
                message: "attachment filename must be at most 255 bytes".to_owned(),
            },
            DomainError::InvalidAttachmentFilenameCharacters => Self {
                field: "filename",
                message: "attachment filename contains unsupported characters".to_owned(),
            },
            DomainError::ReservedAttachmentFilename => Self {
                field: "filename",
                message: "attachment filename must not be . or ..".to_owned(),
            },
        }
    }
}

pub(super) fn not_found() -> rmcp::ErrorData {
    rmcp::ErrorData::resource_not_found(
        "resource not found",
        Some(json!({
            "problem": {
                "code": "not_found",
                "message": "resource not found",
            }
        })),
    )
}

pub(super) fn conflict(code: &'static str, message: &'static str) -> rmcp::ErrorData {
    rmcp::ErrorData::new(
        ErrorCode(-32000),
        message,
        Some(json!({
            "problem": {
                "code": code,
                "message": message,
            }
        })),
    )
}

pub(super) fn service_error(error: ServiceError) -> rmcp::ErrorData {
    if let ServiceError::Repository(RepositoryError::Conflict(kind)) = error {
        return repository_conflict(kind);
    }

    tracing::error!(%error, "MCP service failure");
    rmcp::ErrorData::internal_error(
        "dependency failure",
        Some(json!({
            "problem": {
                "code": "dependency_failed",
                "message": "a dependency failed while handling the tool call",
            }
        })),
    )
}

fn repository_conflict(kind: ConflictKind) -> rmcp::ErrorData {
    conflict(kind.code(), kind.message())
}

pub(super) fn format_datetime(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::{
        argument_errors, domain_errors, optional_timestamp, required_timestamp, required_uuid,
        FieldIssue, McpArgumentError,
    };
    use crate::domain::DomainError;
    use rmcp::model::ErrorCode;

    fn field_issues(error: &rmcp::ErrorData) -> Vec<(String, String)> {
        error.data.as_ref().expect("error data")["problem"]["field_issues"]
            .as_array()
            .expect("field issues")
            .iter()
            .map(|issue| {
                (
                    issue["field"].as_str().expect("field").to_owned(),
                    issue["message"].as_str().expect("message").to_owned(),
                )
            })
            .collect()
    }

    #[test]
    fn invalid_now_maps_to_rfc3339_timestamp_message() {
        let error = optional_timestamp("now", Some("not-a-date".to_owned()))
            .into_result()
            .map_err(argument_errors)
            .expect_err("invalid timestamp");

        assert_eq!(
            field_issues(&error),
            [("now".to_owned(), "must be an RFC 3339 timestamp".to_owned())]
        );
    }

    #[test]
    fn argument_errors_preserve_mcp_validation_problem_shape() {
        let error = argument_errors(vec![
            McpArgumentError::Missing {
                field: "workspace_id",
            },
            McpArgumentError::InvalidUuid {
                field: "submission_id",
            },
            McpArgumentError::InvalidTimestamp { field: "now" },
        ]);

        assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
        assert_eq!(error.message, "tool argument validation failed");
        let problem = &error.data.as_ref().expect("error data")["problem"];
        assert_eq!(problem["code"], "validation_failed");
        assert_eq!(problem["message"], "tool argument validation failed");
        assert_eq!(
            field_issues(&error),
            [
                ("workspace_id".to_owned(), "is required".to_owned()),
                ("submission_id".to_owned(), "must be a UUID".to_owned()),
                ("now".to_owned(), "must be an RFC 3339 timestamp".to_owned()),
            ]
        );
    }

    #[test]
    fn argument_error_maps_to_field_issue_messages() {
        assert_eq!(
            FieldIssue::from(McpArgumentError::Missing {
                field: "workspace_id"
            })
            .message,
            "is required"
        );
        assert_eq!(
            FieldIssue::from(McpArgumentError::InvalidUuid {
                field: "workspace_id"
            })
            .message,
            "must be a UUID"
        );
        assert_eq!(
            FieldIssue::from(McpArgumentError::InvalidTimestamp { field: "now" }).message,
            "must be an RFC 3339 timestamp"
        );
    }

    #[test]
    fn required_timestamp_maps_missing_and_invalid_values() {
        let missing = required_timestamp("coverage_start_at", None)
            .into_result()
            .map_err(argument_errors)
            .expect_err("missing timestamp");
        assert_eq!(
            field_issues(&missing),
            [("coverage_start_at".to_owned(), "is required".to_owned())]
        );

        let invalid = required_timestamp("coverage_start_at", Some("nope".to_owned()))
            .into_result()
            .map_err(argument_errors)
            .expect_err("invalid timestamp");
        assert_eq!(
            field_issues(&invalid),
            [(
                "coverage_start_at".to_owned(),
                "must be an RFC 3339 timestamp".to_owned()
            )]
        );
    }

    #[test]
    fn domain_errors_map_to_validation_field_issues() {
        let error = domain_errors(vec![
            DomainError::EmptyRequiredText {
                field: "source_system",
            },
            DomainError::OptionalTextTooLong {
                field: "description",
                maximum: 4_000,
            },
            DomainError::InvalidCoverageWindow,
        ]);

        assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
        assert_eq!(
            field_issues(&error),
            [
                (
                    "source_system".to_owned(),
                    "source_system must not be empty".to_owned()
                ),
                (
                    "description".to_owned(),
                    "description must be at most 4000 characters".to_owned()
                ),
                (
                    "coverage_end_at".to_owned(),
                    "coverage_end_at must be greater than or equal to coverage_start_at".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn required_uuid_maps_missing_to_required_message() {
        let error = required_uuid("workspace_id", None)
            .into_result()
            .map_err(argument_errors)
            .expect_err("missing UUID");

        assert_eq!(
            field_issues(&error),
            [("workspace_id".to_owned(), "is required".to_owned())]
        );
    }
}

use chrono::{DateTime, SecondsFormat, Utc};
use rmcp::{model::ErrorCode, service::RequestContext, RoleServer};
use serde::Serialize;
use serde_json::json;
use uuid::Uuid;

use super::super::context::McpRequestContext;
use crate::{
    domain::{BatchError, DomainError, WorkspacePermission},
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
    DuplicateIds { field: &'static str, ids: Vec<Uuid> },
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

pub(super) fn authorize_connection(
    ctx: &RequestContext<RoleServer>,
) -> Result<McpRequestContext, rmcp::ErrorData> {
    let parts = ctx
        .extensions
        .get::<http::request::Parts>()
        .ok_or_else(|| rmcp::ErrorData::internal_error("request context unavailable", None))?;

    McpRequestContext::authorize_connection(&parts.extensions, &parts.headers)
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

    field_errors(issues)
}

pub(super) fn invalid_field(field: &'static str, message: impl Into<String>) -> rmcp::ErrorData {
    field_errors(vec![FieldIssue {
        field,
        message: message.into(),
    }])
}

fn field_errors(issues: Vec<FieldIssue>) -> rmcp::ErrorData {
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
            McpArgumentError::DuplicateIds { field, ids } => Self {
                field,
                message: format!(
                    "must not contain duplicate ids: {}",
                    ids.iter()
                        .map(Uuid::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
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
            DomainError::RequiredTextTooLong { field, maximum } => Self {
                field,
                message: format!("{field} must be at most {maximum} characters"),
            },
            DomainError::OptionalTextTooLong { field, maximum } => Self {
                field,
                message: format!("{field} must be at most {maximum} characters"),
            },
            DomainError::InvalidCoverageWindow => Self {
                field: "valid_until",
                message: "valid_until must be greater than or equal to valid_from".to_owned(),
            },
            DomainError::DuplicatePermission { permission } => Self {
                field: "permissions",
                message: format!("permissions contains duplicate value {permission}"),
            },
            DomainError::DuplicatePolicyControlId => Self {
                field: "control_ids",
                message: "control_ids contains a duplicate value".to_owned(),
            },
            DomainError::InvalidEnumValue { field, value } => Self {
                field,
                message: format!("{field} has invalid value {value}"),
            },
            DomainError::EmptyDocumentFilename => Self {
                field: "filename",
                message: "document filename must not be empty".to_owned(),
            },
            DomainError::DocumentFilenameTooLong => Self {
                field: "filename",
                message: "document filename must be at most 255 bytes".to_owned(),
            },
            DomainError::InvalidDocumentFilenameCharacters => Self {
                field: "filename",
                message: "document filename contains unsupported characters".to_owned(),
            },
            DomainError::ReservedDocumentFilename => Self {
                field: "filename",
                message: "document filename must not be . or ..".to_owned(),
            },
        }
    }
}

impl From<BatchError> for rmcp::ErrorData {
    fn from(error: BatchError) -> Self {
        let message = error.to_string();
        let problem = match error {
            BatchError::Empty { field } => json!({
                "code": "empty_batch",
                "message": message,
                "field": field,
            }),
            BatchError::TooLarge {
                field,
                maximum,
                received,
            } => json!({
                "code": "batch_too_large",
                "message": message,
                "field": field,
                "maximum": maximum,
                "received": received,
            }),
            BatchError::Duplicates { field, ids } => json!({
                "code": "duplicate_ids",
                "message": message,
                "field": field,
                "ids": ids,
            }),
            BatchError::Unknown { field, ids } => json!({
                "code": "unknown_ids",
                "message": message,
                "field": field,
                "ids": ids,
            }),
        };

        rmcp::ErrorData::invalid_params(message, Some(json!({ "problem": problem })))
    }
}

/// Builds the combined `batch_rejected` payload shared by every removal/detach
/// batch tool. Each `(key, ids)` bucket becomes an `id`-list field under
/// `problem`, always present (even when empty) so a single retry can fix every
/// failing id category at once.
pub(super) fn batch_rejected(
    field: &'static str,
    message: &'static str,
    buckets: Vec<(&'static str, Vec<Uuid>)>,
) -> rmcp::ErrorData {
    let mut problem = serde_json::Map::new();
    problem.insert("code".to_owned(), json!("batch_rejected"));
    problem.insert("message".to_owned(), json!(message));
    problem.insert("field".to_owned(), json!(field));
    for (key, ids) in buckets {
        problem.insert(key.to_owned(), json!(ids));
    }

    rmcp::ErrorData::invalid_params(message, Some(json!({ "problem": problem })))
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

impl From<ServiceError> for rmcp::ErrorData {
    fn from(error: ServiceError) -> Self {
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
        BatchError, FieldIssue, McpArgumentError,
    };
    use crate::domain::DomainError;
    use rmcp::model::ErrorCode;
    use uuid::Uuid;

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
        let [first, second] = [Uuid::new_v4(), Uuid::new_v4()];
        assert_eq!(
            FieldIssue::from(McpArgumentError::DuplicateIds {
                field: "framework_requirement_ids",
                ids: vec![first, second],
            })
            .message,
            format!("must not contain duplicate ids: {first}, {second}")
        );
    }

    #[test]
    fn required_timestamp_maps_missing_and_invalid_values() {
        let missing = required_timestamp("valid_from", None)
            .into_result()
            .map_err(argument_errors)
            .expect_err("missing timestamp");
        assert_eq!(
            field_issues(&missing),
            [("valid_from".to_owned(), "is required".to_owned())]
        );

        let invalid = required_timestamp("valid_from", Some("nope".to_owned()))
            .into_result()
            .map_err(argument_errors)
            .expect_err("invalid timestamp");
        assert_eq!(
            field_issues(&invalid),
            [(
                "valid_from".to_owned(),
                "must be an RFC 3339 timestamp".to_owned()
            )]
        );
    }

    #[test]
    fn domain_errors_map_to_validation_field_issues() {
        let error = domain_errors(vec![
            DomainError::EmptyRequiredText { field: "title" },
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
                ("title".to_owned(), "title must not be empty".to_owned()),
                (
                    "description".to_owned(),
                    "description must be at most 4000 characters".to_owned()
                ),
                (
                    "valid_until".to_owned(),
                    "valid_until must be greater than or equal to valid_from".to_owned()
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

    fn problem(error: &rmcp::ErrorData) -> &serde_json::Value {
        &error.data.as_ref().expect("error data")["problem"]
    }

    #[test]
    fn empty_batch_renders_its_own_problem_code() {
        let error = rmcp::ErrorData::from(BatchError::Empty {
            field: "control_ids",
        });

        assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
        assert_eq!(error.message, "control_ids must not be empty");
        let problem = problem(&error);
        assert_eq!(problem["code"], "empty_batch");
        assert_eq!(problem["field"], "control_ids");
    }

    #[test]
    fn oversized_batch_reports_the_limit_and_the_received_count() {
        let error = rmcp::ErrorData::from(BatchError::TooLarge {
            field: "control_ids",
            maximum: 50,
            received: 51,
        });

        let problem = problem(&error);
        assert_eq!(problem["code"], "batch_too_large");
        assert_eq!(problem["maximum"], 50);
        assert_eq!(problem["received"], 51);
    }

    #[test]
    fn duplicate_batch_ids_all_appear_in_the_response() {
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let error = rmcp::ErrorData::from(BatchError::Duplicates {
            field: "evidence_ids",
            ids: vec![first, second],
        });

        let problem = problem(&error);
        assert_eq!(problem["code"], "duplicate_ids");
        assert_eq!(problem["field"], "evidence_ids");
        assert_eq!(
            problem["ids"],
            serde_json::json!([first.to_string(), second.to_string()])
        );
    }

    #[test]
    fn unknown_batch_ids_all_appear_in_the_response() {
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let error = rmcp::ErrorData::from(BatchError::Unknown {
            field: "control_ids",
            ids: vec![first, second],
        });

        assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
        let problem = problem(&error);
        assert_eq!(problem["code"], "unknown_ids");
        assert_eq!(problem["field"], "control_ids");
        assert_eq!(
            problem["ids"],
            serde_json::json!([first.to_string(), second.to_string()])
        );
    }

    #[test]
    fn batch_rejected_surfaces_every_bucket_at_once() {
        let unknown = Uuid::new_v4();
        let archived = Uuid::new_v4();
        let error = super::batch_rejected(
            "policy_ids",
            "policy_ids contains unknown, archived, or not-mapped ids",
            vec![
                ("unknown_ids", vec![unknown]),
                ("archived_ids", vec![archived]),
                ("not_mapped_ids", vec![]),
            ],
        );

        assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
        let problem = problem(&error);
        assert_eq!(problem["code"], "batch_rejected");
        assert_eq!(problem["field"], "policy_ids");
        assert_eq!(
            problem["unknown_ids"],
            serde_json::json!([unknown.to_string()])
        );
        assert_eq!(
            problem["archived_ids"],
            serde_json::json!([archived.to_string()])
        );
        // Empty buckets are still present so an agent reads one stable shape.
        assert_eq!(problem["not_mapped_ids"], serde_json::json!([]));
    }
}

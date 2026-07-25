use serde_json::Value;
use url::Url;
use uuid::Uuid;

use crate::support::json::{assert_rfc3339, object_keys};

pub fn invite_token(invite_url: &Url) -> String {
    let query = invite_url
        .query_pairs()
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    let [(query_name, invite_token)] = query.as_slice() else {
        panic!("auditor URL has exactly one query parameter: {query:?}");
    };
    assert_eq!(query_name, "token");
    invite_token.clone()
}

#[track_caller]
pub fn assert_portal_read_audit_event(
    record: &Value,
    event_name: &str,
    operation: &str,
    workspace_id: Uuid,
    auditor_email: &str,
    request_id: Uuid,
) {
    assert_eq!(
        object_keys(record),
        ["fields", "level", "target", "timestamp"]
            .into_iter()
            .collect()
    );
    assert_eq!(record["level"], "INFO");
    assert_eq!(record["target"], "proofplane::audit");
    assert_rfc3339(&record["timestamp"]);

    let fields = &record["fields"];
    assert_eq!(
        object_keys(fields),
        [
            "actor_type",
            "client_type",
            "event_id",
            "event_name",
            "metadata",
            "object_id",
            "object_type",
            "operation",
            "outcome",
            "request_id",
            "system_name",
            "type",
            "workspace_id",
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(fields["type"], "audit_log");
    Uuid::parse_str(fields["event_id"].as_str().expect("event id is text"))
        .expect("event id is a UUID");
    assert_eq!(fields["event_name"], event_name);
    assert_eq!(fields["outcome"], "success");
    assert_eq!(fields["actor_type"], "system");
    assert_eq!(fields["system_name"], "auditor_browser");
    assert_eq!(fields["client_type"], "rest");
    assert_eq!(fields["operation"], operation);
    assert_eq!(fields["workspace_id"], workspace_id.to_string());
    assert_eq!(fields["request_id"], request_id.to_string());
    assert_eq!(
        fields["metadata"],
        format!(r#"{{"auditor_email":"{auditor_email}"}}"#)
    );
    assert_eq!(fields["object_type"], "auditor_access_session");
    Uuid::parse_str(fields["object_id"].as_str().expect("object id is text"))
        .expect("object id is a UUID");
}

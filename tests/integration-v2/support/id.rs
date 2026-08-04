use serde_json::Value;
use uuid::Uuid;

pub fn workspace_uuid(created: &Value) -> Uuid {
    Uuid::parse_str(created["id"].as_str().expect("workspace id is a string"))
        .expect("workspace id is a UUID")
}

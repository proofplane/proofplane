use std::collections::BTreeSet;

use serde_json::Value;

#[track_caller]
pub fn assert_rfc3339(value: &Value) {
    chrono::DateTime::parse_from_rfc3339(value.as_str().expect("timestamp is a string"))
        .expect("timestamp is RFC 3339");
}

pub fn object_keys(value: &Value) -> BTreeSet<&str> {
    value
        .as_object()
        .expect("value is an object")
        .keys()
        .map(String::as_str)
        .collect()
}

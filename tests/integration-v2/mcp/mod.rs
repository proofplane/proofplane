mod auditor_access_links;
mod authentication;
mod controls;
mod evidence;
mod evidence_control_mappings;
mod policy_catalog;
mod policy_control_mappings;
mod protocol;
mod tool_catalog;

use serde_json::{json, Value};

fn initialize_body() -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": { "name": "integration-v2", "version": "0" },
        },
    })
}

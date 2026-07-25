use proofplane::routes::authentication::AUTHORIZATION_HEADER;
use serde_json::Value;
use uuid::Uuid;

use crate::support::harness::TestApp;

pub async fn get_agent_connection_id_for(app: &TestApp, subject: &str, client_name: &str) -> Uuid {
    let connections = app
        .app_server()
        .get("/agent-connections")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {subject}"))
        .await;
    connections.assert_status_ok();

    Uuid::parse_str(
        connections.json::<Value>()["connections"]
            .as_array()
            .expect("connections is an array")
            .iter()
            .find(|connection| connection["client_name"] == client_name)
            .expect("agent connection is listed")["id"]
            .as_str()
            .expect("agent connection id is a string"),
    )
    .expect("agent connection id is a UUID")
}

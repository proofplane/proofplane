use axum::http::{header, StatusCode};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use url::Url;

use super::support::TestApp;

#[tokio::test]
async fn oauth_discovery_and_authorization_code_flow() {
    let app = TestApp::start().await;
    let workspace = app
        .server()
        .post("/workspaces")
        .add_header(header::AUTHORIZATION, "Bearer integration-human")
        .json(&json!({"name":"OAuth Workspace"}))
        .await;
    workspace.assert_status_ok();
    let workspace_id = workspace.json::<Value>()["id"].as_str().unwrap().to_owned();
    let metadata = app
        .server()
        .get("/.well-known/oauth-authorization-server")
        .await;
    metadata.assert_status_ok();
    let metadata: Value = metadata.json();
    assert_eq!(metadata["issuer"], "https://api.proofplane.test/");
    assert_eq!(
        metadata["code_challenge_methods_supported"],
        json!(["S256"])
    );

    let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let authorize = app.server().get(&format!(
        "/oauth/authorize?response_type=code&client_id=proofplane-local&redirect_uri=http%3A%2F%2F127.0.0.1%2Fcallback&resource=https%3A%2F%2Fmcp.proofplane.test%2Fmcp&scope=read_controls%20offline_access&state=test&code_challenge={challenge}&code_challenge_method=S256"
    )).await;
    authorize.assert_status(StatusCode::SEE_OTHER);
    let location = authorize
        .header(header::LOCATION)
        .to_str()
        .unwrap()
        .to_owned();
    let request_id = Url::parse(&location)
        .unwrap()
        .query_pairs()
        .find(|(key, _)| key == "request_id")
        .unwrap()
        .1
        .into_owned();

    let approve = app
        .server()
        .post(&format!("/oauth/requests/{request_id}/approve"))
        .add_header(header::AUTHORIZATION, "Bearer integration-human")
        .json(&json!({"workspace_id": workspace_id}))
        .await;
    approve.assert_status_ok();
    let callback = Url::parse(approve.json::<Value>()["redirect_uri"].as_str().unwrap()).unwrap();
    let code = callback
        .query_pairs()
        .find(|(key, _)| key == "code")
        .unwrap()
        .1
        .into_owned();

    let token = app
        .server()
        .post("/oauth/token")
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("client_id", "proofplane-local"),
            ("redirect_uri", "http://127.0.0.1/callback"),
            ("resource", "https://mcp.proofplane.test/mcp"),
            ("code_verifier", verifier),
        ])
        .await;
    token.assert_status_ok();
    let tokens: Value = token.json();
    assert!(tokens["access_token"]
        .as_str()
        .unwrap()
        .starts_with("v4.public."));
    assert!(tokens["refresh_token"]
        .as_str()
        .unwrap()
        .starts_with("v4.public."));
    let mcp = app.mcp_http_server();
    mcp.post("/mcp")
        .add_header(
            header::AUTHORIZATION,
            format!("Bearer {}", tokens["access_token"].as_str().unwrap()),
        )
        .add_header(header::ACCEPT, "application/json, text/event-stream")
        .json(&json!({
            "jsonrpc":"2.0","id":1,"method":"initialize",
            "params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}
        }))
        .await
        .assert_status_ok();

    app.server()
        .post("/oauth/token")
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("client_id", "proofplane-local"),
            ("redirect_uri", "http://127.0.0.1/callback"),
            ("resource", "https://mcp.proofplane.test/mcp"),
            ("code_verifier", verifier),
        ])
        .await
        .assert_status(StatusCode::BAD_REQUEST);

    let refresh = tokens["refresh_token"].as_str().unwrap();
    let rotated = app
        .server()
        .post("/oauth/token")
        .form(&[("grant_type", "refresh_token"), ("refresh_token", refresh)])
        .await;
    rotated.assert_status_ok();
    app.server()
        .post("/oauth/token")
        .form(&[("grant_type", "refresh_token"), ("refresh_token", refresh)])
        .await
        .assert_status(StatusCode::BAD_REQUEST);

    mcp.post("/mcp")
        .add_header(
            header::AUTHORIZATION,
            format!(
                "Bearer {}",
                rotated.json::<Value>()["access_token"].as_str().unwrap()
            ),
        )
        .add_header(header::ACCEPT, "application/json, text/event-stream")
        .json(&json!({
            "jsonrpc":"2.0","id":1,"method":"initialize",
            "params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}
        }))
        .await
        .assert_status(StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn oauth_rejects_unknown_clients_and_redirect_mismatches() {
    let app = TestApp::start().await;
    for query in [
        "client_id=unknown&redirect_uri=http%3A%2F%2F127.0.0.1%2Fcallback",
        "client_id=proofplane-local&redirect_uri=http%3A%2F%2Fevil.example%2Fcallback",
    ] {
        app.server().get(&format!(
            "/oauth/authorize?response_type=code&{query}&resource=https%3A%2F%2Fmcp.proofplane.test%2Fmcp&scope=read_controls&code_challenge=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA&code_challenge_method=S256"
        )).await.assert_status(StatusCode::BAD_REQUEST);
    }
}

use proofplane::{authorization::spicedb::WorkspacePermission, domain::WorkspaceId};
use uuid::Uuid;

use super::support::TestApp;

#[tokio::test]
async fn schema_and_workspace_membership_are_idempotent_and_check_permissions() {
    let app = TestApp::start().await;
    let client = app.spicedb();
    let schema = include_str!("../../authz/spicedb/proofplane.zed");
    let workspace_id = WorkspaceId::from(
        Uuid::parse_str("00000000-0000-4000-8000-000000000001").expect("workspace id parses"),
    );

    let schema_write = client.write_schema(schema).await;
    assert!(
        schema_write.is_ok(),
        "schema deploy should succeed: {schema_write:?}"
    );

    let repeated_schema_write = client.write_schema(schema).await;
    assert!(
        repeated_schema_write.is_ok(),
        "same schema redeploy should succeed: {repeated_schema_write:?}"
    );

    let membership_write = client
        .write_workspace_membership(workspace_id, "system-actor")
        .await;
    assert!(
        membership_write.is_ok(),
        "membership write should succeed: {membership_write:?}"
    );

    let repeated_membership_write = client
        .write_workspace_membership(workspace_id, "system-actor")
        .await;
    assert!(
        repeated_membership_write.is_ok(),
        "membership rewrite should succeed: {repeated_membership_write:?}"
    );

    let member_read = client
        .check_workspace_permission(
            workspace_id,
            WorkspacePermission::ReadEvidenceRequests,
            "system-actor",
        )
        .await;
    assert!(
        matches!(member_read, Ok(true)),
        "member should be allowed to read evidence requests: {member_read:?}"
    );

    let member_write = client
        .check_workspace_permission(
            workspace_id,
            WorkspacePermission::WriteEvidenceRequests,
            "system-actor",
        )
        .await;
    assert!(
        matches!(member_write, Ok(true)),
        "member should be allowed to write evidence requests: {member_write:?}"
    );

    let non_member_read = client
        .check_workspace_permission(
            workspace_id,
            WorkspacePermission::ReadEvidenceRequests,
            "outside-actor",
        )
        .await;
    assert!(
        matches!(non_member_read, Ok(false)),
        "non-member should not be allowed to read evidence requests: {non_member_read:?}"
    );

    let non_member_write = client
        .check_workspace_permission(
            workspace_id,
            WorkspacePermission::WriteEvidenceRequests,
            "outside-actor",
        )
        .await;
    assert!(
        matches!(non_member_write, Ok(false)),
        "non-member should not be allowed to write evidence requests: {non_member_write:?}"
    );
}

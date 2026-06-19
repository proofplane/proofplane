use chrono::{DateTime, Utc};

use crate::authentication::opaque_token::ApiTokenDigest;

use super::{ids::uuid_id, DomainError, UserId, WorkspaceId, WorkspacePermission};

uuid_id!(ApiTokenId);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiToken {
    pub id: ApiTokenId,
    pub user_id: UserId,
    pub workspace_id: WorkspaceId,
    pub name: String,
    pub expires_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiTokenWithPermissions {
    pub token: ApiToken,
    pub permissions: Vec<WorkspacePermission>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateApiTokenPayload {
    pub id: ApiTokenId,
    pub token_digest: ApiTokenDigest,
    pub user_id: UserId,
    pub workspace_id: WorkspaceId,
    pub name: String,
    pub expires_at: DateTime<Utc>,
    pub permissions: Vec<WorkspacePermission>,
}

pub fn canonical_permissions(
    values: Vec<WorkspacePermission>,
) -> Result<Vec<WorkspacePermission>, DomainError> {
    let mut seen = [false; 6];
    for permission in values {
        let index = permission_index(permission);
        if seen[index] {
            return Err(DomainError::DuplicatePermission {
                permission: permission.as_str().to_owned(),
            });
        }
        seen[index] = true;
    }

    Ok(WorkspacePermission::ALL
        .into_iter()
        .filter(|permission| seen[permission_index(*permission)])
        .collect())
}

fn permission_index(permission: WorkspacePermission) -> usize {
    match permission {
        WorkspacePermission::ReadEvidenceRequests => 0,
        WorkspacePermission::WriteEvidenceRequests => 1,
        WorkspacePermission::ReadEvidenceSubmissions => 2,
        WorkspacePermission::WriteEvidenceSubmissions => 3,
        WorkspacePermission::ReadControls => 4,
        WorkspacePermission::WriteControls => 5,
    }
}

#[cfg(test)]
mod tests {
    use super::canonical_permissions;
    use crate::domain::{DomainError, WorkspacePermission};

    #[test]
    fn canonical_permissions_reject_duplicates() {
        assert_eq!(
            canonical_permissions(vec![
                WorkspacePermission::ReadControls,
                WorkspacePermission::ReadControls,
            ]),
            Err(DomainError::DuplicatePermission {
                permission: "read_controls".to_owned(),
            })
        );
    }

    #[test]
    fn canonical_permissions_preserve_workspace_permission_order() {
        assert_eq!(
            canonical_permissions(vec![
                WorkspacePermission::WriteControls,
                WorkspacePermission::ReadEvidenceRequests,
                WorkspacePermission::ReadControls,
            ])
            .unwrap(),
            vec![
                WorkspacePermission::ReadEvidenceRequests,
                WorkspacePermission::ReadControls,
                WorkspacePermission::WriteControls,
            ]
        );
    }
}

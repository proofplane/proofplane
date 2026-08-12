use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{
    domain::{UserId, WorkspaceId, WorkspaceInvitationId, WorkspaceRole},
    persistence::Error,
    read_models::{
        PendingWorkspaceInvitation, WorkspaceInvitationPreviewSource, WorkspacePeople,
        WorkspacePerson,
    },
};

use super::ReadExecutor;

pub(crate) struct WorkspacePeopleReads<'a, E> {
    executor: &'a E,
}
impl<'a, E> WorkspacePeopleReads<'a, E> {
    pub(crate) fn new(executor: &'a E) -> Self {
        Self { executor }
    }
}

impl<E: ReadExecutor> WorkspacePeopleReads<'_, E> {
    pub async fn member_id_by_email(
        &self,
        workspace_id: WorkspaceId,
        email: &str,
    ) -> Result<Option<UserId>, Error> {
        self.executor.query_opt(
            "SELECT m.user_id FROM workspace_memberships m JOIN users u ON u.id = m.user_id WHERE m.workspace_id = $1 AND lower(trim(u.email)) = $2",
            &[&Uuid::from(workspace_id), &email],
        ).await?.map(|row| row.try_get::<_, Uuid>("user_id").map(UserId::from)).transpose().map_err(Into::into)
    }

    pub async fn get(
        &self,
        actor_user_id: UserId,
        now: DateTime<Utc>,
    ) -> Result<Option<WorkspacePeople>, Error> {
        let Some(workspace) = self.executor.query_opt(
            "SELECT w.id, w.name, m.role FROM workspace_memberships m JOIN workspaces w ON w.id = m.workspace_id WHERE m.user_id = $1",
            &[&Uuid::from(actor_user_id)],
        ).await? else { return Ok(None); };
        let workspace_id = WorkspaceId::from(workspace.try_get::<_, Uuid>("id")?);
        let members = self.executor.query(
            "SELECT m.user_id, u.name, u.email, m.role, m.created_at FROM workspace_memberships m JOIN users u ON u.id = m.user_id WHERE m.workspace_id = $1 ORDER BY m.created_at, m.user_id",
            &[&Uuid::from(workspace_id)],
        ).await?.into_iter().map(|row| Ok(WorkspacePerson {
            user_id: row.try_get::<_, Uuid>("user_id")?.into(),
            display_name: row.try_get("name")?, email: row.try_get("email")?,
            role: parse_role(row.try_get("role")?)?, joined_at: row.try_get("created_at")?,
        })).collect::<Result<Vec<_>, Error>>()?;
        let pending_invitations = self.executor.query(
            "SELECT id, invited_email, role, generation, expires_at, queued_generation, queued_at, delivered_generation, delivered_at, last_delivery_failure, delivery_failed_at FROM workspace_invitations WHERE workspace_id = $1 AND accepted_at IS NULL AND revoked_at IS NULL AND expires_at > $2 ORDER BY created_at, id",
            &[&Uuid::from(workspace_id), &now],
        ).await?.into_iter().map(|row| Ok(PendingWorkspaceInvitation {
            id: WorkspaceInvitationId::from(row.try_get::<_, Uuid>("id")?), invited_email: row.try_get("invited_email")?,
            role: parse_role(row.try_get("role")?)?, generation: row.try_get("generation")?, expires_at: row.try_get("expires_at")?,
            queued_generation: row.try_get("queued_generation")?, queued_at: row.try_get("queued_at")?,
            delivered_generation: row.try_get("delivered_generation")?, delivered_at: row.try_get("delivered_at")?,
            last_delivery_failure: row.try_get("last_delivery_failure")?, delivery_failed_at: row.try_get("delivery_failed_at")?,
        })).collect::<Result<Vec<_>, Error>>()?;
        Ok(Some(WorkspacePeople {
            workspace_id,
            workspace_name: workspace.try_get("name")?,
            actor_role: parse_role(workspace.try_get("role")?)?,
            members,
            pending_invitations,
        }))
    }

    pub async fn invitation_preview_source(
        &self,
        id: WorkspaceInvitationId,
    ) -> Result<Option<WorkspaceInvitationPreviewSource>, Error> {
        self.executor.query_opt(
            "SELECT i.id, i.workspace_id, w.name AS workspace_name, i.invited_email, i.role, i.generation, i.expires_at, i.accepted_at, i.revoked_at FROM workspace_invitations i JOIN workspaces w ON w.id = i.workspace_id WHERE i.id = $1",
            &[&Uuid::from(id)],
        ).await?.map(|row| Ok(WorkspaceInvitationPreviewSource {
            invitation_id: row.try_get::<_, Uuid>("id")?.into(), workspace_id: row.try_get::<_, Uuid>("workspace_id")?.into(),
            workspace_name: row.try_get("workspace_name")?, invited_email: row.try_get("invited_email")?,
            role: parse_role(row.try_get("role")?)?, generation: row.try_get("generation")?, expires_at: row.try_get("expires_at")?,
            accepted_at: row.try_get("accepted_at")?, revoked_at: row.try_get("revoked_at")?,
        })).transpose()
    }
}

fn parse_role(value: String) -> Result<WorkspaceRole, Error> {
    value
        .parse()
        .map_err(|_| Error::InvariantViolation("unknown workspace people role"))
}

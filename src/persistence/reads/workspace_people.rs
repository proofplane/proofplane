use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{
    domain::{
        UserId, WorkspaceId, WorkspaceInvitationDeliveryFailure, WorkspaceInvitationDeliveryState,
        WorkspaceInvitationId, WorkspaceRole,
    },
    persistence::Error,
    read_models::{
        CurrentWorkspaceInvitation, PendingWorkspaceInvitation, WorkspaceInvitationPreviewSource,
        WorkspacePeople, WorkspacePerson,
    },
};

use super::{param, ReadExecutor, TransactionalReadExecutor};

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
            &[param(&Uuid::from(workspace_id)), param(&email)],
        ).await?.map(|row| row.try_get::<_, Uuid>("user_id").map(UserId::from)).transpose().map_err(Into::into)
    }

    pub async fn get(
        &self,
        workspace_id: WorkspaceId,
        workspace_name: String,
        actor_role: WorkspaceRole,
        now: DateTime<Utc>,
    ) -> Result<WorkspacePeople, Error> {
        let members = self.executor.query(
            "SELECT m.user_id, u.name, u.email, m.role, m.created_at FROM workspace_memberships m JOIN users u ON u.id = m.user_id WHERE m.workspace_id = $1 ORDER BY m.created_at, m.user_id",
            &[param(&Uuid::from(workspace_id))],
        ).await?.into_iter().map(|row| Ok(WorkspacePerson {
            user_id: row.try_get::<_, Uuid>("user_id")?.into(),
            display_name: row.try_get("name")?, email: row.try_get("email")?,
            role: parse_role(row.try_get("role")?)?, joined_at: row.try_get("created_at")?,
        })).collect::<Result<Vec<_>, Error>>()?;
        let pending_invitations = self.executor.query(
            "SELECT id, invited_email, role, generation, expires_at, queued_generation, queued_at, delivered_generation, delivered_at, last_delivery_failure, delivery_failed_at FROM workspace_invitations WHERE workspace_id = $1 AND accepted_at IS NULL AND revoked_at IS NULL AND expires_at > $2 ORDER BY created_at, id",
            &[param(&Uuid::from(workspace_id)), param(&now)],
        ).await?.into_iter().map(pending_invitation_from_row).collect::<Result<Vec<_>, Error>>()?;
        Ok(WorkspacePeople {
            workspace_id,
            workspace_name,
            actor_role,
            members,
            pending_invitations,
        })
    }

    pub async fn current_for_workspace(
        &self,
        id: WorkspaceInvitationId,
        workspace_id: WorkspaceId,
        now: DateTime<Utc>,
    ) -> Result<Option<CurrentWorkspaceInvitation>, Error> {
        self.executor
            .query_opt(
                &format!(
                    "SELECT {CURRENT_COLUMNS} FROM workspace_invitations WHERE id = $1 AND workspace_id = $2 AND accepted_at IS NULL AND revoked_at IS NULL AND expires_at > $3"
                ),
                &[param(&Uuid::from(id)), param(&Uuid::from(workspace_id)), param(&now)],
            )
            .await?
            .map(current_invitation_from_row)
            .transpose()
    }

    pub async fn invitation_preview_source(
        &self,
        id: WorkspaceInvitationId,
    ) -> Result<Option<WorkspaceInvitationPreviewSource>, Error> {
        self.executor.query_opt(
            "SELECT i.id, i.workspace_id, w.name AS workspace_name, i.invited_email, i.role, i.generation, i.expires_at, i.accepted_at, i.revoked_at FROM workspace_invitations i JOIN workspaces w ON w.id = i.workspace_id WHERE i.id = $1",
            &[param(&Uuid::from(id))],
        ).await?.map(|row| Ok(WorkspaceInvitationPreviewSource {
            invitation_id: row.try_get::<_, Uuid>("id")?.into(), workspace_id: row.try_get::<_, Uuid>("workspace_id")?.into(),
            workspace_name: row.try_get("workspace_name")?, invited_email: row.try_get("invited_email")?,
            role: parse_role(row.try_get("role")?)?, generation: row.try_get("generation")?, expires_at: row.try_get("expires_at")?,
            accepted_at: row.try_get("accepted_at")?, revoked_at: row.try_get("revoked_at")?,
        })).transpose()
    }
}

impl WorkspacePeopleReads<'_, TransactionalReadExecutor<'_>> {
    pub async fn lock_pending_for_email(
        &self,
        workspace_id: WorkspaceId,
        email: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<CurrentWorkspaceInvitation>, Error> {
        let invitation_key = format!("{}:{email}", Uuid::from(workspace_id));
        self.executor
            .query_one(
                "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
                &[param(&invitation_key)],
            )
            .await?;
        self.executor
            .query_opt(
                &format!(
                    "SELECT {CURRENT_COLUMNS} FROM workspace_invitations WHERE workspace_id = $1 AND invited_email = $2 AND accepted_at IS NULL AND revoked_at IS NULL AND expires_at > $3 ORDER BY created_at DESC LIMIT 1"
                ),
                &[param(&Uuid::from(workspace_id)), param(&email), param(&now)],
            )
            .await?
            .map(current_invitation_from_row)
            .transpose()
    }
}

const CURRENT_COLUMNS: &str = "id, workspace_id, invited_email, role, generation, expires_at, queued_generation, delivered_generation, last_delivery_failure";

fn current_invitation_from_row(
    row: tokio_postgres::Row,
) -> Result<CurrentWorkspaceInvitation, Error> {
    let generation = row.try_get("generation")?;
    let last_delivery_failure = parse_delivery_failure(row.try_get("last_delivery_failure")?)?;
    Ok(CurrentWorkspaceInvitation {
        id: row.try_get::<_, Uuid>("id")?.into(),
        workspace_id: row.try_get::<_, Uuid>("workspace_id")?.into(),
        invited_email: row.try_get("invited_email")?,
        role: parse_role(row.try_get("role")?)?,
        generation,
        expires_at: row.try_get("expires_at")?,
        delivery_state: WorkspaceInvitationDeliveryState::from_snapshot(
            generation,
            row.try_get("queued_generation")?,
            row.try_get("delivered_generation")?,
            last_delivery_failure,
        ),
    })
}

fn pending_invitation_from_row(
    row: tokio_postgres::Row,
) -> Result<PendingWorkspaceInvitation, Error> {
    let generation = row.try_get("generation")?;
    let last_delivery_failure = parse_delivery_failure(row.try_get("last_delivery_failure")?)?;
    Ok(PendingWorkspaceInvitation {
        id: WorkspaceInvitationId::from(row.try_get::<_, Uuid>("id")?),
        invited_email: row.try_get("invited_email")?,
        role: parse_role(row.try_get("role")?)?,
        generation,
        expires_at: row.try_get("expires_at")?,
        delivery_state: WorkspaceInvitationDeliveryState::from_snapshot(
            generation,
            row.try_get("queued_generation")?,
            row.try_get("delivered_generation")?,
            last_delivery_failure,
        ),
        queued_at: row.try_get("queued_at")?,
        delivered_at: row.try_get("delivered_at")?,
        delivery_failed_at: row.try_get("delivery_failed_at")?,
    })
}

fn parse_delivery_failure(
    value: Option<String>,
) -> Result<Option<WorkspaceInvitationDeliveryFailure>, Error> {
    value
        .map(|value| value.parse())
        .transpose()
        .map_err(|_| Error::InvariantViolation("unknown invitation delivery failure"))
}

fn parse_role(value: String) -> Result<WorkspaceRole, Error> {
    value
        .parse()
        .map_err(|_| Error::InvariantViolation("unknown workspace people role"))
}

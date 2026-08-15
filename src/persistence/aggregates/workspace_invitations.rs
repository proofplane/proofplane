use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::domain::{
    UserId, WorkspaceId, WorkspaceInvitation, WorkspaceInvitationDeliveryFailure,
    WorkspaceInvitationId, WorkspaceRole,
};

use super::{
    snapshot::{save_snapshot, snapshot_record},
    Error, UnitOfWork,
};

pub struct WorkspaceInvitationRepository<'a> {
    unit_of_work: &'a UnitOfWork<'a>,
}

impl<'a> UnitOfWork<'a> {
    pub fn workspace_invitations(&'a self) -> WorkspaceInvitationRepository<'a> {
        WorkspaceInvitationRepository { unit_of_work: self }
    }
}

impl WorkspaceInvitationRepository<'_> {
    pub async fn get(
        &self,
        id: WorkspaceInvitationId,
    ) -> Result<Option<WorkspaceInvitation>, Error> {
        self.unit_of_work
            .transaction
            .query_opt(
                &format!("SELECT {COLUMNS} FROM workspace_invitations WHERE id = $1 FOR UPDATE"),
                &[&Uuid::from(id)],
            )
            .await?
            .map(|row| WorkspaceInvitationRecord::try_from_row(&row)?.into_domain())
            .transpose()
    }

    pub async fn get_for_workspace(
        &self,
        id: WorkspaceInvitationId,
        workspace_id: WorkspaceId,
    ) -> Result<Option<WorkspaceInvitation>, Error> {
        self.unit_of_work.transaction.query_opt(
            &format!("SELECT {COLUMNS} FROM workspace_invitations WHERE id = $1 AND workspace_id = $2 FOR UPDATE"),
            &[&Uuid::from(id), &Uuid::from(workspace_id)],
        ).await?.map(|row| WorkspaceInvitationRecord::try_from_row(&row)?.into_domain()).transpose()
    }

    pub async fn save(&self, invitation: &WorkspaceInvitation) -> Result<(), Error> {
        let record = WorkspaceInvitationRecord::from_domain(invitation);
        save_snapshot(&self.unit_of_work.transaction, record.as_snapshot()).await
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};
    use uuid::Uuid;

    use crate::{domain::WorkspaceInvitation, persistence::test_support};

    #[tokio::test]
    async fn complete_snapshot_round_trips_and_acceptance_replaces_it() {
        let database = test_support::database().await;
        let fixture = test_support::workspace(&database.postgres, "Invitation Snapshot").await;
        let now = Utc::now();
        let invitation = WorkspaceInvitation::create(
            Uuid::new_v4().into(),
            fixture.workspace_id,
            fixture.user_id,
            "admin@example.com".to_owned(),
            now,
            now + Duration::days(7),
        )
        .unwrap();
        let expected = invitation.clone();

        database
            .postgres
            .in_unit_of_work(async |unit_of_work| {
                unit_of_work.workspace_invitations().save(&invitation).await
            })
            .await
            .unwrap();
        let loaded = database
            .postgres
            .in_unit_of_work(async |unit_of_work| {
                unit_of_work
                    .workspace_invitations()
                    .get(expected.id())
                    .await
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(loaded, expected);

        let mut accepted = loaded;
        accepted
            .accept(fixture.user_id, now + Duration::minutes(1))
            .unwrap();
        let expected = accepted.clone();
        database
            .postgres
            .in_unit_of_work(async |unit_of_work| {
                unit_of_work.workspace_invitations().save(&accepted).await
            })
            .await
            .unwrap();
        let loaded = database
            .postgres
            .in_unit_of_work(async |unit_of_work| {
                unit_of_work
                    .workspace_invitations()
                    .get(expected.id())
                    .await
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(loaded, expected);
    }
}

const COLUMNS: &str = "id, workspace_id, inviter_user_id, invited_email, role, generation, created_at, expires_at, accepted_at, revoked_at, accepting_user_id, queued_generation, queued_at, delivered_generation, delivered_at, last_delivery_failure, delivery_failed_at";

snapshot_record! {
    struct WorkspaceInvitationRecord {
        id: Uuid,
        workspace_id: Uuid,
        inviter_user_id: Uuid,
        invited_email: String,
        role: String,
        generation: i64,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
        accepted_at: Option<DateTime<Utc>>,
        revoked_at: Option<DateTime<Utc>>,
        accepting_user_id: Option<Uuid>,
        queued_generation: Option<i64>,
        queued_at: Option<DateTime<Utc>>,
        delivered_generation: Option<i64>,
        delivered_at: Option<DateTime<Utc>>,
        last_delivery_failure: Option<String>,
        delivery_failed_at: Option<DateTime<Utc>>,
    }
    table: workspace_invitations,
    conflict: id,
}

impl WorkspaceInvitationRecord {
    fn try_from_row(row: &Row) -> Result<Self, Error> {
        Ok(Self {
            id: row.try_get("id")?,
            workspace_id: row.try_get("workspace_id")?,
            inviter_user_id: row.try_get("inviter_user_id")?,
            invited_email: row.try_get("invited_email")?,
            role: row.try_get("role")?,
            generation: row.try_get("generation")?,
            created_at: row.try_get("created_at")?,
            expires_at: row.try_get("expires_at")?,
            accepted_at: row.try_get("accepted_at")?,
            revoked_at: row.try_get("revoked_at")?,
            accepting_user_id: row.try_get("accepting_user_id")?,
            queued_generation: row.try_get("queued_generation")?,
            queued_at: row.try_get("queued_at")?,
            delivered_generation: row.try_get("delivered_generation")?,
            delivered_at: row.try_get("delivered_at")?,
            last_delivery_failure: row.try_get("last_delivery_failure")?,
            delivery_failed_at: row.try_get("delivery_failed_at")?,
        })
    }

    fn from_domain(value: &WorkspaceInvitation) -> Self {
        Self {
            id: value.id().into(),
            workspace_id: value.workspace_id().into(),
            inviter_user_id: value.inviter_user_id().into(),
            invited_email: value.invited_email().to_owned(),
            role: value.role().as_str().to_owned(),
            generation: value.generation(),
            created_at: value.created_at(),
            expires_at: value.expires_at(),
            accepted_at: value.accepted_at(),
            revoked_at: value.revoked_at(),
            accepting_user_id: value.accepting_user_id().map(Into::into),
            queued_generation: value.queued_generation(),
            queued_at: value.queued_at(),
            delivered_generation: value.delivered_generation(),
            delivered_at: value.delivered_at(),
            last_delivery_failure: value
                .last_delivery_failure()
                .map(|failure| failure.as_str().to_owned()),
            delivery_failed_at: value.delivery_failed_at(),
        }
    }

    fn into_domain(self) -> Result<WorkspaceInvitation, Error> {
        let role = self
            .role
            .parse::<WorkspaceRole>()
            .map_err(|_| Error::InvariantViolation("unknown invitation role"))?;
        let last_delivery_failure = self
            .last_delivery_failure
            .map(|failure| failure.parse::<WorkspaceInvitationDeliveryFailure>())
            .transpose()
            .map_err(|_| Error::InvariantViolation("unknown invitation delivery failure"))?;
        WorkspaceInvitation::rehydrate(
            self.id.into(),
            self.workspace_id.into(),
            self.inviter_user_id.into(),
            self.invited_email,
            role,
            self.generation,
            self.created_at,
            self.expires_at,
            self.accepted_at,
            self.revoked_at,
            self.accepting_user_id.map(UserId::from),
            self.queued_generation,
            self.queued_at,
            self.delivered_generation,
            self.delivered_at,
            last_delivery_failure,
            self.delivery_failed_at,
        )
        .map_err(|_| Error::InvariantViolation("persisted workspace invitation is inconsistent"))
    }
}

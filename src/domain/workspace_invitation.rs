use chrono::{DateTime, Timelike, Utc};

use super::{ids::uuid_id, UserId, WorkspaceId, WorkspaceRole};

uuid_id!(WorkspaceInvitationId);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceInvitation {
    id: WorkspaceInvitationId,
    workspace_id: WorkspaceId,
    inviter_user_id: UserId,
    invited_email: String,
    role: WorkspaceRole,
    generation: i64,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    accepted_at: Option<DateTime<Utc>>,
    revoked_at: Option<DateTime<Utc>>,
    accepting_user_id: Option<UserId>,
    queued_generation: Option<i64>,
    queued_at: Option<DateTime<Utc>>,
    delivered_generation: Option<i64>,
    delivered_at: Option<DateTime<Utc>>,
    last_delivery_failure: Option<String>,
    delivery_failed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceInvitationStatus {
    Pending,
    Accepted,
    Revoked,
    Expired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvitationAcceptance {
    Applied,
    Replay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum WorkspaceInvitationError {
    #[error("workspace invitation snapshot is inconsistent")]
    InvalidSnapshot,
    #[error("workspace invitation is unavailable")]
    Unavailable,
    #[error("workspace invitation was accepted by another user")]
    AcceptedByAnotherUser,
    #[error("workspace invitation generation is stale")]
    StaleGeneration,
}

impl WorkspaceInvitation {
    pub fn create(
        id: WorkspaceInvitationId,
        workspace_id: WorkspaceId,
        inviter_user_id: UserId,
        invited_email: String,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Result<Self, WorkspaceInvitationError> {
        let created_at = created_at
            .with_nanosecond(0)
            .ok_or(WorkspaceInvitationError::InvalidSnapshot)?;
        let expires_at = expires_at
            .with_nanosecond(0)
            .ok_or(WorkspaceInvitationError::InvalidSnapshot)?;
        Self::rehydrate(
            id,
            workspace_id,
            inviter_user_id,
            invited_email,
            WorkspaceRole::Admin,
            1,
            created_at,
            expires_at,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn rehydrate(
        id: WorkspaceInvitationId,
        workspace_id: WorkspaceId,
        inviter_user_id: UserId,
        invited_email: String,
        role: WorkspaceRole,
        generation: i64,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
        accepted_at: Option<DateTime<Utc>>,
        revoked_at: Option<DateTime<Utc>>,
        accepting_user_id: Option<UserId>,
        queued_generation: Option<i64>,
        queued_at: Option<DateTime<Utc>>,
        delivered_generation: Option<i64>,
        delivered_at: Option<DateTime<Utc>>,
        last_delivery_failure: Option<String>,
        delivery_failed_at: Option<DateTime<Utc>>,
    ) -> Result<Self, WorkspaceInvitationError> {
        let terminal_valid = matches!(
            (accepted_at, revoked_at, accepting_user_id),
            (Some(_), None, Some(_)) | (None, Some(_), None) | (None, None, None)
        );
        let delivery_valid = queued_generation.is_none() == queued_at.is_none()
            && delivered_generation.is_none() == delivered_at.is_none()
            && last_delivery_failure.is_none() == delivery_failed_at.is_none()
            && queued_generation.is_none_or(|value| value > 0 && value <= generation)
            && delivered_generation.is_none_or(|value| value > 0 && value <= generation);
        if role != WorkspaceRole::Admin
            || generation <= 0
            || invited_email.trim().is_empty()
            || expires_at <= created_at
            || !terminal_valid
            || !delivery_valid
        {
            return Err(WorkspaceInvitationError::InvalidSnapshot);
        }
        Ok(Self {
            id,
            workspace_id,
            inviter_user_id,
            invited_email,
            role,
            generation,
            created_at,
            expires_at,
            accepted_at,
            revoked_at,
            accepting_user_id,
            queued_generation,
            queued_at,
            delivered_generation,
            delivered_at,
            last_delivery_failure,
            delivery_failed_at,
        })
    }

    pub fn status_at(&self, now: DateTime<Utc>) -> WorkspaceInvitationStatus {
        if self.accepted_at.is_some() {
            WorkspaceInvitationStatus::Accepted
        } else if self.revoked_at.is_some() {
            WorkspaceInvitationStatus::Revoked
        } else if now >= self.expires_at {
            WorkspaceInvitationStatus::Expired
        } else {
            WorkspaceInvitationStatus::Pending
        }
    }

    pub fn ensure_current(
        &self,
        generation: i64,
        expires_at: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<(), WorkspaceInvitationError> {
        (self.generation == generation
            && self.expires_at == expires_at
            && self.status_at(now) == WorkspaceInvitationStatus::Pending)
            .then_some(())
            .ok_or(WorkspaceInvitationError::Unavailable)
    }

    pub fn accept(
        &mut self,
        user_id: UserId,
        accepted_at: DateTime<Utc>,
    ) -> Result<InvitationAcceptance, WorkspaceInvitationError> {
        if self.accepting_user_id == Some(user_id) && self.accepted_at.is_some() {
            return Ok(InvitationAcceptance::Replay);
        }
        if self.accepted_at.is_some() {
            return Err(WorkspaceInvitationError::AcceptedByAnotherUser);
        }
        if self.status_at(accepted_at) != WorkspaceInvitationStatus::Pending {
            return Err(WorkspaceInvitationError::Unavailable);
        }
        self.accepted_at = Some(accepted_at);
        self.accepting_user_id = Some(user_id);
        Ok(InvitationAcceptance::Applied)
    }

    pub fn queue_delivery(
        &mut self,
        generation: i64,
        queued_at: DateTime<Utc>,
    ) -> Result<(), WorkspaceInvitationError> {
        if generation != self.generation {
            return Err(WorkspaceInvitationError::StaleGeneration);
        }
        if self.status_at(queued_at) != WorkspaceInvitationStatus::Pending {
            return Err(WorkspaceInvitationError::Unavailable);
        }
        self.queued_generation = Some(generation);
        self.queued_at = Some(queued_at);
        self.last_delivery_failure = None;
        self.delivery_failed_at = None;
        Ok(())
    }

    pub fn resend(
        &mut self,
        expected_generation: i64,
        sent_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Result<(), WorkspaceInvitationError> {
        if expected_generation != self.generation {
            return Err(WorkspaceInvitationError::StaleGeneration);
        }
        if self.status_at(sent_at) != WorkspaceInvitationStatus::Pending || expires_at <= sent_at {
            return Err(WorkspaceInvitationError::Unavailable);
        }
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or(WorkspaceInvitationError::InvalidSnapshot)?;
        self.expires_at = expires_at;
        self.queued_generation = Some(self.generation);
        self.queued_at = Some(sent_at);
        self.last_delivery_failure = None;
        self.delivery_failed_at = None;
        Ok(())
    }

    pub fn record_delivery_success(
        &mut self,
        generation: i64,
        delivered_at: DateTime<Utc>,
    ) -> bool {
        if generation != self.generation
            || self.status_at(delivered_at) != WorkspaceInvitationStatus::Pending
        {
            return false;
        }
        self.delivered_generation = Some(generation);
        self.delivered_at = Some(delivered_at);
        self.last_delivery_failure = None;
        self.delivery_failed_at = None;
        true
    }

    pub fn record_delivery_failure(
        &mut self,
        generation: i64,
        classification: &'static str,
        failed_at: DateTime<Utc>,
    ) -> bool {
        if generation != self.generation
            || self.status_at(failed_at) != WorkspaceInvitationStatus::Pending
        {
            return false;
        }
        self.last_delivery_failure = Some(classification.to_owned());
        self.delivery_failed_at = Some(failed_at);
        true
    }

    pub fn id(&self) -> WorkspaceInvitationId {
        self.id
    }
    pub fn workspace_id(&self) -> WorkspaceId {
        self.workspace_id
    }
    pub fn inviter_user_id(&self) -> UserId {
        self.inviter_user_id
    }
    pub fn invited_email(&self) -> &str {
        &self.invited_email
    }
    pub fn role(&self) -> WorkspaceRole {
        self.role
    }
    pub fn generation(&self) -> i64 {
        self.generation
    }
    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    pub fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }
    pub fn accepted_at(&self) -> Option<DateTime<Utc>> {
        self.accepted_at
    }
    pub fn revoked_at(&self) -> Option<DateTime<Utc>> {
        self.revoked_at
    }
    pub fn accepting_user_id(&self) -> Option<UserId> {
        self.accepting_user_id
    }
    pub fn queued_generation(&self) -> Option<i64> {
        self.queued_generation
    }
    pub fn queued_at(&self) -> Option<DateTime<Utc>> {
        self.queued_at
    }
    pub fn delivered_generation(&self) -> Option<i64> {
        self.delivered_generation
    }
    pub fn delivered_at(&self) -> Option<DateTime<Utc>> {
        self.delivered_at
    }
    pub fn last_delivery_failure(&self) -> Option<&str> {
        self.last_delivery_failure.as_deref()
    }
    pub fn delivery_failed_at(&self) -> Option<DateTime<Utc>> {
        self.delivery_failed_at
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone};
    use uuid::Uuid;

    use super::*;

    fn pending() -> WorkspaceInvitation {
        let now = Utc
            .with_ymd_and_hms(2026, 8, 12, 12, 0, 0)
            .single()
            .unwrap();
        WorkspaceInvitation::create(
            Uuid::new_v4().into(),
            Uuid::new_v4().into(),
            Uuid::new_v4().into(),
            "admin@example.com".to_owned(),
            now,
            now + Duration::days(7),
        )
        .unwrap()
    }

    #[test]
    fn lifecycle_is_pending_then_expired_without_mutation() {
        let invitation = pending();
        assert_eq!(
            invitation.status_at(invitation.created_at()),
            WorkspaceInvitationStatus::Pending
        );
        assert_eq!(
            invitation.status_at(invitation.expires_at()),
            WorkspaceInvitationStatus::Expired
        );
    }

    #[test]
    fn acceptance_is_terminal_and_same_user_replays() {
        let mut invitation = pending();
        let user_id = UserId::from(Uuid::new_v4());
        let accepted_at = invitation.created_at() + Duration::hours(1);
        assert_eq!(
            invitation.accept(user_id, accepted_at),
            Ok(InvitationAcceptance::Applied)
        );
        assert_eq!(
            invitation.accept(user_id, accepted_at),
            Ok(InvitationAcceptance::Replay)
        );
        assert_eq!(
            invitation.accept(Uuid::new_v4().into(), accepted_at),
            Err(WorkspaceInvitationError::AcceptedByAnotherUser)
        );
    }

    #[test]
    fn resend_rotates_generation_expiry_and_delivery_state() {
        let mut invitation = pending();
        let first_expiry = invitation.expires_at();
        invitation
            .queue_delivery(1, invitation.created_at())
            .unwrap();
        assert!(invitation.record_delivery_failure(
            1,
            "exhausted",
            invitation.created_at() + Duration::minutes(1)
        ));

        let resent_at = invitation.created_at() + Duration::hours(2);
        invitation
            .resend(1, resent_at, resent_at + Duration::days(7))
            .unwrap();

        assert_eq!(invitation.generation(), 2);
        assert_eq!(invitation.expires_at(), resent_at + Duration::days(7));
        assert!(invitation.expires_at() > first_expiry);
        assert_eq!(invitation.queued_generation(), Some(2));
        assert_eq!(invitation.queued_at(), Some(resent_at));
        assert_eq!(invitation.last_delivery_failure(), None);
        assert_eq!(
            invitation.resend(1, resent_at, resent_at + Duration::days(7)),
            Err(WorkspaceInvitationError::StaleGeneration)
        );
    }

    #[test]
    fn delivery_updates_only_the_current_pending_generation() {
        let mut invitation = pending();
        invitation
            .queue_delivery(1, invitation.created_at())
            .unwrap();
        assert!(!invitation.record_delivery_success(2, invitation.created_at()));
        assert!(invitation.record_delivery_success(1, invitation.created_at()));
        assert_eq!(invitation.delivered_generation(), Some(1));
        assert!(!invitation.record_delivery_failure(2, "permanent", invitation.created_at()));
        assert_eq!(invitation.last_delivery_failure(), None);
    }
}

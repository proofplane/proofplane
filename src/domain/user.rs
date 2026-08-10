use chrono::{DateTime, Utc};

use super::ids::uuid_id;

uuid_id!(UserId);

/**
 * User is the human management-plane identity, backed by Auth0 and keyed on
 * `auth0_sub`. JIT-provisioned the first time a valid Auth0 token is seen.
 */
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    pub id: UserId,
    pub auth0_sub: String,
    pub email: Option<String>,
    pub name: Option<String>,
    pub last_login_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserTransition {
    Applied,
    Replay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum UserError {
    #[error("user lifecycle timestamps are inconsistent")]
    InvalidLifecycle,
}

impl User {
    pub fn provision(
        id: UserId,
        auth0_sub: String,
        email: Option<String>,
        name: Option<String>,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            auth0_sub,
            email,
            name,
            last_login_at: None,
            created_at,
        }
    }

    pub fn rehydrate(
        id: UserId,
        auth0_sub: String,
        email: Option<String>,
        name: Option<String>,
        last_login_at: Option<DateTime<Utc>>,
        created_at: DateTime<Utc>,
    ) -> Result<Self, UserError> {
        if last_login_at.is_some_and(|logged_in_at| logged_in_at < created_at) {
            return Err(UserError::InvalidLifecycle);
        }
        Ok(Self {
            id,
            auth0_sub,
            email,
            name,
            last_login_at,
            created_at,
        })
    }

    pub fn provision_profile(
        &mut self,
        email: Option<String>,
        name: Option<String>,
    ) -> UserTransition {
        let next_email = email.or_else(|| self.email.clone());
        let next_name = name.or_else(|| self.name.clone());
        if self.email == next_email && self.name == next_name {
            return UserTransition::Replay;
        }
        self.email = next_email;
        self.name = next_name;
        UserTransition::Applied
    }

    pub fn record_login(
        &mut self,
        logged_in_at: DateTime<Utc>,
    ) -> Result<UserTransition, UserError> {
        if logged_in_at < self.created_at {
            return Err(UserError::InvalidLifecycle);
        }
        if self
            .last_login_at
            .is_some_and(|current| logged_in_at <= current)
        {
            return Ok(UserTransition::Replay);
        }
        self.last_login_at = Some(logged_in_at);
        Ok(UserTransition::Applied)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvisionUserPayload {
    pub auth0_sub: String,
    pub email: Option<String>,
    pub name: Option<String>,
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone, Utc};
    use uuid::Uuid;

    use super::{User, UserId, UserTransition};

    #[test]
    fn user_id_is_uuid_value_type() {
        let uuid = Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap();
        let id = UserId::from(uuid);

        assert_eq!(Uuid::from(id), uuid);
        assert_eq!(id, UserId::from(uuid));
    }

    #[test]
    fn provisioning_replay_preserves_identity_and_absent_profile_claims() {
        let created_at = Utc.with_ymd_and_hms(2026, 8, 2, 12, 0, 0).single().unwrap();
        let mut user = User::provision(
            UserId::from(Uuid::new_v4()),
            "auth0|alice".to_owned(),
            Some("alice@example.com".to_owned()),
            Some("Alice".to_owned()),
            created_at,
        );

        assert_eq!(user.provision_profile(None, None), UserTransition::Replay);
        assert_eq!(user.email.as_deref(), Some("alice@example.com"));
        assert_eq!(user.name.as_deref(), Some("Alice"));
        assert_eq!(user.created_at, created_at);
    }

    #[test]
    fn provisioning_updates_present_profile_claims_once() {
        let created_at = Utc.with_ymd_and_hms(2026, 8, 2, 12, 0, 0).single().unwrap();
        let mut user = User::provision(
            UserId::from(Uuid::new_v4()),
            "auth0|alice".to_owned(),
            None,
            None,
            created_at,
        );

        assert_eq!(
            user.provision_profile(
                Some("alice@example.com".to_owned()),
                Some("Alice".to_owned())
            ),
            UserTransition::Applied
        );
        assert_eq!(
            user.provision_profile(
                Some("alice@example.com".to_owned()),
                Some("Alice".to_owned())
            ),
            UserTransition::Replay
        );
    }

    #[test]
    fn login_replay_does_not_move_the_lifecycle_timestamp() {
        let created_at = Utc.with_ymd_and_hms(2026, 8, 2, 12, 0, 0).single().unwrap();
        let logged_in_at = created_at + Duration::minutes(1);
        let mut user = User::provision(
            UserId::from(Uuid::new_v4()),
            "auth0|alice".to_owned(),
            None,
            None,
            created_at,
        );

        assert_eq!(user.record_login(logged_in_at), Ok(UserTransition::Applied));
        assert_eq!(user.record_login(logged_in_at), Ok(UserTransition::Replay));
        assert_eq!(
            user.record_login(logged_in_at - Duration::seconds(1)),
            Ok(UserTransition::Replay)
        );
        assert_eq!(user.last_login_at, Some(logged_in_at));
    }
}

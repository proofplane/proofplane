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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvisionUserPayload {
    pub auth0_sub: String,
    pub email: Option<String>,
    pub name: Option<String>,
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::UserId;

    #[test]
    fn user_id_is_uuid_value_type() {
        let uuid = Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap();
        let id = UserId::from(uuid);

        assert_eq!(Uuid::from(id), uuid);
        assert_eq!(id, UserId::from(uuid));
    }
}

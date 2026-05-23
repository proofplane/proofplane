use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiCredential {
    pub id: String,
    pub actor_id: String,
    pub name: String,
    pub key_id: String,
    pub credential_hash: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateApiCredentialPayload {
    pub id: String,
    pub actor_id: String,
    pub name: String,
    pub key_id: String,
    pub credential_hash: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateApiCredentialPayload {
    pub actor_id: String,
    pub name: String,
    pub key_id: String,
    pub credential_hash: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::ApiCredential;

    #[test]
    fn credential_domain_carries_lifecycle_fields() {
        let created_at = Utc.timestamp_opt(1, 0).unwrap();
        let expires_at = Utc.timestamp_opt(2, 0).unwrap();
        let revoked_at = Utc.timestamp_opt(3, 0).unwrap();
        let credential = ApiCredential {
            id: "credential".to_owned(),
            actor_id: "actor".to_owned(),
            name: "Actor API Key".to_owned(),
            key_id: "key-id".to_owned(),
            credential_hash: "$argon2id$hash".to_owned(),
            expires_at: Some(expires_at),
            revoked_at: Some(revoked_at),
            created_at,
        };

        assert_eq!(credential.key_id, "key-id");
        assert_eq!(credential.expires_at, Some(expires_at));
        assert_eq!(credential.revoked_at, Some(revoked_at));
        assert_eq!(credential.created_at, created_at);
    }
}

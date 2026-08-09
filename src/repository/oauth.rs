use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::{
    domain::{
        NewOAuthAuthorizationCode, NewOAuthAuthorizationRequest, OAuthAuthorizationCode,
        OAuthAuthorizationFlow, OAuthAuthorizationFlowCode, OAuthAuthorizationRequest,
        OAuthAuthorizationRequestId, Sha256Digest, UserId, WorkspaceId, WorkspacePermission,
    },
    services::agent_connections::digest_secret,
};

use super::{constraints::classify_db_error, Error, Postgres, TransactionContext};

/// Complete-snapshot persistence for OAuth authorization flows. Legacy methods
/// below remain only for the uncoupled OAuth service during adapter cutover.
pub struct OAuthAuthorizationFlowRepository<'a> {
    context: &'a TransactionContext<'a>,
}

impl<'a> TransactionContext<'a> {
    pub fn oauth_authorization_flows(&'a self) -> OAuthAuthorizationFlowRepository<'a> {
        OAuthAuthorizationFlowRepository { context: self }
    }
}

impl OAuthAuthorizationFlowRepository<'_> {
    pub async fn get(
        &self,
        request_id: OAuthAuthorizationRequestId,
    ) -> Result<Option<OAuthAuthorizationFlow>, Error> {
        self.get_with("r.id = $1", &[&Uuid::from(request_id)]).await
    }

    pub async fn get_by_csrf_digest(
        &self,
        digest: Sha256Digest,
    ) -> Result<Option<OAuthAuthorizationFlow>, Error> {
        let digest: &[u8] = digest.as_bytes();
        self.get_with("r.csrf_token_digest = $1", &[&digest]).await
    }

    pub async fn get_by_code_digest(
        &self,
        digest: Sha256Digest,
    ) -> Result<Option<OAuthAuthorizationFlow>, Error> {
        let digest: &[u8] = digest.as_bytes();
        self.get_with("c.code_digest = $1", &[&digest]).await
    }

    async fn get_with(
        &self,
        predicate: &str,
        values: &[&(dyn tokio_postgres::types::ToSql + Sync)],
    ) -> Result<Option<OAuthAuthorizationFlow>, Error> {
        // Locking the request serializes every lifecycle transition, including
        // code consumption, without attempting to lock the nullable side of
        // the left join before a code has been issued.
        let sql = format!("{FLOW_SELECT_SQL} WHERE {predicate} FOR UPDATE OF r");
        self.context
            .transaction
            .query_opt(&sql, values)
            .await?
            .map(flow_from_row)
            .transpose()
    }

    pub async fn save(&self, flow: &OAuthAuthorizationFlow) -> Result<(), Error> {
        let record = OAuthFlowRecord::from(flow);
        let affected = self
            .context
            .transaction
            .execute(
                FLOW_SAVE_SQL,
                &[
                    &record.id,
                    &record.client_id,
                    &record.client_name,
                    &record.redirect_uri,
                    &record.code_challenge,
                    &record.state,
                    &record.resource,
                    &record.scopes,
                    &record.csrf_digest,
                    &record.auth0_subject,
                    &record.user_id,
                    &record.expires_at,
                    &record.created_at,
                    &record.consumed_at,
                ],
            )
            .await?;
        if affected != 1 {
            return Err(Error::InvariantViolation(
                "OAuth authorization flow snapshot save affected an unexpected row count",
            ));
        }
        if let Some(code) = OAuthCodeRecord::from(flow) {
            let affected = self
                .context
                .transaction
                .execute(
                    CODE_SAVE_SQL,
                    &[
                        &code.code_digest,
                        &code.request_id,
                        &code.agent_connection_id,
                        &code.workspace_id,
                        &code.client_id,
                        &code.redirect_uri,
                        &code.code_challenge,
                        &code.resource,
                        &code.scopes,
                        &code.expires_at,
                        &code.created_at,
                        &code.consumed_at,
                    ],
                )
                .await?;
            if affected != 1 {
                return Err(Error::InvariantViolation(
                    "OAuth authorization code snapshot save affected an unexpected row count",
                ));
            }
        }
        Ok(())
    }
}

const FLOW_SELECT_SQL: &str = r#"SELECT r.id, r.client_id, r.client_name, r.redirect_uri, r.code_challenge, r.state, r.resource, r.scopes, r.csrf_token_digest, r.auth0_subject, r.user_id, r.expires_at AS request_expires_at, r.created_at AS request_created_at, r.consumed_at AS request_consumed_at, c.code_digest, c.agent_connection_id, c.workspace_id, c.expires_at AS code_expires_at, c.created_at AS code_created_at, c.consumed_at AS code_consumed_at FROM oauth_authorization_requests r LEFT JOIN oauth_authorization_codes c ON c.request_id = r.id"#;
const FLOW_SAVE_SQL: &str = r#"INSERT INTO oauth_authorization_requests (id, client_id, client_name, redirect_uri, code_challenge, state, resource, scopes, csrf_token_digest, auth0_subject, user_id, expires_at, created_at, consumed_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14) ON CONFLICT (id) DO UPDATE SET client_id = EXCLUDED.client_id, client_name = EXCLUDED.client_name, redirect_uri = EXCLUDED.redirect_uri, code_challenge = EXCLUDED.code_challenge, state = EXCLUDED.state, resource = EXCLUDED.resource, scopes = EXCLUDED.scopes, csrf_token_digest = EXCLUDED.csrf_token_digest, auth0_subject = EXCLUDED.auth0_subject, user_id = EXCLUDED.user_id, expires_at = EXCLUDED.expires_at, created_at = EXCLUDED.created_at, consumed_at = EXCLUDED.consumed_at"#;
const CODE_SAVE_SQL: &str = r#"INSERT INTO oauth_authorization_codes (code_digest, request_id, agent_connection_id, workspace_id, client_id, redirect_uri, code_challenge, resource, scopes, expires_at, created_at, consumed_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12) ON CONFLICT (request_id) DO UPDATE SET code_digest = EXCLUDED.code_digest, agent_connection_id = EXCLUDED.agent_connection_id, workspace_id = EXCLUDED.workspace_id, client_id = EXCLUDED.client_id, redirect_uri = EXCLUDED.redirect_uri, code_challenge = EXCLUDED.code_challenge, resource = EXCLUDED.resource, scopes = EXCLUDED.scopes, expires_at = EXCLUDED.expires_at, created_at = EXCLUDED.created_at, consumed_at = EXCLUDED.consumed_at"#;

struct OAuthFlowRecord {
    id: Uuid,
    client_id: String,
    client_name: String,
    redirect_uri: String,
    code_challenge: String,
    state: String,
    resource: String,
    scopes: Vec<String>,
    csrf_digest: Vec<u8>,
    auth0_subject: Option<String>,
    user_id: Option<Uuid>,
    expires_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
    consumed_at: Option<DateTime<Utc>>,
}
impl From<&OAuthAuthorizationFlow> for OAuthFlowRecord {
    fn from(flow: &OAuthAuthorizationFlow) -> Self {
        Self {
            id: flow.id().into(),
            client_id: flow.client_id().to_owned(),
            client_name: flow.client_name().to_owned(),
            redirect_uri: flow.redirect_uri().to_owned(),
            code_challenge: flow.code_challenge().to_owned(),
            state: flow.state().to_owned(),
            resource: flow.resource().to_owned(),
            scopes: permission_strings(flow.scopes()),
            csrf_digest: flow.csrf_digest().as_bytes().to_vec(),
            auth0_subject: flow.auth0_subject().map(str::to_owned),
            user_id: flow.user_id().map(Uuid::from),
            expires_at: flow.expires_at(),
            created_at: flow.created_at(),
            consumed_at: flow.consumed_at(),
        }
    }
}
struct OAuthCodeRecord {
    code_digest: Vec<u8>,
    request_id: Uuid,
    agent_connection_id: Uuid,
    workspace_id: Uuid,
    client_id: String,
    redirect_uri: String,
    code_challenge: String,
    resource: String,
    scopes: Vec<String>,
    expires_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
    consumed_at: Option<DateTime<Utc>>,
}
impl OAuthCodeRecord {
    fn from(flow: &OAuthAuthorizationFlow) -> Option<Self> {
        flow.authorization_code().map(|code| Self {
            code_digest: code.code_digest().as_bytes().to_vec(),
            request_id: flow.id().into(),
            agent_connection_id: code.agent_connection_id().into(),
            workspace_id: code.workspace_id().into(),
            client_id: flow.client_id().to_owned(),
            redirect_uri: flow.redirect_uri().to_owned(),
            code_challenge: flow.code_challenge().to_owned(),
            resource: flow.resource().to_owned(),
            scopes: permission_strings(flow.scopes()),
            expires_at: code.expires_at(),
            created_at: code.created_at(),
            consumed_at: code.consumed_at(),
        })
    }
}

fn flow_from_row(row: Row) -> Result<OAuthAuthorizationFlow, Error> {
    let csrf: [u8; 32] = row
        .try_get::<_, Vec<u8>>("csrf_token_digest")?
        .try_into()
        .map_err(|_| Error::InvariantViolation("OAuth CSRF digest must contain 32 bytes"))?;
    let code = row
        .try_get::<_, Option<Vec<u8>>>("code_digest")?
        .map(|bytes| {
            let digest: [u8; 32] = bytes.try_into().map_err(|_| {
                Error::InvariantViolation("OAuth code digest must contain 32 bytes")
            })?;
            OAuthAuthorizationFlowCode::rehydrate(
                Sha256Digest::from_bytes(digest),
                row.try_get::<_, Uuid>("agent_connection_id")?.into(),
                row.try_get::<_, Uuid>("workspace_id")?.into(),
                row.try_get("code_expires_at")?,
                row.try_get("code_created_at")?,
                row.try_get("code_consumed_at")?,
            )
            .map_err(|_| {
                Error::InvariantViolation("persisted OAuth authorization code is inconsistent")
            })
        })
        .transpose()?;
    OAuthAuthorizationFlow::rehydrate(
        row.try_get::<_, Uuid>("id")?.into(),
        row.try_get("client_id")?,
        row.try_get("client_name")?,
        row.try_get("redirect_uri")?,
        row.try_get("code_challenge")?,
        row.try_get("state")?,
        row.try_get("resource")?,
        parse_permissions(row.try_get("scopes")?)?,
        Sha256Digest::from_bytes(csrf),
        row.try_get("auth0_subject")?,
        row.try_get::<_, Option<Uuid>>("user_id")?.map(UserId::from),
        row.try_get("request_expires_at")?,
        row.try_get("request_created_at")?,
        row.try_get("request_consumed_at")?,
        code,
    )
    .map_err(|_| Error::InvariantViolation("persisted OAuth authorization flow is inconsistent"))
}

impl Postgres {
    pub async fn create_oauth_authorization_request(
        &self,
        request: &NewOAuthAuthorizationRequest,
    ) -> Result<OAuthAuthorizationRequest, Error> {
        let db = self.get().await?;
        let csrf_digest = digest_secret(&request.csrf_token);
        let csrf_digest: &[u8] = csrf_digest.as_bytes();
        let scopes = permission_strings(&request.scopes);
        let row = db
            .query_one(
                r#"
INSERT INTO oauth_authorization_requests (
    id, client_id, client_name, redirect_uri, code_challenge, state, resource,
    scopes, csrf_token_digest, expires_at
)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
RETURNING id, client_id, client_name, redirect_uri, code_challenge, state,
    resource, scopes, auth0_subject, user_id, expires_at
"#,
                &[
                    &Uuid::from(request.id),
                    &request.client_id,
                    &request.client_name,
                    &request.redirect_uri,
                    &request.code_challenge,
                    &request.state,
                    &request.resource,
                    &scopes,
                    &csrf_digest,
                    &request.expires_at,
                ],
            )
            .await
            .map_err(classify_db_error)?;
        oauth_request_from_row(&row)
    }

    pub async fn get_oauth_authorization_request_by_csrf(
        &self,
        csrf_token: &str,
    ) -> Result<Option<OAuthAuthorizationRequest>, Error> {
        let db = self.get().await?;
        let digest = digest_secret(csrf_token);
        let digest: &[u8] = digest.as_bytes();
        db.query_opt(
            r#"
SELECT id, client_id, client_name, redirect_uri, code_challenge, state, resource,
    scopes, auth0_subject, user_id, expires_at
FROM oauth_authorization_requests
WHERE csrf_token_digest = $1
  AND expires_at > now()
  AND consumed_at IS NULL
"#,
            &[&digest],
        )
        .await?
        .map(|row| oauth_request_from_row(&row))
        .transpose()
    }

    pub async fn get_oauth_authorization_request(
        &self,
        request_id: OAuthAuthorizationRequestId,
    ) -> Result<Option<OAuthAuthorizationRequest>, Error> {
        let db = self.get().await?;
        db.query_opt(
            r#"
SELECT id, client_id, client_name, redirect_uri, code_challenge, state, resource,
    scopes, auth0_subject, user_id, expires_at
FROM oauth_authorization_requests
WHERE id = $1
  AND expires_at > now()
  AND consumed_at IS NULL
"#,
            &[&Uuid::from(request_id)],
        )
        .await?
        .map(|row| oauth_request_from_row(&row))
        .transpose()
    }

    pub async fn attach_oauth_authorization_subject(
        &self,
        request_id: OAuthAuthorizationRequestId,
        auth0_subject: &str,
        user_id: UserId,
    ) -> Result<Option<OAuthAuthorizationRequest>, Error> {
        let db = self.get().await?;
        db.query_opt(
            r#"
UPDATE oauth_authorization_requests
SET auth0_subject = $2, user_id = $3
WHERE id = $1
  AND expires_at > now()
  AND consumed_at IS NULL
RETURNING id, client_id, client_name, redirect_uri, code_challenge, state,
    resource, scopes, auth0_subject, user_id, expires_at
"#,
            &[
                &Uuid::from(request_id),
                &auth0_subject,
                &Uuid::from(user_id),
            ],
        )
        .await?
        .map(|row| oauth_request_from_row(&row))
        .transpose()
    }

    pub async fn consume_oauth_authorization_request(
        &self,
        request_id: OAuthAuthorizationRequestId,
    ) -> Result<Option<OAuthAuthorizationRequest>, Error> {
        let db = self.get().await?;
        db.query_opt(
            r#"
UPDATE oauth_authorization_requests
SET consumed_at = now()
WHERE id = $1
  AND expires_at > now()
  AND consumed_at IS NULL
  AND auth0_subject IS NOT NULL
  AND user_id IS NOT NULL
RETURNING id, client_id, client_name, redirect_uri, code_challenge, state,
    resource, scopes, auth0_subject, user_id, expires_at
"#,
            &[&Uuid::from(request_id)],
        )
        .await?
        .map(|row| oauth_request_from_row(&row))
        .transpose()
    }

    pub async fn create_oauth_authorization_code(
        &self,
        code: &NewOAuthAuthorizationCode,
    ) -> Result<(), Error> {
        let db = self.get().await?;
        let code_digest = digest_secret(&code.code);
        let code_digest: &[u8] = code_digest.as_bytes();
        let scopes = permission_strings(&code.scopes);
        db.execute(
            r#"
WITH consumed_request AS (
    UPDATE oauth_authorization_requests
    SET consumed_at = now()
    WHERE id = $2
      AND consumed_at IS NULL
      AND expires_at > now()
    RETURNING id
)
INSERT INTO oauth_authorization_codes (
    code_digest, request_id, agent_connection_id, workspace_id, client_id, redirect_uri,
    code_challenge, resource, scopes, expires_at
)
SELECT $1, id, $3, $4, $5, $6, $7, $8, $9, $10
FROM consumed_request
"#,
            &[
                &code_digest,
                &Uuid::from(code.request_id),
                &Uuid::from(code.agent_connection_id),
                &Uuid::from(code.workspace_id),
                &code.client_id,
                &code.redirect_uri,
                &code.code_challenge,
                &code.resource,
                &scopes,
                &code.expires_at,
            ],
        )
        .await
        .map_err(classify_db_error)?;
        Ok(())
    }

    pub async fn consume_oauth_authorization_code(
        &self,
        code: &str,
        client_id: &str,
        redirect_uri: &str,
    ) -> Result<Option<OAuthAuthorizationCode>, Error> {
        let mut db = self.get().await?;
        let tx = db.transaction().await?;
        let digest = digest_secret(code);
        let digest: &[u8] = digest.as_bytes();
        let row = tx
            .query_opt(
                r#"
WITH consumed AS (
    UPDATE oauth_authorization_codes
    SET consumed_at = now()
    WHERE code_digest = $1
      AND client_id = $2
      AND redirect_uri = $3
      AND consumed_at IS NULL
      AND expires_at > now()
    RETURNING request_id, agent_connection_id, workspace_id, client_id, redirect_uri,
        code_challenge, resource, scopes, expires_at
)
SELECT consumed.*, r.auth0_subject, r.user_id
FROM consumed
JOIN oauth_authorization_requests r ON r.id = consumed.request_id
"#,
                &[&digest, &client_id, &redirect_uri],
            )
            .await?;
        let output = row.map(|row| oauth_code_from_row(&row)).transpose()?;
        tx.commit().await?;
        Ok(output)
    }
}

fn oauth_request_from_row(row: &Row) -> Result<OAuthAuthorizationRequest, Error> {
    Ok(OAuthAuthorizationRequest {
        id: OAuthAuthorizationRequestId::from(row.try_get::<_, Uuid>("id")?),
        client_id: row.try_get("client_id")?,
        client_name: row.try_get("client_name")?,
        redirect_uri: row.try_get("redirect_uri")?,
        code_challenge: row.try_get("code_challenge")?,
        state: row.try_get("state")?,
        resource: row.try_get("resource")?,
        scopes: parse_permissions(row.try_get("scopes")?)?,
        auth0_subject: row.try_get("auth0_subject")?,
        user_id: row.try_get::<_, Option<Uuid>>("user_id")?.map(UserId::from),
        expires_at: row.try_get("expires_at")?,
    })
}

fn oauth_code_from_row(row: &Row) -> Result<OAuthAuthorizationCode, Error> {
    Ok(OAuthAuthorizationCode {
        request_id: OAuthAuthorizationRequestId::from(row.try_get::<_, Uuid>("request_id")?),
        agent_connection_id: row.try_get::<_, Uuid>("agent_connection_id")?.into(),
        workspace_id: row
            .try_get::<_, Uuid>("workspace_id")
            .map(WorkspaceId::from)?,
        client_id: row.try_get("client_id")?,
        redirect_uri: row.try_get("redirect_uri")?,
        code_challenge: row.try_get("code_challenge")?,
        resource: row.try_get("resource")?,
        scopes: parse_permissions(row.try_get("scopes")?)?,
        auth0_subject: row
            .try_get::<_, Option<String>>("auth0_subject")?
            .ok_or(Error::InvariantViolation("OAuth code missing subject"))?,
        user_id: row
            .try_get::<_, Option<Uuid>>("user_id")?
            .map(UserId::from)
            .ok_or(Error::InvariantViolation("OAuth code missing user"))?,
        expires_at: row.try_get("expires_at")?,
    })
}

fn permission_strings(permissions: &[WorkspacePermission]) -> Vec<String> {
    permissions
        .iter()
        .map(|permission| permission.as_str().to_owned())
        .collect()
}

fn parse_permissions(values: Vec<String>) -> Result<Vec<WorkspacePermission>, Error> {
    values
        .into_iter()
        .map(|value| {
            value
                .parse()
                .map_err(|_| Error::InvariantViolation("unknown OAuth scope"))
        })
        .collect()
}

#[cfg(test)]
mod snapshot_tests {
    use super::{CODE_SAVE_SQL, FLOW_SAVE_SQL, FLOW_SELECT_SQL};

    #[test]
    fn flow_snapshot_loads_and_saves_request_and_code_state() {
        assert!(FLOW_SELECT_SQL.contains("LEFT JOIN oauth_authorization_codes"));
        for field in [
            "csrf_token_digest",
            "auth0_subject",
            "user_id",
            "consumed_at",
        ] {
            assert!(FLOW_SAVE_SQL.contains(field));
        }
        for field in [
            "code_digest",
            "agent_connection_id",
            "workspace_id",
            "consumed_at",
        ] {
            assert!(CODE_SAVE_SQL.contains(field));
        }
    }
}

use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::domain::{
    OAuthAuthorizationFlow, OAuthAuthorizationFlowCode, OAuthAuthorizationRequestId, Sha256Digest,
    UserId, WorkspacePermission,
};

use super::{Error, UnitOfWork};

/// Complete-snapshot persistence for OAuth authorization flows.
pub struct OAuthAuthorizationFlowRepository<'a> {
    unit_of_work: &'a UnitOfWork<'a>,
}

impl<'a> UnitOfWork<'a> {
    pub fn oauth_authorization_flows(&'a self) -> OAuthAuthorizationFlowRepository<'a> {
        OAuthAuthorizationFlowRepository { unit_of_work: self }
    }
}

impl OAuthAuthorizationFlowRepository<'_> {
    pub async fn get(
        &self,
        request_id: OAuthAuthorizationRequestId,
    ) -> Result<Option<OAuthAuthorizationFlow>, Error> {
        // Locking the request serializes every lifecycle transition, including
        // code consumption, without attempting to lock the nullable side of
        // the left join before a code has been issued.
        self.unit_of_work
            .transaction
            .query_opt(
                &format!("{FLOW_SELECT_SQL} WHERE r.id = $1 FOR UPDATE OF r"),
                &[&Uuid::from(request_id)],
            )
            .await?
            .map(flow_from_row)
            .transpose()
    }

    pub async fn save(&self, flow: &OAuthAuthorizationFlow) -> Result<(), Error> {
        let record = OAuthFlowRecord::from(flow);
        let affected = self
            .unit_of_work
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
                .unit_of_work
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

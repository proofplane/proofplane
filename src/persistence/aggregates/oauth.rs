use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::domain::{
    OAuthAuthorizationFlow, OAuthAuthorizationFlowCode, OAuthAuthorizationRequestId, Sha256Digest,
    UserId, WorkspacePermission,
};

use super::params::param;
use super::{
    snapshot::{save_snapshot, snapshot_record},
    Error, UnitOfWork,
};

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
            .query_typed_opt(
                &format!("{FLOW_SELECT_SQL} WHERE r.id = $1 FOR UPDATE OF r"),
                &[param(&Uuid::from(request_id))],
            )
            .await?
            .map(|row| {
                let code = OAuthCodeRecord::try_from_row(&row)?
                    .map(OAuthCodeRecord::into_domain)
                    .transpose()?;
                OAuthAuthorizationFlowRecord::try_from_row(&row)?.into_domain(code)
            })
            .transpose()
    }

    pub async fn save(&self, flow: &OAuthAuthorizationFlow) -> Result<(), Error> {
        let record = OAuthAuthorizationFlowRecord::from_domain(flow)?;
        save_snapshot(&self.unit_of_work.transaction, record.as_snapshot()).await?;
        if let Some(code) = OAuthCodeRecord::from_domain(flow)? {
            save_snapshot(&self.unit_of_work.transaction, code.as_snapshot()).await?;
        } else {
            self.unit_of_work
                .transaction
                .execute_typed(
                    "DELETE FROM oauth_authorization_codes WHERE request_id = $1",
                    &[param(&Uuid::from(flow.id()))],
                )
                .await?;
        }
        Ok(())
    }
}

const FLOW_SELECT_SQL: &str = r#"SELECT r.id, r.client_id, r.client_name, r.redirect_uri, r.code_challenge, r.state, r.resource, r.scopes, r.csrf_token_digest, r.auth0_subject, r.user_id, r.expires_at AS request_expires_at, r.created_at AS request_created_at, r.consumed_at AS request_consumed_at, c.code_digest, c.agent_connection_id, c.workspace_id, c.expires_at AS code_expires_at, c.created_at AS code_created_at, c.consumed_at AS code_consumed_at FROM oauth_authorization_requests r LEFT JOIN oauth_authorization_codes c ON c.request_id = r.id"#;
snapshot_record! {
struct OAuthAuthorizationFlowRecord {
    id: Uuid,
    client_id: String,
    client_name: String,
    redirect_uri: String,
    code_challenge: String,
    state: String,
    resource: String,
    scopes: Vec<String>,
    csrf_token_digest: Vec<u8>,
    auth0_subject: Option<String>,
    user_id: Option<Uuid>,
    expires_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
    consumed_at: Option<DateTime<Utc>>,
}
table: oauth_authorization_requests,
conflict: id,
}
impl OAuthAuthorizationFlowRecord {
    fn try_from_row(row: &Row) -> Result<Self, Error> {
        Ok(Self {
            id: row.try_get("id")?,
            client_id: row.try_get("client_id")?,
            client_name: row.try_get("client_name")?,
            redirect_uri: row.try_get("redirect_uri")?,
            code_challenge: row.try_get("code_challenge")?,
            state: row.try_get("state")?,
            resource: row.try_get("resource")?,
            scopes: row.try_get("scopes")?,
            csrf_token_digest: row.try_get("csrf_token_digest")?,
            auth0_subject: row.try_get("auth0_subject")?,
            user_id: row.try_get("user_id")?,
            expires_at: row.try_get("request_expires_at")?,
            created_at: row.try_get("request_created_at")?,
            consumed_at: row.try_get("request_consumed_at")?,
        })
    }

    fn from_domain(flow: &OAuthAuthorizationFlow) -> Result<Self, Error> {
        Ok(Self {
            id: flow.id().into(),
            client_id: flow.client_id().to_owned(),
            client_name: flow.client_name().to_owned(),
            redirect_uri: flow.redirect_uri().to_owned(),
            code_challenge: flow.code_challenge().to_owned(),
            state: flow.state().to_owned(),
            resource: flow.resource().to_owned(),
            scopes: permission_strings(flow.scopes()),
            csrf_token_digest: flow.csrf_digest().as_bytes().to_vec(),
            auth0_subject: flow.auth0_subject().map(str::to_owned),
            user_id: flow.user_id().map(Uuid::from),
            expires_at: flow.expires_at(),
            created_at: flow.created_at(),
            consumed_at: flow.consumed_at(),
        })
    }

    fn into_domain(
        self,
        code: Option<OAuthAuthorizationFlowCode>,
    ) -> Result<OAuthAuthorizationFlow, Error> {
        let csrf: [u8; 32] = self
            .csrf_token_digest
            .try_into()
            .map_err(|_| Error::InvariantViolation("OAuth CSRF digest must contain 32 bytes"))?;
        OAuthAuthorizationFlow::rehydrate(
            self.id.into(),
            self.client_id,
            self.client_name,
            self.redirect_uri,
            self.code_challenge,
            self.state,
            self.resource,
            parse_permissions(self.scopes)?,
            Sha256Digest::from_bytes(csrf),
            self.auth0_subject,
            self.user_id.map(UserId::from),
            self.expires_at,
            self.created_at,
            self.consumed_at,
            code,
        )
        .map_err(|_| {
            Error::InvariantViolation("persisted OAuth authorization flow is inconsistent")
        })
    }
}
snapshot_record! {
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
table: oauth_authorization_codes,
conflict: request_id,
}
impl OAuthCodeRecord {
    fn try_from_row(row: &Row) -> Result<Option<Self>, Error> {
        let Some(code_digest) = row.try_get::<_, Option<Vec<u8>>>("code_digest")? else {
            return Ok(None);
        };
        Ok(Some(Self {
            code_digest,
            request_id: row.try_get("id")?,
            agent_connection_id: row.try_get("agent_connection_id")?,
            workspace_id: row.try_get("workspace_id")?,
            client_id: row.try_get("client_id")?,
            redirect_uri: row.try_get("redirect_uri")?,
            code_challenge: row.try_get("code_challenge")?,
            resource: row.try_get("resource")?,
            scopes: row.try_get("scopes")?,
            expires_at: row.try_get("code_expires_at")?,
            created_at: row.try_get("code_created_at")?,
            consumed_at: row.try_get("code_consumed_at")?,
        }))
    }

    fn from_domain(flow: &OAuthAuthorizationFlow) -> Result<Option<Self>, Error> {
        Ok(flow.authorization_code().map(|code| Self {
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
        }))
    }

    fn into_domain(self) -> Result<OAuthAuthorizationFlowCode, Error> {
        let digest: [u8; 32] = self
            .code_digest
            .try_into()
            .map_err(|_| Error::InvariantViolation("OAuth code digest must contain 32 bytes"))?;
        OAuthAuthorizationFlowCode::rehydrate(
            Sha256Digest::from_bytes(digest),
            self.agent_connection_id.into(),
            self.workspace_id.into(),
            self.expires_at,
            self.created_at,
            self.consumed_at,
        )
        .map_err(|_| {
            Error::InvariantViolation("persisted OAuth authorization code is inconsistent")
        })
    }
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
    use super::FLOW_SELECT_SQL;

    #[test]
    fn flow_snapshot_loads_and_saves_request_and_code_state() {
        assert!(FLOW_SELECT_SQL.contains("LEFT JOIN oauth_authorization_codes"));
        assert!(FLOW_SELECT_SQL.contains("code_digest"));
    }
}

use tokio_postgres::Row;
use uuid::Uuid;

use crate::{
    domain::{
        NewOAuthAuthorizationCode, NewOAuthAuthorizationRequest, NewOAuthClient,
        OAuthAuthorizationCode, OAuthAuthorizationRequest, OAuthAuthorizationRequestId,
        OAuthClient, UserId, WorkspaceId, WorkspacePermission,
    },
    services::agent_connections::digest_secret,
};

use super::{constraints::classify_db_error, Error, Postgres};

impl Postgres {
    pub async fn create_oauth_client(&self, client: &NewOAuthClient) -> Result<OAuthClient, Error> {
        let db = self.get().await?;
        let row = db
            .query_one(
                r#"
INSERT INTO oauth_clients (id, client_name, redirect_uris)
VALUES ($1, $2, $3)
RETURNING id, client_name, redirect_uris, created_at
"#,
                &[&client.id, &client.client_name, &client.redirect_uris],
            )
            .await
            .map_err(classify_db_error)?;
        oauth_client_from_row(&row)
    }

    pub async fn get_oauth_client(&self, id: &str) -> Result<Option<OAuthClient>, Error> {
        let db = self.get().await?;
        db.query_opt(
            r#"
SELECT id, client_name, redirect_uris, created_at
FROM oauth_clients
WHERE id = $1
"#,
            &[&id],
        )
        .await?
        .map(|row| oauth_client_from_row(&row))
        .transpose()
    }

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
    id, client_id, redirect_uri, code_challenge, state, resource, scopes,
    csrf_token_digest, expires_at
)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
RETURNING id, client_id, redirect_uri, code_challenge, state, resource, scopes,
    auth0_subject, user_id, expires_at
"#,
                &[
                    &Uuid::from(request.id),
                    &request.client_id,
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
SELECT id, client_id, redirect_uri, code_challenge, state, resource, scopes,
    auth0_subject, user_id, expires_at
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
SELECT id, client_id, redirect_uri, code_challenge, state, resource, scopes,
    auth0_subject, user_id, expires_at
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
RETURNING id, client_id, redirect_uri, code_challenge, state, resource, scopes,
    auth0_subject, user_id, expires_at
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
RETURNING id, client_id, redirect_uri, code_challenge, state, resource, scopes,
    auth0_subject, user_id, expires_at
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

fn oauth_client_from_row(row: &Row) -> Result<OAuthClient, Error> {
    Ok(OAuthClient {
        id: row.try_get("id")?,
        client_name: row.try_get("client_name")?,
        redirect_uris: row.try_get("redirect_uris")?,
        created_at: row.try_get("created_at")?,
    })
}

fn oauth_request_from_row(row: &Row) -> Result<OAuthAuthorizationRequest, Error> {
    Ok(OAuthAuthorizationRequest {
        id: OAuthAuthorizationRequestId::from(row.try_get::<_, Uuid>("id")?),
        client_id: row.try_get("client_id")?,
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

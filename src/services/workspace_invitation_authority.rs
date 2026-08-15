use std::collections::HashMap;

use chrono::{DateTime, SecondsFormat, Utc};
use pasetors::{
    keys::SymmetricKey,
    token::{Local, UntrustedToken},
    version4::{LocalToken, V4},
};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

use crate::{
    config::WorkspaceInvitationsConfig,
    domain::{WorkspaceInvitation, WorkspaceInvitationId},
};

const PURPOSE: &str = "workspace-invitation";
const IMPLICIT_ASSERTION: &[u8] = b"proofplane:workspace-invitation:v1";

#[derive(Clone)]
pub struct WorkspaceInvitationAuthority {
    landing_portal_base_url: Url,
    active_key_id: String,
    active_key: SymmetricKey<V4>,
    keys: HashMap<String, SymmetricKey<V4>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifiedWorkspaceInvitationAuthority {
    pub invitation_id: crate::domain::WorkspaceInvitationId,
    pub generation: i64,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

pub struct IssuedWorkspaceInvitationLink {
    pub url: Url,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceInvitationAuthoritySource {
    pub invitation_id: WorkspaceInvitationId,
    pub generation: i64,
    pub expires_at: DateTime<Utc>,
}

impl From<&WorkspaceInvitation> for WorkspaceInvitationAuthoritySource {
    fn from(invitation: &WorkspaceInvitation) -> Self {
        Self {
            invitation_id: invitation.id(),
            generation: invitation.generation(),
            expires_at: invitation.expires_at(),
        }
    }
}

#[derive(Debug, Clone, Copy, thiserror::Error)]
pub enum WorkspaceInvitationAuthorityError {
    #[error("workspace invitation authority configuration is invalid")]
    Configuration,
    #[error("workspace invitation authority could not be issued")]
    Issue,
    #[error("workspace invitation authority is unavailable")]
    Unavailable,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Claims {
    invitation_id: String,
    generation: i64,
    purpose: String,
    iat: String,
    exp: String,
}

#[derive(Serialize, Deserialize)]
struct Footer {
    kid: String,
}

impl WorkspaceInvitationAuthority {
    pub fn from_config(
        config: &WorkspaceInvitationsConfig,
    ) -> Result<Self, WorkspaceInvitationAuthorityError> {
        let active = config
            .keys
            .iter()
            .find(|key| key.id == config.active_key_id)
            .ok_or(WorkspaceInvitationAuthorityError::Configuration)?;
        let active_key = SymmetricKey::<V4>::try_from(active.secret.expose_secret())
            .map_err(|_| WorkspaceInvitationAuthorityError::Configuration)?;
        let keys = config
            .keys
            .iter()
            .map(|key| {
                SymmetricKey::<V4>::try_from(key.secret.expose_secret())
                    .map(|secret| (key.id.clone(), secret))
                    .map_err(|_| WorkspaceInvitationAuthorityError::Configuration)
            })
            .collect::<Result<HashMap<_, _>, _>>()?;
        Ok(Self {
            landing_portal_base_url: config.landing_portal_base_url.clone(),
            active_key_id: active.id.clone(),
            active_key,
            keys,
        })
    }

    pub fn issue(
        &self,
        source: WorkspaceInvitationAuthoritySource,
    ) -> Result<IssuedWorkspaceInvitationLink, WorkspaceInvitationAuthorityError> {
        self.issue_at(source, Utc::now())
    }

    fn issue_at(
        &self,
        source: WorkspaceInvitationAuthoritySource,
        issued_at: DateTime<Utc>,
    ) -> Result<IssuedWorkspaceInvitationLink, WorkspaceInvitationAuthorityError> {
        if issued_at >= source.expires_at {
            return Err(WorkspaceInvitationAuthorityError::Unavailable);
        }
        let claims = Claims {
            invitation_id: Uuid::from(source.invitation_id).to_string(),
            generation: source.generation,
            purpose: PURPOSE.to_owned(),
            iat: timestamp(issued_at),
            exp: timestamp(source.expires_at),
        };
        let payload =
            serde_json::to_vec(&claims).map_err(|_| WorkspaceInvitationAuthorityError::Issue)?;
        let footer = serde_json::to_vec(&Footer {
            kid: self.active_key_id.clone(),
        })
        .map_err(|_| WorkspaceInvitationAuthorityError::Issue)?;
        let token = LocalToken::encrypt(
            &self.active_key,
            &payload,
            Some(&footer),
            Some(IMPLICIT_ASSERTION),
        )
        .map_err(|_| WorkspaceInvitationAuthorityError::Issue)?;
        let mut url = self.landing_portal_base_url.clone();
        url.set_path("/join");
        url.set_query(None);
        url.set_fragment(Some(&format!("token={token}")));
        Ok(IssuedWorkspaceInvitationLink { url })
    }

    pub fn verify(
        &self,
        token: &str,
    ) -> Result<VerifiedWorkspaceInvitationAuthority, WorkspaceInvitationAuthorityError> {
        self.verify_at(token, Utc::now())
    }

    fn verify_at(
        &self,
        token: &str,
        now: DateTime<Utc>,
    ) -> Result<VerifiedWorkspaceInvitationAuthority, WorkspaceInvitationAuthorityError> {
        let untrusted = UntrustedToken::<Local, V4>::try_from(token)
            .map_err(|_| WorkspaceInvitationAuthorityError::Unavailable)?;
        let footer: Footer = serde_json::from_slice(untrusted.untrusted_footer())
            .map_err(|_| WorkspaceInvitationAuthorityError::Unavailable)?;
        let key = self
            .keys
            .get(&footer.kid)
            .ok_or(WorkspaceInvitationAuthorityError::Unavailable)?;
        let trusted = LocalToken::decrypt(
            key,
            &untrusted,
            Some(untrusted.untrusted_footer()),
            Some(IMPLICIT_ASSERTION),
        )
        .map_err(|_| WorkspaceInvitationAuthorityError::Unavailable)?;
        let claims: Claims = serde_json::from_str(trusted.payload())
            .map_err(|_| WorkspaceInvitationAuthorityError::Unavailable)?;
        let invitation_id = Uuid::parse_str(&claims.invitation_id)
            .map_err(|_| WorkspaceInvitationAuthorityError::Unavailable)?
            .into();
        let issued_at = parse_timestamp(&claims.iat)?;
        let expires_at = parse_timestamp(&claims.exp)?;
        if claims.purpose != PURPOSE
            || claims.generation <= 0
            || issued_at > now
            || expires_at <= now
            || issued_at >= expires_at
        {
            return Err(WorkspaceInvitationAuthorityError::Unavailable);
        }
        Ok(VerifiedWorkspaceInvitationAuthority {
            invitation_id,
            generation: claims.generation,
            issued_at,
            expires_at,
        })
    }
}

fn timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Secs, true)
}
fn parse_timestamp(value: &str) -> Result<DateTime<Utc>, WorkspaceInvitationAuthorityError> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| WorkspaceInvitationAuthorityError::Unavailable)
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone};
    use secrecy::SecretString;

    use super::*;
    use crate::{config::WorkspaceInvitationPasetoKey, domain::WorkspaceInvitation};

    fn service() -> WorkspaceInvitationAuthority {
        WorkspaceInvitationAuthority::from_config(&WorkspaceInvitationsConfig {
            landing_portal_base_url: Url::parse("https://app.proofplane.test/base").unwrap(),
            active_key_id: "invite-1".to_owned(),
            keys: vec![WorkspaceInvitationPasetoKey {
                id: "invite-1".to_owned(),
                secret: SecretString::from("k4.local.mKj2EzeLOuNBNlHNX6oLl76yopCc1K9YvWQVIo1xYEs"),
            }],
        })
        .unwrap()
    }

    #[test]
    fn link_is_fragment_only_and_generation_bound() {
        let now = Utc
            .with_ymd_and_hms(2026, 8, 12, 12, 0, 0)
            .single()
            .unwrap();
        let invitation = WorkspaceInvitation::create(
            Uuid::new_v4().into(),
            Uuid::new_v4().into(),
            Uuid::new_v4().into(),
            "admin@example.com".to_owned(),
            now,
            now + Duration::days(7),
        )
        .unwrap();
        let service = service();
        let link = service
            .issue_at((&invitation).into(), now + Duration::seconds(1))
            .unwrap();
        assert_eq!(link.url.path(), "/join");
        assert!(link.url.query().is_none());
        let token = link.url.fragment().unwrap().strip_prefix("token=").unwrap();
        let verified = service
            .verify_at(token, now + Duration::minutes(1))
            .unwrap();
        assert_eq!(verified.invitation_id, invitation.id());
        assert_eq!(verified.generation, 1);
        assert_eq!(verified.expires_at, invitation.expires_at());
    }
}

use chrono::{DateTime, Utc};
use rmcp::schemars::{self, JsonSchema};
use secrecy::{ExposeSecret, SecretString};
use serde::Serialize;
use url::Url;

use super::common::format_datetime;

const AUTHORIZATION_SCHEME: &str = "Proofplane-Upload";

#[derive(Serialize, JsonSchema)]
pub(super) struct MachineUploadDescriptor {
    method: &'static str,
    url: String,
    authorization: String,
    content_type: String,
    expires_at: String,
    max_bytes: u64,
}

impl MachineUploadDescriptor {
    pub(super) fn new(
        url: Url,
        credential: &SecretString,
        content_type: &str,
        expires_at: DateTime<Utc>,
        max_bytes: u64,
    ) -> Self {
        Self {
            method: "PUT",
            url: url.to_string(),
            authorization: format!("{AUTHORIZATION_SCHEME} {}", credential.expose_secret()),
            content_type: content_type.to_owned(),
            expires_at: format_datetime(expires_at),
            max_bytes,
        }
    }
}

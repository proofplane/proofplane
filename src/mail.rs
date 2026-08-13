use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use reqwest::StatusCode;
use secrecy::{ExposeSecret, SecretString};
use serde::Serialize;
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailMessage {
    pub to: String,
    pub subject: String,
    pub text: String,
    pub html: String,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailFailureClass {
    Retryable,
    Permanent,
}

impl MailFailureClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Retryable => "retryable",
            Self::Permanent => "permanent",
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("mail provider request failed ({class:?}, status class: {status_class})")]
pub struct MailError {
    pub class: MailFailureClass,
    pub status_class: &'static str,
}

#[async_trait]
pub trait MailAdapter: Send + Sync {
    async fn send(&self, message: MailMessage) -> Result<(), MailError>;
}

#[derive(Debug, Default)]
pub struct LocalMailAdapter;

#[async_trait]
impl MailAdapter for LocalMailAdapter {
    async fn send(&self, _message: MailMessage) -> Result<(), MailError> {
        tracing::info!(
            provider = "local",
            outcome = "captured",
            "mail delivery completed"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
pub struct CapturingMailAdapter {
    messages: Arc<Mutex<Vec<MailMessage>>>,
}

impl CapturingMailAdapter {
    pub fn messages(&self) -> Vec<MailMessage> {
        self.messages
            .lock()
            .map(|messages| messages.clone())
            .unwrap_or_default()
    }
}

#[async_trait]
impl MailAdapter for CapturingMailAdapter {
    async fn send(&self, message: MailMessage) -> Result<(), MailError> {
        self.messages
            .lock()
            .map_err(|_| MailError {
                class: MailFailureClass::Retryable,
                status_class: "local",
            })?
            .push(message);
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ResendMailAdapter {
    client: reqwest::Client,
    endpoint: Url,
    api_key: SecretString,
    sender: String,
}

impl ResendMailAdapter {
    pub fn new(endpoint: Url, api_key: SecretString, sender: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            endpoint,
            api_key,
            sender,
        }
    }
}

#[derive(Serialize)]
struct ResendRequest<'a> {
    from: &'a str,
    to: [&'a str; 1],
    subject: &'a str,
    text: &'a str,
    html: &'a str,
}

#[async_trait]
impl MailAdapter for ResendMailAdapter {
    async fn send(&self, message: MailMessage) -> Result<(), MailError> {
        let response = self
            .client
            .post(self.endpoint.clone())
            .bearer_auth(self.api_key.expose_secret())
            .header("Idempotency-Key", &message.idempotency_key)
            .json(&ResendRequest {
                from: &self.sender,
                to: [&message.to],
                subject: &message.subject,
                text: &message.text,
                html: &message.html,
            })
            .send()
            .await
            .map_err(|_| MailError {
                class: MailFailureClass::Retryable,
                status_class: "network",
            })?;
        if response.status().is_success() {
            return Ok(());
        }
        let status = response.status();
        Err(MailError {
            class: classify_status(status),
            status_class: status_class(status),
        })
    }
}

fn classify_status(status: StatusCode) -> MailFailureClass {
    if status == StatusCode::REQUEST_TIMEOUT
        || status == StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
    {
        MailFailureClass::Retryable
    } else {
        MailFailureClass::Permanent
    }
}

fn status_class(status: StatusCode) -> &'static str {
    if status.is_client_error() {
        "4xx"
    } else if status.is_server_error() {
        "5xx"
    } else {
        "other"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_statuses_have_bounded_retry_classification() {
        assert_eq!(
            classify_status(StatusCode::BAD_REQUEST),
            MailFailureClass::Permanent
        );
        assert_eq!(
            classify_status(StatusCode::TOO_MANY_REQUESTS),
            MailFailureClass::Retryable
        );
        assert_eq!(
            classify_status(StatusCode::BAD_GATEWAY),
            MailFailureClass::Retryable
        );
    }
}

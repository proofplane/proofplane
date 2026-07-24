use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde::Serialize;
use thiserror::Error;
use tokio::time::sleep;
use uuid::Uuid;

use crate::config::MailAdapterConfig;

const RESEND_EMAIL_ENDPOINT: &str = "https://api.resend.com/emails";
const RESEND_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const RESEND_RETRY_DELAY: Duration = Duration::from_millis(250);

#[derive(Debug, Error)]
pub enum MailError {
    #[error("mail delivery is disabled")]
    Disabled,
    #[error("mail capture failed")]
    Capture,
    #[error("mail provider request failed")]
    ProviderRequest,
    #[error("mail provider rejected the request with status {status}")]
    ProviderResponse { status: u16 },
}

pub struct AuditorOtpMail<'a> {
    pub id: Uuid,
    pub auditor_email: &'a str,
    pub code: &'a str,
}

#[async_trait]
pub trait MailAdapter: Send + Sync {
    async fn send_auditor_otp(&self, mail: &AuditorOtpMail<'_>) -> Result<(), MailError>;
}

pub type SharedMailAdapter = Arc<dyn MailAdapter>;

pub fn from_config(config: &MailAdapterConfig) -> SharedMailAdapter {
    match config {
        MailAdapterConfig::Disabled => Arc::new(DisabledMailAdapter),
        MailAdapterConfig::LocalStdout => Arc::new(LocalStdoutMailAdapter),
        MailAdapterConfig::Resend { api_key, from } => {
            Arc::new(ResendMailAdapter::new(api_key.clone(), from.clone()))
        }
    }
}

pub struct DisabledMailAdapter;

#[async_trait]
impl MailAdapter for DisabledMailAdapter {
    async fn send_auditor_otp(&self, _mail: &AuditorOtpMail<'_>) -> Result<(), MailError> {
        Err(MailError::Disabled)
    }
}

pub struct LocalStdoutMailAdapter;

#[async_trait]
impl MailAdapter for LocalStdoutMailAdapter {
    async fn send_auditor_otp(&self, mail: &AuditorOtpMail<'_>) -> Result<(), MailError> {
        println!(
            "Proofplane auditor OTP for {}: {}",
            mail.auditor_email, mail.code
        );
        Ok(())
    }
}

pub struct ResendMailAdapter {
    client: Client,
    api_key: SecretString,
    from: String,
    endpoint: String,
}

impl ResendMailAdapter {
    pub fn new(api_key: SecretString, from: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            from,
            endpoint: RESEND_EMAIL_ENDPOINT.to_owned(),
        }
    }

    #[cfg(test)]
    fn with_endpoint(api_key: SecretString, from: String, endpoint: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            from,
            endpoint,
        }
    }

    async fn send_once(&self, mail: &AuditorOtpMail<'_>) -> Result<(), ResendAttemptError> {
        let text = format!(
            "Your Proofplane verification code is:\n\n{}\n\nThis code expires in 10 minutes and can be used once.\n\nIf you did not request access, you can ignore this email.",
            mail.code
        );
        let html = format!(
            "<p>Your Proofplane verification code is:</p><p><strong style=\"font-size: 24px; letter-spacing: 0.15em;\">{}</strong></p><p>This code expires in 10 minutes and can be used once.</p><p>If you did not request access, you can ignore this email.</p>",
            mail.code
        );
        let payload = ResendEmailPayload {
            from: &self.from,
            to: mail.auditor_email,
            subject: "Your Proofplane auditor access code",
            text: &text,
            html: &html,
        };

        let response = self
            .client
            .post(&self.endpoint)
            .timeout(RESEND_REQUEST_TIMEOUT)
            .bearer_auth(self.api_key.expose_secret())
            .header("Idempotency-Key", format!("auditor-otp/{}", mail.id))
            .json(&payload)
            .send()
            .await
            .map_err(|error| ResendAttemptError::Request {
                retryable: error.is_connect() || error.is_timeout(),
            })?;

        if response.status().is_success() {
            return Ok(());
        }

        Err(ResendAttemptError::Response {
            status: response.status(),
        })
    }
}

#[async_trait]
impl MailAdapter for ResendMailAdapter {
    async fn send_auditor_otp(&self, mail: &AuditorOtpMail<'_>) -> Result<(), MailError> {
        let started_at = Instant::now();
        let mut attempts = 0_u8;

        loop {
            attempts += 1;
            match self.send_once(mail).await {
                Ok(()) => {
                    tracing::info!(
                        provider = "resend",
                        attempts,
                        latency_ms = started_at.elapsed().as_secs_f64() * 1000.0,
                        "auditor OTP mail accepted"
                    );
                    return Ok(());
                }
                Err(error) if attempts == 1 && error.retryable() => {
                    sleep(RESEND_RETRY_DELAY).await;
                }
                Err(error) => {
                    tracing::warn!(
                        provider = "resend",
                        attempts,
                        status = error.status().map(u16::from),
                        latency_ms = started_at.elapsed().as_secs_f64() * 1000.0,
                        "auditor OTP mail delivery failed"
                    );
                    return Err(error.into());
                }
            }
        }
    }
}

#[derive(Serialize)]
struct ResendEmailPayload<'a> {
    from: &'a str,
    to: &'a str,
    subject: &'a str,
    text: &'a str,
    html: &'a str,
}

enum ResendAttemptError {
    Request { retryable: bool },
    Response { status: StatusCode },
}

impl ResendAttemptError {
    fn retryable(&self) -> bool {
        match self {
            Self::Request { retryable } => *retryable,
            Self::Response { status } => {
                matches!(
                    *status,
                    StatusCode::REQUEST_TIMEOUT | StatusCode::TOO_MANY_REQUESTS
                ) || status.is_server_error()
            }
        }
    }

    fn status(&self) -> Option<StatusCode> {
        match self {
            Self::Request { .. } => None,
            Self::Response { status } => Some(*status),
        }
    }
}

impl From<ResendAttemptError> for MailError {
    fn from(error: ResendAttemptError) -> Self {
        match error {
            ResendAttemptError::Request { .. } => Self::ProviderRequest,
            ResendAttemptError::Response { status } => Self::ProviderResponse {
                status: status.as_u16(),
            },
        }
    }
}

#[derive(Default)]
pub struct CapturingMailAdapter {
    sent: Mutex<Vec<CapturedOtp>>,
    fail_delivery: Mutex<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedOtp {
    pub id: Uuid,
    pub auditor_email: String,
    pub code: String,
}

impl CapturingMailAdapter {
    pub fn sent(&self) -> Vec<CapturedOtp> {
        self.sent
            .lock()
            .map(|sent| sent.clone())
            .unwrap_or_default()
    }

    pub fn set_delivery_failure(&self, fail: bool) {
        if let Ok(mut fail_delivery) = self.fail_delivery.lock() {
            *fail_delivery = fail;
        }
    }
}

#[async_trait]
impl MailAdapter for CapturingMailAdapter {
    async fn send_auditor_otp(&self, mail: &AuditorOtpMail<'_>) -> Result<(), MailError> {
        if *self.fail_delivery.lock().map_err(|_| MailError::Capture)? {
            return Err(MailError::ProviderRequest);
        }

        self.sent
            .lock()
            .map_err(|_| MailError::Capture)?
            .push(CapturedOtp {
                id: mail.id,
                auditor_email: mail.auditor_email.to_owned(),
                code: mail.code.to_owned(),
            });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        sync::{Arc, Mutex},
    };

    use axum::{extract::State, http::HeaderMap, routing::post, Json, Router};
    use secrecy::SecretString;
    use serde_json::Value;

    use super::{
        AuditorOtpMail, MailAdapter, MailError, ResendAttemptError, ResendMailAdapter,
        RESEND_RETRY_DELAY,
    };

    #[derive(Clone)]
    struct TestServerState {
        statuses: Arc<Mutex<VecDeque<axum::http::StatusCode>>>,
        requests: Arc<Mutex<Vec<CapturedRequest>>>,
    }

    #[derive(Debug, Clone)]
    struct CapturedRequest {
        authorization: Option<String>,
        idempotency_key: Option<String>,
        body: Value,
    }

    async fn capture_request(
        State(state): State<TestServerState>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> axum::http::StatusCode {
        let authorization = headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let idempotency_key = headers
            .get("idempotency-key")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        if let Ok(mut requests) = state.requests.lock() {
            requests.push(CapturedRequest {
                authorization,
                idempotency_key,
                body,
            });
        }
        state
            .statuses
            .lock()
            .ok()
            .and_then(|mut statuses| statuses.pop_front())
            .unwrap_or(axum::http::StatusCode::ACCEPTED)
    }

    async fn test_server(
        statuses: impl IntoIterator<Item = axum::http::StatusCode>,
    ) -> (
        String,
        Arc<Mutex<Vec<CapturedRequest>>>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test mail server binds");
        let address = listener.local_addr().expect("test mail server has address");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let state = TestServerState {
            statuses: Arc::new(Mutex::new(statuses.into_iter().collect())),
            requests: requests.clone(),
        };
        let app = Router::new()
            .route("/emails", post(capture_request))
            .with_state(state);
        let handle = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("test mail server runs");
        });

        (format!("http://{address}/emails"), requests, handle)
    }

    fn mail() -> AuditorOtpMail<'static> {
        AuditorOtpMail {
            id: uuid::Uuid::parse_str("00000000-0000-4000-8000-000000000123")
                .expect("OTP ID parses"),
            auditor_email: "auditor@example.com",
            code: "123456",
        }
    }

    #[test]
    fn resend_retry_classification_covers_transient_statuses_only() {
        for status in [
            axum::http::StatusCode::REQUEST_TIMEOUT,
            axum::http::StatusCode::TOO_MANY_REQUESTS,
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            axum::http::StatusCode::BAD_GATEWAY,
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            axum::http::StatusCode::GATEWAY_TIMEOUT,
        ] {
            assert!(
                ResendAttemptError::Response { status }.retryable(),
                "{status} should be retryable"
            );
        }

        for status in [
            axum::http::StatusCode::BAD_REQUEST,
            axum::http::StatusCode::UNAUTHORIZED,
            axum::http::StatusCode::FORBIDDEN,
            axum::http::StatusCode::UNPROCESSABLE_ENTITY,
        ] {
            assert!(
                !ResendAttemptError::Response { status }.retryable(),
                "{status} should not be retryable"
            );
        }
    }

    #[tokio::test]
    async fn resend_sends_expected_otp_message_without_retrying_success() {
        let (endpoint, requests, handle) = test_server([axum::http::StatusCode::ACCEPTED]).await;
        let adapter = ResendMailAdapter::with_endpoint(
            SecretString::from("re_test_secret"),
            "Proofplane <noreply@notify.proofplane.app>".to_owned(),
            endpoint,
        );

        adapter.send_auditor_otp(&mail()).await.expect("mail sends");

        let requests = requests.lock().expect("captured requests lock");
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].authorization.as_deref(),
            Some("Bearer re_test_secret")
        );
        assert_eq!(
            requests[0].idempotency_key.as_deref(),
            Some("auditor-otp/00000000-0000-4000-8000-000000000123")
        );
        assert_eq!(
            requests[0].body["from"],
            "Proofplane <noreply@notify.proofplane.app>"
        );
        assert_eq!(requests[0].body["to"], "auditor@example.com");
        assert_eq!(
            requests[0].body["subject"],
            "Your Proofplane auditor access code"
        );
        assert!(requests[0].body["text"]
            .as_str()
            .is_some_and(|text| text.contains("123456") && text.contains("10 minutes")));
        assert!(requests[0].body["html"]
            .as_str()
            .is_some_and(|html| html.contains("123456") && html.contains("10 minutes")));

        handle.abort();
    }

    #[tokio::test]
    async fn resend_retries_retryable_status_once_with_same_idempotency_key() {
        let (endpoint, requests, handle) = test_server([
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            axum::http::StatusCode::ACCEPTED,
        ])
        .await;
        let adapter = ResendMailAdapter::with_endpoint(
            SecretString::from("re_test_secret"),
            "Proofplane <noreply@notify.proofplane.app>".to_owned(),
            endpoint,
        );

        adapter
            .send_auditor_otp(&mail())
            .await
            .expect("retry succeeds");

        let requests = requests.lock().expect("captured requests lock");
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].idempotency_key, requests[1].idempotency_key);

        handle.abort();
    }

    #[tokio::test]
    async fn resend_does_not_retry_permanent_rejection() {
        let (endpoint, requests, handle) =
            test_server([axum::http::StatusCode::UNPROCESSABLE_ENTITY]).await;
        let adapter = ResendMailAdapter::with_endpoint(
            SecretString::from("re_test_secret"),
            "Proofplane <noreply@notify.proofplane.app>".to_owned(),
            endpoint,
        );

        let error = adapter
            .send_auditor_otp(&mail())
            .await
            .expect_err("mail is rejected");

        assert!(matches!(error, MailError::ProviderResponse { status: 422 }));
        assert_eq!(requests.lock().expect("captured requests lock").len(), 1);

        handle.abort();
    }

    #[tokio::test]
    async fn resend_retries_connection_failure_once_and_returns_sanitized_error() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("unused address binds");
        let address = listener.local_addr().expect("unused address exists");
        drop(listener);
        let adapter = ResendMailAdapter::with_endpoint(
            SecretString::from("re_test_secret"),
            "Proofplane <noreply@notify.proofplane.app>".to_owned(),
            format!("http://{address}/emails"),
        );
        let started_at = std::time::Instant::now();

        let error = adapter
            .send_auditor_otp(&mail())
            .await
            .expect_err("connection remains unavailable");

        assert!(matches!(error, MailError::ProviderRequest));
        assert!(started_at.elapsed() >= RESEND_RETRY_DELAY);
        assert!(!error.to_string().contains("re_test_secret"));
        assert!(!error.to_string().contains("123456"));
    }
}

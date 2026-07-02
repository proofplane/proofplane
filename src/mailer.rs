use std::{
    env,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MailError {
    #[error("mail delivery is disabled")]
    Disabled,
    #[error("mail capture failed")]
    Capture,
}

#[async_trait]
pub trait MailAdapter: Send + Sync {
    async fn send_auditor_otp(&self, auditor_email: &str, code: &str) -> Result<(), MailError>;
}

pub type SharedMailAdapter = Arc<dyn MailAdapter>;

pub fn from_env() -> SharedMailAdapter {
    match env::var("PROOFPLANE_MAIL_ADAPTER").as_deref() {
        Ok("local_stdout") => Arc::new(LocalStdoutMailAdapter),
        _ => Arc::new(DisabledMailAdapter),
    }
}

pub struct DisabledMailAdapter;

#[async_trait]
impl MailAdapter for DisabledMailAdapter {
    async fn send_auditor_otp(&self, _auditor_email: &str, _code: &str) -> Result<(), MailError> {
        Err(MailError::Disabled)
    }
}

pub struct LocalStdoutMailAdapter;

#[async_trait]
impl MailAdapter for LocalStdoutMailAdapter {
    async fn send_auditor_otp(&self, auditor_email: &str, code: &str) -> Result<(), MailError> {
        println!("Proofplane auditor OTP for {auditor_email}: {code}");
        Ok(())
    }
}

#[derive(Default)]
pub struct CapturingMailAdapter {
    sent: Mutex<Vec<CapturedOtp>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedOtp {
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
}

#[async_trait]
impl MailAdapter for CapturingMailAdapter {
    async fn send_auditor_otp(&self, auditor_email: &str, code: &str) -> Result<(), MailError> {
        self.sent
            .lock()
            .map_err(|_| MailError::Capture)?
            .push(CapturedOtp {
                auditor_email: auditor_email.to_owned(),
                code: code.to_owned(),
            });
        Ok(())
    }
}

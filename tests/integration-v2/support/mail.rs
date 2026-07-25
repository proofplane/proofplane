use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use proofplane::mailer::{AuditorOtpMail, MailAdapter, MailError};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapturedOtp {
    pub id: Uuid,
    pub auditor_email: String,
    pub code: String,
}

#[derive(Clone, Default)]
pub struct TestMailAdapter {
    state: Arc<Mutex<MailState>>,
}

#[derive(Default)]
struct MailState {
    sent_by_recipient: HashMap<String, Vec<CapturedOtp>>,
    failure_counts: HashMap<String, usize>,
}

impl TestMailAdapter {
    pub fn sent_mail_for(&self, recipient: &str) -> Vec<CapturedOtp> {
        self.state
            .lock()
            .ok()
            .and_then(|state| state.sent_by_recipient.get(recipient).cloned())
            .unwrap_or_default()
    }

    pub fn fail_delivery_for(&self, recipient: &str) -> MailFailureGuard {
        if let Ok(mut state) = self.state.lock() {
            *state
                .failure_counts
                .entry(recipient.to_owned())
                .or_default() += 1;
        }

        MailFailureGuard {
            state: self.state.clone(),
            recipient: recipient.to_owned(),
        }
    }
}

pub struct MailFailureGuard {
    state: Arc<Mutex<MailState>>,
    recipient: String,
}

impl Drop for MailFailureGuard {
    fn drop(&mut self) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let Some(count) = state.failure_counts.get_mut(&self.recipient) else {
            return;
        };

        *count -= 1;
        if *count == 0 {
            state.failure_counts.remove(&self.recipient);
        }
    }
}

#[async_trait]
impl MailAdapter for TestMailAdapter {
    async fn send_auditor_otp(&self, mail: &AuditorOtpMail<'_>) -> Result<(), MailError> {
        let mut state = self.state.lock().map_err(|_| MailError::Capture)?;
        if state
            .failure_counts
            .get(mail.auditor_email)
            .copied()
            .unwrap_or_default()
            > 0
        {
            return Err(MailError::ProviderRequest);
        }

        state
            .sent_by_recipient
            .entry(mail.auditor_email.to_owned())
            .or_default()
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
    use proofplane::mailer::{AuditorOtpMail, MailAdapter, MailError};
    use uuid::Uuid;

    use super::TestMailAdapter;

    async fn send(
        adapter: &TestMailAdapter,
        id: Uuid,
        recipient: &str,
        code: &str,
    ) -> Result<(), MailError> {
        adapter
            .send_auditor_otp(&AuditorOtpMail {
                id,
                auditor_email: recipient,
                code,
            })
            .await
    }

    #[tokio::test]
    async fn concurrent_sends_are_partitioned_by_recipient() {
        let adapter = TestMailAdapter::default();
        let alice_id = Uuid::new_v4();
        let bob_id = Uuid::new_v4();

        let (alice, bob) = tokio::join!(
            send(&adapter, alice_id, "alice@example.com", "111111"),
            send(&adapter, bob_id, "bob@example.com", "222222"),
        );
        alice.expect("Alice's mail sends");
        bob.expect("Bob's mail sends");

        assert_eq!(
            adapter.sent_mail_for("alice@example.com"),
            [super::CapturedOtp {
                id: alice_id,
                auditor_email: "alice@example.com".to_owned(),
                code: "111111".to_owned(),
            }]
        );
        assert_eq!(
            adapter.sent_mail_for("bob@example.com"),
            [super::CapturedOtp {
                id: bob_id,
                auditor_email: "bob@example.com".to_owned(),
                code: "222222".to_owned(),
            }]
        );
        assert!(adapter.sent_mail_for("nobody@example.com").is_empty());
    }

    #[tokio::test]
    async fn recipient_failure_does_not_fail_or_capture_another_recipient() {
        let adapter = TestMailAdapter::default();
        let _alice_failure = adapter.fail_delivery_for("alice@example.com");
        let bob_id = Uuid::new_v4();

        let (alice, bob) = tokio::join!(
            send(&adapter, Uuid::new_v4(), "alice@example.com", "111111"),
            send(&adapter, bob_id, "bob@example.com", "222222"),
        );

        assert!(matches!(alice, Err(MailError::ProviderRequest)));
        bob.expect("Bob's mail sends");
        assert!(adapter.sent_mail_for("alice@example.com").is_empty());
        assert_eq!(adapter.sent_mail_for("bob@example.com")[0].id, bob_id);
    }

    #[tokio::test]
    async fn nested_guards_clean_up_only_their_own_recipient() {
        let adapter = TestMailAdapter::default();
        let first_alice_failure = adapter.fail_delivery_for("alice@example.com");
        let second_alice_failure = adapter.fail_delivery_for("alice@example.com");
        let bob_failure = adapter.fail_delivery_for("bob@example.com");

        drop(first_alice_failure);
        assert!(matches!(
            send(&adapter, Uuid::new_v4(), "alice@example.com", "111111").await,
            Err(MailError::ProviderRequest)
        ));

        drop(second_alice_failure);
        send(&adapter, Uuid::new_v4(), "alice@example.com", "222222")
            .await
            .expect("Alice's final guard restores delivery");
        assert!(matches!(
            send(&adapter, Uuid::new_v4(), "bob@example.com", "333333").await,
            Err(MailError::ProviderRequest)
        ));

        drop(bob_failure);
        send(&adapter, Uuid::new_v4(), "bob@example.com", "444444")
            .await
            .expect("Bob's guard restores delivery");
    }
}

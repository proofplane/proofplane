use std::{future::Future, time::Duration as StdDuration};

use chrono::{DateTime, Duration, Utc};
use serde_json::json;
use thiserror::Error;

use crate::{
    pubsub::{OutboundMessage, Publisher},
    repository::{OutboxMessage, Postgres},
};
use tracing::warn;

pub fn startup_message() -> &'static str {
    "proofplane outbox dequeuer ready"
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxDequeuerConfig {
    pub batch_size: i64,
    pub poll_interval: StdDuration,
    pub max_attempts: i32,
    pub initial_retry_delay: Duration,
    pub max_retry_delay: Duration,
}

impl Default for OutboxDequeuerConfig {
    fn default() -> Self {
        Self {
            batch_size: 100,
            poll_interval: StdDuration::from_secs(1),
            max_attempts: 10,
            initial_retry_delay: Duration::seconds(1),
            max_retry_delay: Duration::minutes(5),
        }
    }
}

#[derive(Debug, Error)]
pub enum OutboxDequeuerError {
    #[error("repository error")]
    Repository(#[from] crate::repository::Error),
}

pub struct OutboxDequeuer<'a, P> {
    postgres: &'a Postgres,
    publisher: &'a P,
    config: OutboxDequeuerConfig,
}

impl<'a, P> OutboxDequeuer<'a, P>
where
    P: Publisher + Sync,
{
    pub fn new(postgres: &'a Postgres, publisher: &'a P, config: OutboxDequeuerConfig) -> Self {
        Self {
            postgres,
            publisher,
            config,
        }
    }

    pub async fn run_once(&self, now: DateTime<Utc>) -> Result<usize, OutboxDequeuerError> {
        let rows = self
            .postgres
            .list_due_outbox_messages(now, self.config.batch_size)
            .await?;
        let mut published = 0;

        for row in rows {
            if self.publish_row(&row, now).await? {
                published += 1;
            }
        }

        Ok(published)
    }

    pub async fn run_until_cancelled<C>(&self, cancelled: C) -> Result<(), OutboxDequeuerError>
    where
        C: Future<Output = ()>,
    {
        tokio::pin!(cancelled);
        let mut interval = tokio::time::interval(self.config.poll_interval);

        loop {
            tokio::select! {
                _ = &mut cancelled => return Ok(()),
                _ = interval.tick() => {
                    self.run_once(Utc::now()).await?;
                }
            }
        }
    }

    async fn publish_row(
        &self,
        row: &OutboxMessage,
        now: DateTime<Utc>,
    ) -> Result<bool, OutboxDequeuerError> {
        match publish_outbox_row(self.publisher, &self.config, row, now).await {
            PublishOutcome::Published => {
                self.postgres.delete_outbox_message(row.id).await?;
                Ok(true)
            }
            PublishOutcome::Failed {
                error,
                next_available_at,
                exhausted,
            } => {
                warn!(
                    outbox_message_id = row.id,
                    attempt_count = row.attempt_count + 1,
                    exhausted,
                    error = %error,
                    "outbox publish failed"
                );
                self.postgres
                    .record_outbox_publish_failure(row.id, next_available_at)
                    .await?;
                Ok(false)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PublishOutcome {
    Published,
    Failed {
        error: String,
        next_available_at: DateTime<Utc>,
        exhausted: bool,
    },
}

async fn publish_outbox_row<P>(
    publisher: &P,
    config: &OutboxDequeuerConfig,
    row: &OutboxMessage,
    now: DateTime<Utc>,
) -> PublishOutcome
where
    P: Publisher + Sync,
{
    let message = outbound_message(row);

    match publisher.publish(&row.topic, message).await {
        Ok(_) => PublishOutcome::Published,
        Err(error) => {
            let exhausted = row.attempt_count + 1 >= config.max_attempts;
            let next_available_at = if exhausted {
                now + config.max_retry_delay
            } else {
                now + retry_delay(
                    row.attempt_count,
                    config.initial_retry_delay,
                    config.max_retry_delay,
                )
            };

            PublishOutcome::Failed {
                error: error.to_string(),
                next_available_at,
                exhausted,
            }
        }
    }
}

fn outbound_message(row: &OutboxMessage) -> OutboundMessage {
    let data = if row.message_version == 0 {
        json!({
            "outbox_message_id": row.id.to_string(),
            "event_type": row.event_type,
            "aggregate_type": row.aggregate_type,
            "aggregate_id": row.aggregate_id,
            "request_id": row.request_id,
            "payload": row.payload,
        })
    } else {
        json!({
            "message_id": row.message_id,
            "kind": row.message_kind,
            "type": row.message_type,
            "version": row.message_version,
            "subject": row.subject,
            "correlation_id": row.correlation_id,
            "causation_id": row.causation_id,
            "payload": row.payload,
        })
    };

    OutboundMessage::new(data.to_string().into_bytes())
}

fn retry_delay(attempt_count: i32, initial: Duration, max: Duration) -> Duration {
    let exponent = attempt_count.clamp(0, 30) as u32;
    let factor = 2_i32.pow(exponent);
    (initial * factor).min(max)
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone, Utc};
    use serde_json::json;
    use uuid::Uuid;

    use crate::{
        messaging::IntegrationMessageKind,
        pubsub::{fake::FakePublisher, Publisher, TopicName, MESSAGE_BUS_TOPIC},
        repository::OutboxMessage,
    };

    use super::{
        outbound_message, publish_outbox_row, retry_delay, startup_message, OutboxDequeuerConfig,
        PublishOutcome,
    };

    #[test]
    fn has_startup_message() {
        assert_eq!(startup_message(), "proofplane outbox dequeuer ready");
    }

    #[test]
    fn typed_outbox_publish_message_preserves_the_versioned_envelope() {
        let row = outbox_row(42, 0);

        let message = outbound_message(&row);

        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&message.data)
                .expect("message data is JSON"),
            json!({
                "message_id": Uuid::from_u128(41),
                "kind": "command",
                "type": "ScanDocument",
                "version": 1,
                "subject": "document-1",
                "correlation_id": Uuid::from_u128(42),
                "causation_id": Uuid::from_u128(40),
                "payload": { "scan_id": "scan-1" },
            })
        );
    }

    #[test]
    fn version_zero_outbox_publish_message_retains_the_legacy_envelope() {
        let mut row = outbox_row(42, 0);
        row.message_version = 0;
        row.message_type = "document.scan_requested".to_owned();

        let message = outbound_message(&row);

        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&message.data)
                .expect("message data is JSON"),
            json!({
                "outbox_message_id": "42",
                "event_type": "document.scan_requested",
                "aggregate_type": "evidence_document",
                "aggregate_id": "document-1",
                "request_id": "00000000-0000-4000-8000-000000000042",
                "payload": { "scan_id": "scan-1" },
            })
        );
    }

    #[test]
    fn retry_delay_uses_exponential_backoff_capped_at_max() {
        assert_eq!(
            retry_delay(0, Duration::seconds(2), Duration::seconds(30)),
            Duration::seconds(2)
        );
        assert_eq!(
            retry_delay(3, Duration::seconds(2), Duration::seconds(30)),
            Duration::seconds(16)
        );
        assert_eq!(
            retry_delay(8, Duration::seconds(2), Duration::seconds(30)),
            Duration::seconds(30)
        );
    }

    #[tokio::test]
    async fn fake_publisher_records_successes_and_transient_failures() {
        let publisher = FakePublisher::new();
        publisher.fail_next("emulator unavailable");

        let err = publisher
            .publish(
                &TopicName::new(MESSAGE_BUS_TOPIC),
                outbound_message(&outbox_row(1, 0)),
            )
            .await
            .expect_err("first publish fails");
        assert_eq!(err.to_string(), "publish failed: emulator unavailable");

        publisher
            .publish(
                &TopicName::new(MESSAGE_BUS_TOPIC),
                outbound_message(&outbox_row(2, 0)),
            )
            .await
            .expect("second publish succeeds");
        assert_eq!(publisher.published().len(), 1);
    }

    #[tokio::test]
    async fn dequeuer_publish_helper_reports_success() {
        let publisher = FakePublisher::new();
        let now = Utc.with_ymd_and_hms(2026, 6, 2, 12, 0, 0).unwrap();

        let outcome = publish_outbox_row(
            &publisher,
            &OutboxDequeuerConfig::default(),
            &outbox_row(1, 0),
            now,
        )
        .await;

        assert_eq!(outcome, PublishOutcome::Published);
        assert_eq!(publisher.published().len(), 1);
    }

    #[tokio::test]
    async fn dequeuer_publish_helper_reports_transient_failure_retry() {
        let publisher = FakePublisher::new();
        publisher.fail_next("pubsub unavailable");
        let now = Utc.with_ymd_and_hms(2026, 6, 2, 12, 0, 0).unwrap();
        let config = OutboxDequeuerConfig {
            initial_retry_delay: Duration::seconds(5),
            max_retry_delay: Duration::minutes(1),
            max_attempts: 3,
            ..OutboxDequeuerConfig::default()
        };

        let outcome = publish_outbox_row(&publisher, &config, &outbox_row(1, 1), now).await;

        assert_eq!(
            outcome,
            PublishOutcome::Failed {
                error: "publish failed: pubsub unavailable".to_owned(),
                next_available_at: now + Duration::seconds(10),
                exhausted: false,
            }
        );
        assert!(publisher.published().is_empty());
    }

    #[tokio::test]
    async fn dequeuer_publish_helper_surfaces_exhausted_retry() {
        let publisher = FakePublisher::new();
        publisher.fail_next("permission denied");
        let now = Utc.with_ymd_and_hms(2026, 6, 2, 12, 0, 0).unwrap();
        let config = OutboxDequeuerConfig {
            max_attempts: 2,
            max_retry_delay: Duration::minutes(30),
            ..OutboxDequeuerConfig::default()
        };

        let outcome = publish_outbox_row(&publisher, &config, &outbox_row(1, 1), now).await;

        assert_eq!(
            outcome,
            PublishOutcome::Failed {
                error: "publish failed: permission denied".to_owned(),
                next_available_at: now + Duration::minutes(30),
                exhausted: true,
            }
        );
    }

    #[test]
    fn default_dequeuer_config_has_retry_policy() {
        let config = OutboxDequeuerConfig::default();

        assert_eq!(config.batch_size, 100);
        assert_eq!(config.max_attempts, 10);
        assert!(config.max_retry_delay > config.initial_retry_delay);
    }

    fn outbox_row(id: i64, attempt_count: i32) -> OutboxMessage {
        OutboxMessage {
            id,
            topic: TopicName::new(MESSAGE_BUS_TOPIC),
            event_type: "document.scan_requested".to_owned(),
            aggregate_type: "evidence_document".to_owned(),
            aggregate_id: "document-1".to_owned(),
            payload: json!({ "scan_id": "scan-1" }),
            request_id: Some(
                "00000000-0000-4000-8000-000000000042"
                    .parse()
                    .expect("request id parses"),
            ),
            message_kind: IntegrationMessageKind::Command,
            message_type: "ScanDocument".to_owned(),
            message_version: 1,
            message_id: Uuid::from_u128(41),
            subject: "document-1".to_owned(),
            correlation_id: Some(Uuid::from_u128(42)),
            causation_id: Some(Uuid::from_u128(40)),
            attempt_count,
            next_available_at: Utc.with_ymd_and_hms(2026, 6, 2, 12, 0, 0).unwrap(),
            created_at: Utc.with_ymd_and_hms(2026, 6, 2, 12, 0, 0).unwrap(),
        }
    }
}

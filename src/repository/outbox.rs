use chrono::{DateTime, Utc};
use deadpool_postgres::GenericClient;
use serde_json::Value;
use tokio_postgres::Row;
use uuid::Uuid;

use crate::pubsub::TopicName;

use super::{Error, Postgres, TransactionContext, WorkspaceTransactionContext};

#[derive(Debug, Clone, PartialEq)]
pub struct NewOutboxMessage {
    pub topic: TopicName,
    pub event_type: String,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub payload: Value,
    pub request_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OutboxMessage {
    pub id: i64,
    pub topic: TopicName,
    pub event_type: String,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub payload: Value,
    pub request_id: Option<Uuid>,
    pub attempt_count: i32,
    pub next_available_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

impl WorkspaceTransactionContext<'_> {
    pub async fn append_outbox_message(
        &self,
        message: &NewOutboxMessage,
    ) -> Result<OutboxMessage, Error> {
        append_outbox_message(&self.transaction, message).await
    }
}

impl TransactionContext<'_> {
    pub async fn append_outbox_message(
        &self,
        message: &NewOutboxMessage,
    ) -> Result<OutboxMessage, Error> {
        append_outbox_message(&self.transaction, message).await
    }
}

async fn append_outbox_message(
    client: &impl GenericClient,
    message: &NewOutboxMessage,
) -> Result<OutboxMessage, Error> {
    let row = client
        .query_one(
            r#"
INSERT INTO outbox_messages (
    topic,
    event_type,
    aggregate_type,
    aggregate_id,
    payload,
    request_id
)
VALUES ($1, $2, $3, $4, $5, $6)
RETURNING
    id,
    topic,
    event_type,
    aggregate_type,
    aggregate_id,
    payload,
    request_id,
    attempt_count,
    next_available_at,
    created_at
"#,
            &[
                &message.topic.as_str(),
                &message.event_type,
                &message.aggregate_type,
                &message.aggregate_id,
                &message.payload,
                &message.request_id,
            ],
        )
        .await?;

    outbox_message_from_row(row)
}

impl Postgres {
    pub async fn list_due_outbox_messages(
        &self,
        now: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<OutboxMessage>, Error> {
        let client = self.get().await?;
        let rows = client
            .query(
                r#"
SELECT
    id,
    topic,
    event_type,
    aggregate_type,
    aggregate_id,
    payload,
    request_id,
    attempt_count,
    next_available_at,
    created_at
FROM outbox_messages
WHERE next_available_at <= $1
ORDER BY next_available_at, id
LIMIT $2
"#,
                &[&now, &limit],
            )
            .await?;

        rows.into_iter().map(outbox_message_from_row).collect()
    }

    pub async fn list_exhausted_outbox_messages(
        &self,
        max_attempts: i32,
        limit: i64,
    ) -> Result<Vec<OutboxMessage>, Error> {
        let client = self.get().await?;
        let rows = client
            .query(
                r#"
SELECT
    id,
    topic,
    event_type,
    aggregate_type,
    aggregate_id,
    payload,
    request_id,
    attempt_count,
    next_available_at,
    created_at
FROM outbox_messages
WHERE attempt_count >= $1
ORDER BY next_available_at, id
LIMIT $2
"#,
                &[&max_attempts, &limit],
            )
            .await?;

        rows.into_iter().map(outbox_message_from_row).collect()
    }

    pub async fn delete_outbox_message(&self, id: i64) -> Result<bool, Error> {
        let client = self.get().await?;
        let deleted = client
            .execute("DELETE FROM outbox_messages WHERE id = $1", &[&id])
            .await?;

        Ok(deleted > 0)
    }

    pub async fn record_outbox_publish_failure(
        &self,
        id: i64,
        next_available_at: DateTime<Utc>,
    ) -> Result<bool, Error> {
        let client = self.get().await?;
        let updated = client
            .execute(
                r#"
UPDATE outbox_messages
SET
    attempt_count = attempt_count + 1,
    next_available_at = $2
WHERE id = $1
"#,
                &[&id, &next_available_at],
            )
            .await?;

        Ok(updated > 0)
    }
}

fn outbox_message_from_row(row: Row) -> Result<OutboxMessage, Error> {
    Ok(OutboxMessage {
        id: row.try_get("id")?,
        topic: TopicName::new(row.try_get::<_, String>("topic")?),
        event_type: row.try_get("event_type")?,
        aggregate_type: row.try_get("aggregate_type")?,
        aggregate_id: row.try_get("aggregate_id")?,
        payload: row.try_get("payload")?,
        request_id: row.try_get("request_id")?,
        attempt_count: row.try_get("attempt_count")?,
        next_available_at: row.try_get("next_available_at")?,
        created_at: row.try_get("created_at")?,
    })
}

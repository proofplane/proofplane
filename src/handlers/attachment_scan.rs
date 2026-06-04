use crate::worker::{RetryableWorkerError, WorkerMessage};

pub async fn handle_scan_requested(message: WorkerMessage) -> Result<(), RetryableWorkerError> {
    tracing::info!(
        message_id = %message.message_id,
        event_type = %message.event_type,
        aggregate_type = %message.aggregate_type,
        aggregate_id = %message.aggregate_id,
        delivery_attempt = ?message.delivery_attempt,
        "worker message accepted"
    );

    Ok(())
}

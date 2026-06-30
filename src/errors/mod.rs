use std::time::Duration;

macro_rules! retry {
    ($error:ty, $retry_attempts:expr, $operation:expr) => {{
        let mut retry = 0;
        loop {
            match ($operation).await {
                Ok(value) => break Ok(value),
                Err(error) if !error.is_retryable() || retry == $retry_attempts => {
                    break Err(error);
                }
                Err(_) => {
                    tokio::time::sleep(<$error>::retry_delay(retry)).await;
                    retry += 1;
                }
            }
        }
    }};
    ($error:ty, $operation:expr) => {
        retry!($error, <$error>::DEFAULT_RETRY_ATTEMPTS, $operation)
    };
}

pub(crate) use retry;

#[allow(async_fn_in_trait)]
pub trait Retryable: Sized {
    const DEFAULT_RETRY_ATTEMPTS: usize = 3;

    fn is_retryable(&self) -> bool;

    fn initial_retry_delay() -> Duration {
        Duration::from_millis(100)
    }

    fn retry_delay(retry: usize) -> Duration {
        retry_delay(Self::initial_retry_delay(), retry)
    }

    async fn retry<T, F, Fut>(operation: F) -> Result<T, Self>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T, Self>>,
    {
        Self::retry_with_attempts(Self::DEFAULT_RETRY_ATTEMPTS, operation).await
    }

    async fn retry_with_attempts<T, F, Fut>(
        retry_attempts: usize,
        mut operation: F,
    ) -> Result<T, Self>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T, Self>>,
    {
        retry!(Self, retry_attempts, operation())
    }
}

fn retry_delay(initial: Duration, retry: usize) -> Duration {
    let factor = u32::try_from(retry)
        .ok()
        .and_then(|retry| 1_u32.checked_shl(retry))
        .unwrap_or(u32::MAX);
    initial.saturating_mul(factor)
}

#[cfg(test)]
mod tests {
    use super::{retry_delay, Retryable};
    use std::cell::Cell;
    use std::time::Duration;
    use thiserror::Error;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
    enum TransientTestError {
        #[error("temporary failure {0}")]
        Failed(usize),
        #[error("permanent failure")]
        Permanent,
    }

    impl Retryable for TransientTestError {
        fn is_retryable(&self) -> bool {
            matches!(self, Self::Failed(_))
        }

        fn initial_retry_delay() -> Duration {
            Duration::ZERO
        }
    }

    #[tokio::test]
    async fn retries_until_success_with_explicit_retry_count() {
        let attempts = Cell::new(0);

        let result = TransientTestError::retry_with_attempts(3, || async {
            attempts.set(attempts.get() + 1);

            if attempts.get() < 3 {
                Err(TransientTestError::Failed(attempts.get()))
            } else {
                Ok("ok")
            }
        })
        .await;

        assert_eq!(result, Ok("ok"));
        assert_eq!(attempts.get(), 3);
    }

    #[tokio::test]
    async fn exhaustion_returns_last_error() {
        let attempts = Cell::new(0);

        let result = TransientTestError::retry_with_attempts(2, || async {
            attempts.set(attempts.get() + 1);
            Err::<&str, _>(TransientTestError::Failed(attempts.get()))
        })
        .await;

        assert_eq!(result, Err(TransientTestError::Failed(3)));
        assert_eq!(attempts.get(), 3);
    }

    #[tokio::test]
    async fn permanent_error_returns_without_retrying() {
        let attempts = Cell::new(0);

        let result = TransientTestError::retry_with_attempts(3, || async {
            attempts.set(attempts.get() + 1);
            Err::<(), _>(TransientTestError::Permanent)
        })
        .await;

        assert_eq!(result, Err(TransientTestError::Permanent));
        assert_eq!(attempts.get(), 1);
    }

    #[tokio::test]
    async fn zero_retry_attempts_still_runs_once() {
        let attempts = Cell::new(0);

        let result = TransientTestError::retry_with_attempts(0, || async {
            attempts.set(attempts.get() + 1);
            Ok::<_, TransientTestError>("ok")
        })
        .await;

        assert_eq!(result, Ok("ok"));
        assert_eq!(attempts.get(), 1);
    }

    #[tokio::test]
    async fn default_retry_count_runs_initial_attempt_plus_default_retries() {
        let attempts = Cell::new(0);

        let result = TransientTestError::retry(|| async {
            attempts.set(attempts.get() + 1);
            Err::<&str, _>(TransientTestError::Failed(attempts.get()))
        })
        .await;

        assert_eq!(
            result,
            Err(TransientTestError::Failed(
                TransientTestError::DEFAULT_RETRY_ATTEMPTS + 1
            ))
        );
        assert_eq!(
            attempts.get(),
            TransientTestError::DEFAULT_RETRY_ATTEMPTS + 1
        );
    }

    #[test]
    fn delay_doubles_for_each_retry() {
        let initial = Duration::from_millis(100);

        assert_eq!(retry_delay(initial, 0), Duration::from_millis(100));
        assert_eq!(retry_delay(initial, 1), Duration::from_millis(200));
        assert_eq!(retry_delay(initial, 2), Duration::from_millis(400));
    }
}

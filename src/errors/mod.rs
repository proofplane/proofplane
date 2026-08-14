#[allow(async_fn_in_trait)]
pub trait Retryable: Sized {
    const DEFAULT_RETRY_ATTEMPTS: usize = 3;

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
        let mut last_error = match operation().await {
            Ok(value) => return Ok(value),
            Err(error) => error,
        };

        for _ in 0..retry_attempts {
            match operation().await {
                Ok(value) => return Ok(value),
                Err(error) => last_error = error,
            }
        }

        Err(last_error)
    }
}

#[cfg(test)]
mod tests {
    use super::Retryable;
    use std::cell::Cell;
    use thiserror::Error;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
    enum TransientTestError {
        #[error("temporary failure {0}")]
        Failed(usize),
    }

    impl Retryable for TransientTestError {}

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
}

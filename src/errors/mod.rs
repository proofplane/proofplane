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
        let max_attempts = retry_attempts + 1;
        let mut last_error = None;

        for _ in 0..max_attempts {
            match operation().await {
                Ok(value) => return Ok(value),
                Err(error) => last_error = Some(error),
            }
        }

        Err(last_error.expect("operation is always attempted at least once"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaffoldError {
    Failed,
}

impl Retryable for ScaffoldError {}

#[cfg(test)]
mod tests {
    use super::{Retryable, ScaffoldError};
    use std::cell::Cell;

    #[tokio::test]
    async fn retries_until_success_with_default_attempts() {
        let attempts = Cell::new(0);

        let result = ScaffoldError::retry(|| async {
            attempts.set(attempts.get() + 1);

            if attempts.get() < ScaffoldError::DEFAULT_RETRY_ATTEMPTS {
                Err(ScaffoldError::Failed)
            } else {
                Ok("ok")
            }
        })
        .await;

        assert_eq!(result, Ok("ok"));
        assert_eq!(attempts.get(), ScaffoldError::DEFAULT_RETRY_ATTEMPTS);
    }

    #[tokio::test]
    async fn returns_last_error_after_retry_attempts_are_exhausted() {
        let attempts = Cell::new(0);

        let result = ScaffoldError::retry_with_attempts(2, || async {
            attempts.set(attempts.get() + 1);
            Err::<&str, _>(ScaffoldError::Failed)
        })
        .await;

        assert_eq!(result, Err(ScaffoldError::Failed));
        assert_eq!(attempts.get(), 3);
    }

    #[tokio::test]
    async fn zero_retry_attempts_still_runs_once() {
        let attempts = Cell::new(0);

        let result = ScaffoldError::retry_with_attempts(0, || async {
            attempts.set(attempts.get() + 1);
            Ok::<_, ScaffoldError>("ok")
        })
        .await;

        assert_eq!(result, Ok("ok"));
        assert_eq!(attempts.get(), 1);
    }
}

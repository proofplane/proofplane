# 005 - Error, Retry, and Result Extensions

## Goal

Standardize the real error boundaries that exist today with `thiserror` and provide retry behavior for fallible async operations.

## Design

Use `thiserror` for concrete error types at real boundaries:

- configuration errors
- storage errors

Do not introduce placeholder error enums for API, worker, MCP, Pub/Sub, repositories, or services before those stories create real behavior at those boundaries.

Introduce a `Retryable` trait:

```rust
pub trait Retryable: Sized {
    async fn retry<T, F, Fut>(operation: F) -> Result<T, Self>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, Self>>;

    async fn retry_with_attempts<T, F, Fut>(
        retry_attempts: usize,
        operation: F,
    ) -> Result<T, Self>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, Self>>;
}
```

The API should support call sites like:

```rust
let response: Result<&str, ErrorType> = ErrorType::retry(|| async {
    operation_that_returns_result().await
})
.await;
```

Every `Err(ErrorType)` returned by the operation is retried until the configured retry count is exhausted. `retry_with_attempts(5, ...)` means one initial operation attempt plus up to five retries, for six total possible executions. Do not require individual error variants to classify themselves as retryable or non-retryable. Later implementation can add logging, metrics, configurable backoff, jitter, max retry attempts, timeout, and cancellation behavior.

## Acceptance Criteria

- Retry helper works for any error type that implements `Retryable`.
- Retry helper is async and uses `tokio`.
- Retry helper supports a default retry count and an explicit retry count.
- Retry helper retries every returned `Err(E)` until retry attempts are exhausted.
- Explicit retry counts are additional retries after the initial operation attempt.
- Retry logging and metrics are deferred to story 006, when observability is available.
- Future boundary error enums are introduced when their stories create real boundaries.

## Tests

- Unit tests cover success after N failures.
- Unit tests cover max retry attempts exhaustion.
- Unit tests cover explicit retry count and default retry count.
- Unit tests cover `retry_with_attempts(0)` running exactly once.
- Unit tests cover use with any test error type implementing `Retryable`.
- Error tests cover stable `StorageError` display output and `ConfigFieldError` display output.

## QA Guide

1. Run error and retry crate tests.
2. Use a fake operation that fails twice and succeeds, then verify exactly three attempts.
3. Use a fake operation that always fails and verify it runs once plus the configured retry count.
4. Run `make check`.
5. Run `make test-integration`.

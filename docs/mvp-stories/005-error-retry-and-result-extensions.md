# 005 - Error, Retry, and Result Extensions

## Goal

Standardize error handling with `thiserror` and provide retry behavior for fallible async operations.

## Design

Define error enums per boundary:

- configuration errors
- validation errors
- repository errors
- service errors
- Pub/Sub errors
- storage errors
- API errors
- worker errors
- MCP errors

Use `thiserror` for all error enums.

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

Every `Err(ErrorType)` returned by the operation is retried until the configured retry count is exhausted. `retry_with_attempts(5, ...)` means one initial operation attempt plus up to five retries, for six total possible executions. Do not require individual error variants to classify themselves as retryable or non-retryable. Later implementation can add configurable backoff, jitter, max retry attempts, and cancellation behavior.

## Acceptance Criteria

- Retry helper works for any error type that implements `Retryable`.
- Retry helper is async and uses `tokio`.
- Retry helper supports a default retry count and an explicit retry count.
- Retry helper retries every returned `Err(E)` until retry attempts are exhausted.
- Explicit retry counts are additional retries after the initial operation attempt.
- Retry attempts are logged and emit metrics once observability is available.

## Tests

- Unit tests cover success after N failures.
- Unit tests cover max retry attempts exhaustion.
- Unit tests cover explicit retry count and default retry count.
- Unit tests cover elapsed timeout or cancellation if supported.
- Service-layer tests verify repository errors are retried through the shared API.

## QA Guide

1. Run error and retry crate tests.
2. Use a fake operation that fails twice and succeeds, then verify exactly three attempts.
3. Use a fake operation that always fails and verify it runs once plus the configured retry count.

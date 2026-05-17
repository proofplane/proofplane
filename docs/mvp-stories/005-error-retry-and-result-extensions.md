# 005 - Error, Retry, and Result Extensions

## Goal

Standardize error handling with `thiserror` and provide retry behavior for temporary failures.

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
pub trait Retryable {
    fn is_retryable(&self) -> bool;
}
```

Add a result extension, such as `RetryResultExt`, that retries retryable failures with configurable backoff, jitter, max attempts, and cancellation behavior.

## Acceptance Criteria

- All infrastructure-facing errors classify retryable and non-retryable failures.
- Retry helper works with any `Result<T, E>` where `E: Retryable`.
- Retry helper is async and uses `tokio`.
- Retry attempts are logged and emit metrics once observability is available.
- Non-retryable errors are never retried.

## Tests

- Unit tests cover retryable success after N failures.
- Unit tests cover non-retryable immediate failure.
- Unit tests cover max attempts exhaustion.
- Unit tests cover elapsed timeout or cancellation if supported.
- Service-layer tests verify temporary repository errors are retried.

## QA Guide

1. Run error and retry crate tests.
2. Use a fake operation that fails twice and succeeds, then verify exactly three attempts.
3. Use a non-retryable fake error and verify exactly one attempt.

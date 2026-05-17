# 004 - Applicative Validation Framework

## Goal

Create the validation primitives and macro used by API request validation and configuration validation.

## Design

Implement:

- `Validation<T, E>` with valid and invalid states.
- Error accumulation for one or more field errors.
- A `validate!` macro supporting syntax like:

```rust
let control_upload = validate! {
    name <- ControlName::parse(&request.name).map_err(FieldError::from),
    some_prop <- Validation::valid(&request.some_prop),
    => Control::new(name, some_prop)
}
.into_result()?;
```

The macro should be simple, typed, and documented with examples. It should accept expressions that return either `Result<T, E>` or `Validation<T, E>` if practical; if supporting both makes the macro brittle, add explicit helpers and document the convention.

## Acceptance Criteria

- Validation errors accumulate across independent fields.
- Macro bindings preserve types and ownership without cloning unless the call site requests it.
- API handlers can map accumulated field errors into a stable error response DTO.
- Configuration validation can reuse the same framework.
- Validation APIs do not require dynamic dispatch.

## Tests

- Unit tests for `Validation::valid`, `Validation::invalid`, `map`, `and_then` if provided, and `into_result`.
- Macro tests for all-valid, one-invalid, and multiple-invalid cases.
- Compile-time style tests or doc tests for representative request DTO validation.
- API error DTO tests assert field paths and messages are stable.

## QA Guide

1. Run validation crate tests.
2. Add a temporary sample DTO with two invalid fields and confirm both errors are returned.
3. Confirm generated errors serialize cleanly to JSON.

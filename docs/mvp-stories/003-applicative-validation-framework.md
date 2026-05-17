# 003 - Applicative Validation Framework

## Goal

Create the validation primitives and macro used by API request validation and configuration validation.

This story only introduces the generic validation layer. Concrete API/configuration error DTOs, field error structs, and serialization shapes are owned by the stories that need them.

## Design

Implement:

- `Validation<T, E>` with `valid`, `invalid`, and `invalid_many` constructors.
- `into_result() -> Result<T, Vec<E>>`.
- `map` for transforming valid values while preserving errors.
- `and_then` for dependent validation chains.
- Error accumulation for one or more generic errors.
- A `validate!` macro supporting syntax like:

```rust
let control_upload = validate! {
    name <- ControlName::validate(&request.name),
    some_prop <- Validation::valid(&request.some_prop),
    => Control::new(name, some_prop)
}
.into_result()?;
```

Every binding in the macro must return `Validation<T, E>`. The macro itself returns one final `Validation`, and callers cross into fallible control flow explicitly with `into_result()`.

## Acceptance Criteria

- Validation errors accumulate across independent fields.
- Macro bindings preserve types and ownership without cloning unless the call site requests it.
- Configuration validation can reuse the same framework.
- Validation APIs do not require dynamic dispatch.
- No shared `FieldError` or serialized API/configuration error DTO is introduced in this story.

## Tests

- Unit tests for `Validation::valid`, `Validation::invalid`, `Validation::invalid_many`, `map`, `and_then`, and `into_result`.
- Macro tests for all-valid, one-invalid, multiple-invalid, ownership/borrowing, and trailing comma syntax.

## QA Guide

1. Run `make check`.
2. Run `make test-integration`.

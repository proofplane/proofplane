# 004 - Configuration System

## Goal

Load application configuration from YAML, selected by the `PROOFPLANE_CONFIG` environment variable.

## Design

Implement a typed configuration module. `PROOFPLANE_CONFIG` points to a YAML file path. Use the Rust `config` crate for file loading/deserialization, then validate into public config types with the applicative validation framework from story 003.

Configuration should cover:

- server bind addresses and ports
- Postgres connection settings
- Pub/Sub project, endpoint, topics, subscriptions, and dead-letter topics
- GCS bucket, endpoint override, credentials mode, and object key prefix
- logging format and default filters
- auth settings
- worker concurrency and retry settings
- liveness and readiness settings

Configuration parsing should separate deserialization from validation. Validation must use the applicative validation framework from story 003.

Secrets should use a redacting wrapper so passwords and credential material do not appear in debug output.

## Acceptance Criteria

- All binaries fail fast with a clear error if `PROOFPLANE_CONFIG` is unset, unreadable, malformed, or invalid.
- YAML config supports local development and can be reused by generated integration configs when story 009 adds that harness.
- Config validation accumulates all field errors instead of returning only the first error.
- Sensitive values are not printed in logs or debug output.
- Config structs are shared by API, worker, MCP, and seed.
- `PROOFPLANE_ENV` is fully replaced by `PROOFPLANE_CONFIG`.

## Tests

- Unit tests cover valid config loading.
- Unit tests cover missing file, invalid YAML, missing required fields, invalid numeric ranges, and invalid URLs.
- Validation tests assert multiple field errors are returned at once.
- Binary boot coverage with generated temp config is deferred to story 009.

## QA Guide

1. Run a binary with `PROOFPLANE_CONFIG` unset and confirm it exits clearly.
2. Run a binary with a valid local YAML config and confirm startup proceeds.
3. Break multiple config fields and confirm all validation errors are reported together.

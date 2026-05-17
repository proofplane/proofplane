# 003 - Configuration System

## Goal

Load application configuration from YAML, selected by the `PROOFPLANE_ENV` environment variable.

## Design

Implement a typed configuration crate. `PROOFPLANE_ENV` points to a YAML file path, not just an environment name.

Configuration should cover:

- server bind addresses and ports
- Postgres connection settings
- Pub/Sub project, endpoint, topics, subscriptions, and dead-letter topics
- GCS bucket, endpoint override, credentials mode, and object key prefix
- logging format and default filters
- auth settings
- worker concurrency and retry settings
- liveness and readiness settings

Configuration parsing should separate deserialization from validation. Validation must use the applicative validation framework introduced in story 004 when that story lands; before then it can expose a placeholder API.

## Acceptance Criteria

- All binaries fail fast with a clear error if `PROOFPLANE_ENV` is unset, unreadable, malformed, or invalid.
- YAML config supports local development and integration test examples.
- Config validation accumulates all field errors instead of returning only the first error.
- Sensitive values are not printed in logs or debug output.
- Config structs are shared by API, worker, MCP, seed, and integration tests.

## Tests

- Unit tests cover valid config loading.
- Unit tests cover missing file, invalid YAML, missing required fields, invalid numeric ranges, and invalid URLs.
- Validation tests assert multiple field errors are returned at once.
- Integration tests boot at least one binary with a generated temp config file.

## QA Guide

1. Run a binary with `PROOFPLANE_ENV` unset and confirm it exits clearly.
2. Run a binary with a valid local YAML config and confirm startup proceeds.
3. Break multiple config fields and confirm all validation errors are reported together.

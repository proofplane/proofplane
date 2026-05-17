# 008 - Database Migrations and Seed Data

## Goal

Add SQL migrations using `refinery` and a maintained seed script.

## Design

Use `refinery` with pure SQL migration files. Keep migration definitions in the migrations crate and expose a runner that API, worker, seed, and integration tests can call.

Initial schema should include only platform tables needed by near-term stories:

- schema migrations
- workspaces
- actors
- API keys or auth credentials
- audit events placeholder
- outbox messages placeholder

The seed binary should be idempotent and safe to run repeatedly in local development.

## Acceptance Criteria

- Migrations run from Rust using `refinery`.
- SQL files are the source of truth.
- Seed script creates useful local actors, credentials, and workspace data.
- Seed script is maintained as stories add tables and demo data.
- Migration runner can be used in integration tests against ephemeral Postgres.

## Tests

- Unit tests cover migration discovery where practical.
- Integration tests run migrations against a fresh testcontainers Postgres.
- Integration tests run the seed binary twice and verify idempotency.
- Repository smoke tests verify seeded records are queryable.

## QA Guide

1. Start Postgres.
2. Run migrations.
3. Run seed.
4. Run seed again and confirm no duplicates or errors.
5. Inspect core tables with `psql`.

# 008 - Database Migrations and Seed Data

## Goal

Add SQL migrations using `refinery` and a maintained seed script.

## Design

Use `refinery` with pure SQL migration files. Keep shared database startup utilities in `store`, including the migration runner used by API, worker, MCP, and seed.

Initial schema should include only platform tables needed by near-term stories:

- schema migrations
- workspaces
- actors
- API keys or auth credentials
- audit events placeholder
- outbox messages placeholder

The seed binary should run migrations, then idempotently create local workspace, actor, and API credential placeholder data.

## Acceptance Criteria

- Migrations run from Rust using `refinery`.
- SQL files are the source of truth.
- Seed script creates useful local actors, credentials, and workspace data.
- Seed script is maintained as stories add tables and demo data.
- Migrations are run on startup for API, worker, and MCP
- Seed is idempotent and safe to run repeatedly.

## Tests

- Unit tests cover migration discovery where practical.
- Postgres/testcontainers migration and seed coverage is deferred to story 009.
- Repository smoke tests are deferred until repository behavior exists.

## QA Guide

1. Start Postgres.
2. Run migrations.
3. Run seed.
4. Run seed again and confirm no duplicates or errors.
5. Inspect core tables with `psql`.

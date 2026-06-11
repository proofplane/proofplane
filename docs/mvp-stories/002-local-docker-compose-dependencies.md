# 002 - Local Docker Compose Dependencies

## Goal

Provide local infrastructure for development: Postgres, Google Cloud Pub/Sub
emulator through `deltio`, ClamAV, and a documented filesystem-backed object
storage location.

## Design

Create `docker-compose.yml` for local development. Include:

- Postgres with a stable database, username, password, and exposed port.
- Pub/Sub emulator using the `deltio` image or wrapper expected by the team.
- ClamAV with bundled signatures, TCP clamd access, and `freshclam` disabled.

Object storage is not run in Docker Compose for the MVP. Local configuration reserves `.local/storage` for the filesystem-backed object storage adapter that will be introduced in story 014.

Use named volumes for Postgres developer state and a documented clean-reset
command for destructive local resets. Local dependency readiness is checked
with `scripts/check-local-deps.sh`, which verifies Postgres, Pub/Sub, SpiceDB,
and clamd, and ensures `.local/storage` exists.

## Acceptance Criteria

- `docker compose up -d` starts all required services.
- Postgres accepts connections using checked-in local config values.
- Pub/Sub emulator is reachable at the checked-in local config endpoint.
- Clamd responds to `PING` at the checked-in scanner address.
- `.local/storage` is created for filesystem-backed object storage and ignored by Git.
- Compose health checks cover Postgres and the Pub/Sub emulator.
- `make up`, `make down`, `make health`, and `make reset-local` are documented.
- `CONTRIBUTING.md` documents local service endpoints and the filesystem object-storage decision.

## Tests

- Add a script or test command that verifies all compose services are reachable.
- Defer Docker-independent integration tests to story 009.
- Defer object storage adapter tests to story 014.

## QA Guide

1. Run `docker compose up -d`.
2. Run `make health`.
3. Connect to Postgres with `psql` using local config.
4. Confirm the Pub/Sub emulator is reachable at `127.0.0.1:8085`.
5. Confirm clamd responds at `127.0.0.1:3310`.
6. Run `make reset-local` and confirm `.local/storage` is recreated.

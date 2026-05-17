# 002 - Local Docker Compose Dependencies

## Goal

Provide local infrastructure for development and tests: Postgres, Google Cloud Pub/Sub emulator through `deltio`, and GCS-compatible object storage.

## Design

Create `docker-compose.yml` for local development. Include:

- Postgres with a stable database, username, password, and exposed port.
- Pub/Sub emulator using the `deltio` image or wrapper expected by the team.
- A GCS-compatible emulator if a reliable image is selected for local object storage.

Use named volumes for developer state and a documented clean-reset command for destructive local resets.

## Acceptance Criteria

- `docker compose up -d` starts all required services.
- Postgres accepts connections using checked-in local config values.
- Pub/Sub emulator supports topic and subscription creation.
- Object storage emulator supports bucket creation, upload, download, metadata, and delete flows.
- Compose health checks allow dependent commands to wait for readiness.
- Documentation explains how to reset local state.

## Tests

- Add a script or test command that verifies all compose services are reachable.
- Integration tests use testcontainers rather than assuming compose is already running.
- Add one integration test that creates and deletes a Pub/Sub topic and subscription.
- Add one integration test that writes and reads a test object.

## QA Guide

1. Run `docker compose up -d`.
2. Run the dependency health-check command.
3. Connect to Postgres with `psql` using local config.
4. Create a Pub/Sub topic and subscription against the emulator.
5. Upload and read a small object through the local storage endpoint.

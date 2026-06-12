# 024 - End-to-End Demo and Release Hardening

> Deferred and no longer an active execution plan. Production deployment needs
> a dedicated epic based on the infrastructure and operating model that exist
> when deployment work begins.

## Goal

Prove the MVP flow and harden the system for a demo-quality release.

## Design

Implement a scripted demo:

1. Bootstrap a workspace and actors.
2. Register an AI agent actor and credentials.
3. Query due Evidence Requests.
4. Submit evidence against an existing requirement.
5. Approve the submission.
6. Verify linked control status updates.
7. Query approved source material.
8. Inspect structured audit logs in the configured logging sink.
9. Show outbox publishing and worker processing.

Add operational hardening:

- graceful shutdown for every binary
- consistent readiness checks
- documented local runbook
- documented Kubernetes probe paths
- Dockerfiles if needed
- minimal deployment manifests or examples if needed

## Acceptance Criteria

- One command starts local dependencies.
- One command runs migrations and seed data.
- One command runs the API, worker, and MCP server locally or documents separate commands clearly.
- End-to-end tests cover the full demo flow.
- Kubernetes liveness and readiness probe paths are documented for API, worker, and MCP.
- Prometheus scraping paths are documented.
- Known MVP limitations are documented explicitly.

## Tests

- End-to-end integration test runs the complete evidence lifecycle.
- End-to-end test verifies outbox event publication.
- End-to-end test verifies worker handles at least one Pub/Sub message.
- End-to-end test verifies captured structured audit logs contain the expected
  chain.
- Smoke tests verify all binaries start and stop cleanly.

## QA Guide

1. Reset local state.
2. Start compose dependencies.
3. Run migrations and seed.
4. Start API, worker, and MCP.
5. Run the scripted demo.
6. Run the end-to-end test suite.
7. Review application/audit logs, metrics, and dead-letter topics for unexpected failures.

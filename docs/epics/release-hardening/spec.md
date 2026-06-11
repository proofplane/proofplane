# Release Hardening Spec

## Goal

Turn the backend MVP into a reproducible release candidate with one documented
local flow, production-ready process behavior, deployment examples, and an
end-to-end test that reflects the actual no-approval domain.

## Release Flow

The scripted flow:

1. provision or use a workspace owner;
2. create a workspace actor and API key;
3. list due Evidence Requests and mapped controls;
4. create a submission and upload an attachment;
5. run dequeuer, worker, ClamAV scan, and finalization;
6. retrieve the latest submission, issue a download grant, and download it;
7. create/search trusted source material;
8. preview and export an auditor packet;
9. perform representative MCP reads/writes;
10. inspect structured audit logs and runtime metrics.

No approval, rejection, or derived control-status transition is expected.

## Process Contract

API, worker, dequeuer, and MCP:

- run migrations before serving or polling;
- expose or document liveness, readiness, and metrics;
- reject startup when required dependencies/config are invalid;
- stop accepting new work on shutdown and honor a bounded grace period;
- exit non-zero on unrecoverable runtime failure.

The dequeuer may expose health through a small HTTP endpoint or a documented
process-level probe; choose one consistent with the deployment target.

## Deployment Artifacts

Provide production Dockerfiles or one multi-stage Dockerfile with binary
targets, example environment/config templates without secrets, Cloud Run or
Kubernetes deployment examples, probe/scrape paths, and required service
dependencies.

The runbook covers migrations, Auth0, SpiceDB schema, Pub/Sub topics and push
subscription, GCS bucket lifecycle, ClamAV, the audit-log sink and retention,
rollback, dead letters, and common failure diagnostics.

## Release Gate

`make check` and the focused end-to-end integration target pass from a clean
checkout with documented prerequisites. Known MVP limitations are explicit,
including no native approvals, no auditor guest accounts, and no retention UI.

## Revisions

- 2026-06-11: Removed approval/control-status steps from legacy story 024 and
  added the actual evidence, trusted-read, packet, MCP, and audit flow.

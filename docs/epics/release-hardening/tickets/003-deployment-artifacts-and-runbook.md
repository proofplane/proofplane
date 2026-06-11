# 003 - Deployment Artifacts And Runbook

**Status:** Todo · **Depends on:** production-runtime-adapters/003, 002 · **Spec:** [spec.md](../spec.md#deployment-artifacts)

**Summary** - Provide build artifacts, example deployment configuration, and an
operator runbook for the production dependency set.

**Acceptance criteria**

- [ ] Given a clean build environment, when container images are built, then all
  long-running binaries have minimal runnable images.
- [ ] Given the example deployment, when configured with external secrets, then
  probe, scrape, Pub/Sub push, GCS, SpiceDB, Auth0, Postgres, and ClamAV settings
  are represented.
- [ ] Given a common dependency or dead-letter failure, when the runbook is
  followed, then an operator can identify the affected process and recovery
  action without exposing secrets.

**Tasks**

- [ ] Add multi-stage container build targets.
- [ ] Add deployment examples and secret-free config templates.
- [ ] Document migrations, schema apply, bucket lifecycle, and subscriptions.
- [ ] Document rollback, dead-letter, and failure diagnosis.

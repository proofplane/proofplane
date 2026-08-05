# Production Deployment Epic

Deploy Proofplane to a dedicated GCP project without hiding application
readiness gaps. Infrastructure is reproducible in Terraform, releases are
digest-pinned and migration-ordered, and public or privileged paths are narrow
by default.

Full topology, release gates, and operational decisions live in
[spec.md](./spec.md), the source of technical depth. The system-wide deployment
decision is recorded in
[ADR 0002](../../adr/0002-deploy-production-on-cloud-run.md). Operator procedure
lives in the [production runbook](../../runbooks/production-deployment.md).

## Tracking

Tickets live on GitHub, not in this directory.

- Epic: [#114 Epic: Production Deployment](https://github.com/proofplane/proofplane/issues/114)
- Tickets: attached to that issue as sub-issues, and labeled
  [`epic:production-deployment`](https://github.com/proofplane/proofplane/issues?q=is%3Aissue+label%3Aepic%3Aproduction-deployment)

The epic issue carries the ticket index, status, and sequencing. See
[`docs/agents/issue-tracker.md`](../../agents/issue-tracker.md) for the
workflow.

**All Terraform in `infra/gcp/production/` is written but has never been
applied.** Tickets labeled `doing` have partially completed task lists; their
acceptance criteria describe applied infrastructure and remain unverified. The
first apply is blocked on
[#115](https://github.com/proofplane/proofplane/issues/115) and
[#117](https://github.com/proofplane/proofplane/issues/117).
